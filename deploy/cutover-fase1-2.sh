#!/usr/bin/env bash
# Cutover das fases 1 (bolsa) e 2 (equipes) do Nexus, com rollback, sem mexer no relay do PC.
#
#   cutover-fase1-2.sh grants   GRANTs de escrita do nexus_app nas tabelas da bolsa (uma vez)
#   cutover-fase1-2.sh token    cria o token da borda (leitura + escrita) em borda/token.conf
#   cutover-fase1-2.sh teste    sobe a nexus-borda (sem desviar tráfego) e testa os dois lados
#   cutover-fase1-2.sh on       liga a escrita no Nexus e desvia a porta 8181 da Tailscale
#   cutover-fase1-2.sh off      rollback: devolve a porta ao mapper e desliga a escrita
#   cutover-fase1-2.sh status
#
# Como funciona: o relay e o painel chamam o mapper na porta 8181 pela Tailscale. Uma regra
# REDIRECT manda esse tráfego para a nexus-borda (8183), que entrega /api/bolsa/* e
# /api/equipes/* ao Nexus e o resto ao mapper. O tráfego local (127.0.0.1:8181) não muda.
# Roda como root. Nada apaga dado; o mapper continua de pé para o rollback.
set -euo pipefail

BASE=/opt/nexus-deploy
ESTADO="$BASE/estado"
MARCA="$ESTADO/fase12-cutover"
IP_TS=100.115.131.15
UNIDADE=nexus-borda-redirect.service
cd "$BASE"

log() { echo "[fase12 $(date -u +%H:%M:%S)] $*"; }
compose() { (set -a; . "$BASE/imagens.env"; set +a; docker compose -f "$BASE/compose.yml" --profile fase12 "$@"); }
cor_ativa() { cat "$ESTADO/cor-ativa"; }

flag() { # flag <VAR> <true|false>: troca o valor no bloco x-api
  sed -i -E "s/^(\s+$1: )\"(true|false)\"/\1\"$2\"/" "$BASE/compose.yml"
  grep -qE "^\s+$1: \"$2\"" "$BASE/compose.yml" || { log "flag $1 não encontrada no compose"; exit 1; }
  chown nexus-deploy:nexus-deploy "$BASE/compose.yml"
}

esperar_api() {
  local id status
  id=$(compose ps -q "nexus-api-$(cor_ativa)")
  for _ in $(seq 1 45); do
    status=$(docker inspect -f '{{.State.Health.Status}}' "$id" 2>/dev/null || echo ausente)
    [ "$status" = healthy ] && return 0
    sleep 2
  done
  log "nexus-api-$(cor_ativa) não ficou saudável ($status)"
  return 1
}

recriar_api() {
  compose up -d --no-deps --force-recreate "nexus-api-$(cor_ativa)"
  esperar_api
}

# HTTP status de uma URL (000 se não conectar).
http() { curl -s -o /dev/null -m 30 -w '%{http_code}' "$@" || true; }

cmd_grants() {
  sudo -u holodex psql -d pokeidle -XAtq -v ON_ERROR_STOP=1 <<'SQL'
GRANT INSERT, UPDATE ON public.fato_oferta_mercado, public.fato_resumo_item TO nexus_app;
SQL
  log "GRANTs aplicados"
}

cmd_token() {
  if [ -s "$BASE/borda/token.conf" ]; then log "token já existe (borda/token.conf)"; return 0; fi
  local saida token
  saida=$(compose exec -T "nexus-api-$(cor_ativa)" /nexus-api token criar borda-fase12 leitura,escrita)
  token=$(printf '%s\n' "$saida" | awk 'NF{l=$0} END{print l}')
  [[ "$token" =~ ^[A-Za-z0-9_-]{20,}$ ]] || { log "não consegui ler o token criado"; exit 1; }
  umask 077
  printf 'proxy_set_header Authorization "Bearer %s";\n' "$token" > "$BASE/borda/token.conf"
  chown nexus-deploy:nexus-deploy "$BASE/borda/token.conf"
  log "token da borda criado (nexus.api_token: borda-fase12)"
}

