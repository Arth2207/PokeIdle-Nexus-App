#!/usr/bin/env bash
# Cutover das fases 1 (bolsa) e 2 (equipes) do Nexus, com rollback, sem mexer no relay do PC.
#
#   cutover-fase1-2.sh grants   GRANTs de escrita do nexus_app nas tabelas da bolsa (uma vez)
#   cutover-fase1-2.sh token    cria o token da borda (leitura + escrita) em borda/token.conf
#   cutover-fase1-2.sh teste    sobe a nexus-borda (sem desviar tráfego) e testa os dois lados
#   cutover-fase1-2.sh on       liga a escrita no Nexus e desvia a porta 8181 da Tailscale
#   cutover-fase1-2.sh off      rollback: devolve a porta ao mapper e desliga a escrita
#   cutover-fase1-2.sh status
#   cutover-fase1-2.sh guarda   (timer, a cada 15 s) regra só existe com a marca E a borda de pé
#
# Como funciona: o relay e o painel chamam o mapper na porta 8181 pela Tailscale. Uma regra
# REDIRECT manda esse tráfego para a nexus-borda (8183), que entrega /api/bolsa/* e
# /api/equipes/* ao Nexus e o resto ao mapper. O tráfego local (127.0.0.1:8181) não muda.
# Se a borda cair, a guarda tira a regra e o 8181 volta direto ao mapper (fail-open).
#
# Rollback (off): ofertas, vendas e resumo ficam no Postgres, comum aos dois. O que o Nexus
# gravou em equipes/faltas/meus-itens durante a virada NÃO volta aos arquivos do legado: o
# relay reenvia equipes e faltas; edições de meus-itens feitas nesse período se perdem.
# Roda como root. Nada apaga dado; o mapper continua de pé para o rollback.
set -Eeuo pipefail

BASE=/opt/nexus-deploy
ESTADO="$BASE/estado"
MARCA="$ESTADO/fase12-cutover"
IP_TS=100.115.131.15
TIMER=nexus-borda-guarda.timer
cd "$BASE"

log() { echo "[fase12 $(date -u +%H:%M:%S)] $*"; }
compose() { (set -a; . "$BASE/imagens.env"; set +a; docker compose -f "$BASE/compose.yml" --profile fase12 "$@"); }
cor_ativa() { cat "$ESTADO/cor-ativa"; }
outra_cor() { [ "$(cor_ativa)" = blue ] && echo green || echo blue; }

