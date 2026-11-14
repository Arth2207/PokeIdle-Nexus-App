#!/usr/bin/env bash
# Virada da Fase 3: o app (A2) vira o relay oficial e o Electron do PC sai.
#
#   cutover-fase3.sh checar   confere os pré-requisitos (não muda nada)
#   cutover-fase3.sh on       liga raros e comandos no Nexus e manda as rotas do relay pela borda
#   cutover-fase3.sh off      rollback: rotas de volta ao mapper e escrita de raros/comandos desligada
#   cutover-fase3.sh status
#
# Antes do "on" (o que depende do dono, no PC):
#   1. As 4 contas no app (Configurações › Contas), modo Ativo, e Conectar.
#   2. Electron (motor + relay) fechado.
#   3. `checar` sem pendência.
#
# Depende das fases 1 e 2 viradas (borda no ar). Nada apaga dado; o mapper continua de pé.
set -Eeuo pipefail

BASE=/opt/nexus-deploy
ESTADO="$BASE/estado"
MARCA="$ESTADO/fase3-cutover"
ROTAS="$BASE/borda/fase3-rotas.conf"
SQLITE=/opt/mapper/data/mapa.db
cd "$BASE"

log() { echo "[fase3 $(date -u +%H:%M:%S)] $*"; }
compose() { (set -a; . "$BASE/imagens.env"; set +a; docker compose -f "$BASE/compose.yml" --profile fase12 "$@"); }
cor_ativa() { cat "$ESTADO/cor-ativa"; }

flag() { # flag <VAR> <true|false>: troca (ou cria, depois de NEXUS_ESCRITA_INGESTAO) no x-api
  if ! grep -qE "^\s+$1: \"(true|false)\"" "$BASE/compose.yml"; then
    sed -i -E "0,/^(\s+)NEXUS_ESCRITA_INGESTAO: \"(true|false)\"/s//&\n\1$1: \"false\"/" "$BASE/compose.yml"
  fi
  sed -i -E "s/^(\s+$1: )\"(true|false)\"/\1\"$2\"/" "$BASE/compose.yml"
  grep -qE "^\s+$1: \"$2\"" "$BASE/compose.yml" || { log "flag $1 não ficou $2"; return 1; }
  chown nexus-deploy:nexus-deploy "$BASE/compose.yml"
}

recriar_api() {
  compose up -d --no-deps --force-recreate "nexus-api-$(cor_ativa)"
  for _ in $(seq 1 60); do
    [ "$(docker inspect -f '{{.State.Health.Status}}' "nexus-nexus-api-$(cor_ativa)-1" 2>/dev/null)" = healthy ] && return 0
    sleep 2
  done
  log "API não ficou saudável"; return 1
}

recarregar_borda() { compose exec -T nexus-borda sh -c "nginx -t -c /etc/nginx/borda/nginx.conf && kill -HUP 1"; }

# Eventos do Electron nos últimos 15 min (o relay antigo grava no mapa.db).
eventos_electron() {
  local desde=$(( ($(date +%s) - 900) * 1000 ))
  sqlite3 -readonly "$SQLITE" "SELECT count(*) FROM evento_kill WHERE ts > $desde" 2>/dev/null || echo "?"
}

# Contas mandando eventos pelo app nos últimos 15 min.
contas_app() {
  sudo -u postgres psql -d pokeidle -XAtc \
    "SELECT string_agg(DISTINCT conta, ',') FROM nexus.evento WHERE tipo = 'kill' AND em > now() - interval '15 minutes'"
}

cmd_checar() {
  local ok=1 e c
  e=$(eventos_electron); c=$(contas_app)
  log "abates do Electron (mapa.db) nos últimos 15 min: $e"
  log "contas no app nos últimos 15 min: ${c:-nenhuma}"
  [ "$e" = 0 ] || { log "PENDENTE: o Electron ainda está mandando (feche o motor e o relay no PC)"; ok=0; }
  [ "$(tr ',' '\n' <<<"$c" | grep -c .)" -ge 4 ] || { log "PENDENTE: as 4 contas ainda não estão no app"; ok=0; }
  [ -f "$ESTADO/fase12-cutover" ] || { log "PENDENTE: fases 1 e 2 não estão viradas"; ok=0; }
  [ $ok = 1 ] && log "pré-requisitos ok" || return 1
}

cmd_on() {
  cmd_checar || { log "use 'on forcar' para virar mesmo assim"; [ "${2:-}" = forcar ] || exit 1; }
  trap 'log "falhou; desfazendo"; cmd_off' ERR
  cp -p "$BASE/compose.yml" "$BASE/compose.yml.bak-$(date -u +%Y%m%d%H%M)-fase3"
  flag NEXUS_ESCRITA_RAROS true
  flag NEXUS_ESCRITA_COMANDOS true
  recriar_api
  install -o nexus-deploy -g nexus-deploy -m 644 "$ROTAS.off" "$ROTAS"
  recarregar_borda
  touch "$MARCA"
  trap - ERR
  log "fase 3 virada: o app é o relay oficial. Rollback: $0 off"
}

cmd_off() {
  set +e
  trap - ERR
  rm -f "$MARCA" "$ROTAS"
  recarregar_borda
  flag NEXUS_ESCRITA_RAROS false
  flag NEXUS_ESCRITA_COMANDOS false
  recriar_api || log "aviso: API não ficou saudável; conferir com deploy.sh status"
  log "rollback: rotas do relay de volta ao mapper; raros e comandos desligados no Nexus"
  set -e
}

cmd_status() {
  echo "fase 3 virada: $([ -f "$MARCA" ] && echo sim || echo não)"
  echo "  rotas da fase 3 na borda: $([ -f "$ROTAS" ] && echo ligadas || echo desligadas)"
  grep -E '^\s+NEXUS_ESCRITA_(RAROS|COMANDOS|INGESTAO):' "$BASE/compose.yml" | sed 's/^ */  /' || true
  echo "  abates do Electron (15 min): $(eventos_electron) · contas no app (15 min): $(contas_app)"
}

case "${1:-}" in
  checar) cmd_checar ;;
  on) cmd_on "$@" ;;
  off) cmd_off ;;
  status) cmd_status ;;
  *) sed -n '2,13p' "$0"; exit 2 ;;
esac