cmd_teste() {
  [ -s "$BASE/borda/token.conf" ] || { log "rode 'token' antes"; exit 1; }
  compose up -d --no-deps nexus-borda
  sleep 2
  local nexus legado
  nexus=$(http "http://$IP_TS:8183/api/bolsa/mercado?tipo=item&moeda=gold")
  legado=$(http "http://$IP_TS:8183/api/raros2")
  log "borda → Nexus (/api/bolsa/mercado): $nexus · borda → mapper (/api/raros2): $legado"
  [ "$nexus" = 200 ] && [ "$legado" = 200 ] || { log "teste falhou"; exit 1; }
}

regra() { # regra <-C|-I|-D>
  iptables -t nat "$1" PREROUTING -i tailscale0 -p tcp --dport 8181 -j REDIRECT --to-ports 8183
}

cmd_redirect_on() { regra -C 2>/dev/null || regra -I; }
cmd_redirect_off() { while regra -C 2>/dev/null; do regra -D; done; }

cmd_on() {
  [ -s "$BASE/borda/token.conf" ] || { log "rode 'grants' e 'token' antes"; exit 1; }
  log "ligando NEXUS_ESCRITA_BOLSA e NEXUS_ESCRITA_EQUIPES"
  flag NEXUS_ESCRITA_BOLSA true
  flag NEXUS_ESCRITA_EQUIPES true
  recriar_api
  # O nginx do Nexus monta o arquivo de configuração: recriar para ler os locations novos.
  compose up -d --no-deps --force-recreate nginx
  cmd_teste
  install -m 644 "$BASE/borda/$UNIDADE" "/etc/systemd/system/$UNIDADE"
  systemctl daemon-reload
  systemctl enable --now "$UNIDADE"
  # Conexões keep-alive já abertas com o mapper não passam pelo REDIRECT: derrubar as da
  # Tailscale; o cliente reconecta e cai na borda.
  ss -K state established '( sport = :8181 )' dst 100.64.0.0/10 >/dev/null || true
  touch "$MARCA"
  log "fases 1 e 2 viradas: bolsa e equipes no Nexus (rollback: $0 off)"
}

cmd_off() {
  systemctl disable --now "$UNIDADE" 2>/dev/null || true
  cmd_redirect_off
  ss -K state established '( sport = :8183 )' >/dev/null || true
  flag NEXUS_ESCRITA_BOLSA false
  flag NEXUS_ESCRITA_EQUIPES false
  recriar_api
  compose stop nexus-borda || true
  rm -f "$MARCA"
  log "rollback: porta 8181 de volta ao mapper, escrita do Nexus desligada"
}

cmd_status() {
  echo "fases 1 e 2 viradas: $([ -f "$MARCA" ] && echo sim || echo não)"
  echo "  regra REDIRECT 8181→8183: $(regra -C 2>/dev/null && echo ativa || echo ausente)"
  echo "  $UNIDADE: $(systemctl is-enabled "$UNIDADE" 2>/dev/null || true)"
  grep -E '^\s+NEXUS_ESCRITA_(BOLSA|EQUIPES):' "$BASE/compose.yml" | sed 's/^ */  /'
  compose ps --format '  {{.Name}} {{.Status}}' nexus-borda "nexus-api-$(cor_ativa)" 2>/dev/null || true
}

case "${1:-}" in
  grants) cmd_grants ;;
  token) cmd_token ;;
  teste) cmd_teste ;;
  on) cmd_on ;;
  off) cmd_off ;;
  redirect-on) cmd_redirect_on ;;
  redirect-off) cmd_redirect_off ;;
  status) cmd_status ;;
  *) sed -n '2,9p' "$0"; exit 2 ;;
esac