flag() { # flag <VAR> <true|false>: troca o valor no bloco x-api
  sed -i -E "s/^(\s+$1: )\"(true|false)\"/\1\"$2\"/" "$BASE/compose.yml"
  grep -qE "^\s+$1: \"$2\"" "$BASE/compose.yml" || { log "flag $1 não encontrada no compose"; return 1; }
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

# Recria a cor ativa com as flags atuais e recarrega o Nginx do Nexus (o IP do container
# muda). A cor inativa é recriada parada, para um rollback do deploy não voltar com as
# flags antigas.
recriar_api() {
  compose up -d --no-deps --force-recreate "nexus-api-$(cor_ativa)"
  esperar_api
  compose exec -T nginx nginx -s reload
  if [ -n "$(compose ps -aq "nexus-api-$(outra_cor)")" ]; then
    compose up --no-start --no-deps --force-recreate "nexus-api-$(outra_cor)" || log "aviso: cor inativa não recriada"
  fi
}

# HTTP status de uma URL (000 se não conectar).
http() { curl -s -o /dev/null -m "${2:-30}" -w '%{http_code}' "$1" || true; }
borda_saudavel() { [ "$(http "http://$IP_TS:8183/borda-saude" 3)" = 200 ]; }

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
  [ -s "$BASE/borda/token.conf" ] || { log "rode 'token' antes"; return 1; }
  compose up -d --no-deps nexus-borda
  for _ in $(seq 1 10); do borda_saudavel && break; sleep 1; done
  local nexus legado
  nexus=$(http "http://$IP_TS:8183/api/bolsa/mercado?tipo=item&moeda=gold")
  legado=$(http "http://$IP_TS:8183/api/raros2")
  log "borda → Nexus (/api/bolsa/mercado): $nexus · borda → mapper (/api/raros2): $legado"
  [ "$nexus" = 200 ] && [ "$legado" = 200 ] || { log "teste falhou"; return 1; }
}

regra() { # regra <-C|-I|-D>
  iptables -w 5 -t nat "$1" PREROUTING -i tailscale0 -p tcp --dport 8181 -j REDIRECT --to-ports 8183
}
redirect_on() { regra -C 2>/dev/null || regra -I; }
redirect_off() { while regra -C 2>/dev/null; do regra -D; done; }

# Timer: a regra só existe com a marca e com a borda respondendo.
cmd_guarda() {
  if [ -f "$MARCA" ] && borda_saudavel; then
    regra -C 2>/dev/null || { regra -I; log "guarda: regra aplicada"; }
  elif regra -C 2>/dev/null; then
    redirect_off
    log "guarda: borda fora do ar ou sem marca; 8181 de volta ao mapper"
  fi
}

conexoes_ts_8181() { ss -Htn state established '( sport = :8181 )' dst 100.64.0.0/10 | wc -l; }

cmd_on() {
  [ -s "$BASE/borda/token.conf" ] || { log "rode 'grants' e 'token' antes"; exit 1; }
  # Falha no meio: desfaz tudo.
  trap 'log "falhou; desfazendo"; cmd_off' ERR
  log "ligando NEXUS_ESCRITA_BOLSA e NEXUS_ESCRITA_EQUIPES"
  flag NEXUS_ESCRITA_BOLSA true
  flag NEXUS_ESCRITA_EQUIPES true
  recriar_api
  # O nginx do Nexus monta o arquivo de configuração: recriar para ler os locations novos.
  compose up -d --no-deps --force-recreate nginx
  cmd_teste
  # Marca antes do desvio: o sync de catálogos para de importar equipes/faltas/meus-itens.
  touch "$MARCA"
  install -m 644 "$BASE/borda/nexus-borda-guarda.service" /etc/systemd/system/
  install -m 644 "$BASE/borda/$TIMER" /etc/systemd/system/
  systemctl daemon-reload
  systemctl enable --now "$TIMER"
  cmd_guarda
  regra -C 2>/dev/null || { log "regra não aplicada"; false; }
  trap - ERR
  # Conexões keep-alive já abertas com o mapper não passam pelo REDIRECT: derrubar as da
  # Tailscale; o cliente reconecta e cai na borda.
  ss -K state established '( sport = :8181 )' dst 100.64.0.0/10 >/dev/null || true
  sleep 1
  log "fases 1 e 2 viradas (conexões da Tailscale ainda no mapper: $(conexoes_ts_8181)). Rollback: $0 off"
}

# Rollback best-effort: cada passo segue mesmo se o anterior falhar.
cmd_off() {
  set +e
  trap - ERR
  systemctl disable --now "$TIMER" 2>/dev/null
  redirect_off
  rm -f "$MARCA"
  ss -K state established '( sport = :8183 )' >/dev/null
  flag NEXUS_ESCRITA_BOLSA false
  flag NEXUS_ESCRITA_EQUIPES false
  recriar_api || log "aviso: API não ficou saudável; conferir com deploy.sh status"
  compose stop nexus-borda
  log "rollback: porta 8181 de volta ao mapper, escrita do Nexus desligada"
  set -e
}

cmd_status() {
  echo "fases 1 e 2 viradas: $([ -f "$MARCA" ] && echo sim || echo não)"
  echo "  regra REDIRECT 8181→8183: $(regra -C 2>/dev/null && echo ativa || echo ausente)"
  echo "  borda: $(borda_saudavel && echo saudável || echo fora)"
  echo "  $TIMER: $(systemctl is-active "$TIMER" 2>/dev/null || true)"
  echo "  conexões da Tailscale direto no mapper: $(conexoes_ts_8181)"
  grep -E '^\s+NEXUS_ESCRITA_(BOLSA|EQUIPES):' "$BASE/compose.yml" | sed 's/^ */  /'
  compose ps --format '  {{.Name}} {{.Status}}' nexus-borda "nexus-api-$(cor_ativa)" 2>/dev/null || true
}

case "${1:-}" in
  grants) cmd_grants ;;
  token) cmd_token ;;
  teste) cmd_teste ;;
  on) cmd_on ;;
  off) cmd_off ;;
  guarda) cmd_guarda ;;
  status) cmd_status ;;
  *) sed -n '2,10p' "$0"; exit 2 ;;
esac
