#!/usr/bin/env bash
# Cutover da Fase 5 do Nexus (ETL + dossiê), com rollback.
#
#   cutover-fase5.sh grants       GRANTs de escrita para o nexus_app (uma vez)
#   cutover-fase5.sh etl-on       para o ETL do legado e liga a escrita do worker no public
#   cutover-fase5.sh etl-off      rollback: desliga a escrita do worker e religa o legado
#   cutover-fase5.sh dossie-on    para o dossie.service e sobe o nexus-dossie nas portas dele
#   cutover-fase5.sh dossie-off   rollback: derruba o nexus-dossie e religa o dossie.service
#   cutover-fase5.sh status
#
# Roda como root. Cada passo deixa o anterior desfazível; nada apaga dado.
set -euo pipefail

BASE=/opt/nexus-deploy
ESTADO="$BASE/estado"
CURSOR_LEGADO=/opt/mapper/data/etl-cursores.json
cd "$BASE"

log() { echo "[cutover $(date -u +%H:%M:%S)] $*"; }
compose() { (set -a; . "$BASE/imagens.env"; set +a; docker compose -f "$BASE/compose.yml" "$@"); }
psql_nexus() { psql "$(cat "$BASE/secrets/database_url")" -XAtq -v ON_ERROR_STOP=1 "$@"; }

flag() { # flag <VAR> <true|false>: troca o valor no bloco do nexus-worker
  sed -i -E "s/^(\s+$1: )\"(true|false)\"/\1\"$2\"/" "$BASE/compose.yml"
  chown nexus-deploy:nexus-deploy "$BASE/compose.yml"
}

cmd_grants() {
  sudo -u holodex psql -d pokeidle -XAtq -v ON_ERROR_STOP=1 <<'SQL'
GRANT INSERT ON public.fato_kill, public.fato_sessao_contexto, public.dim_hunt TO nexus_app;
GRANT INSERT, UPDATE, DELETE ON public.fato_hunt_eficiencia TO nexus_app;
GRANT INSERT ON public.fato_venda_mercado TO nexus_app;
GRANT INSERT, UPDATE, DELETE ON public.pokemon_ficha TO nexus_app;
GRANT USAGE ON SEQUENCE public.pokemon_ficha_id_seq TO nexus_app;
SQL
  log "GRANTs aplicados"
}

cmd_etl_on() {
  log "parando o ETL do legado (pm2 etl-continuo, etl-eficiencia)"
  pm2 stop etl-continuo etl-eficiencia >/dev/null
  pm2 save >/dev/null
  sleep 2
  # O cursor do Nexus andou na sombra; recua para o do legado para não pular nada.
  # ON CONFLICT (ts_at, ts_ms, conta_id) descarta o que o legado já gravou.
  psql_nexus -c "CREATE TABLE IF NOT EXISTS nexus.etl_cursor_bak_cutover AS SELECT * FROM nexus.etl_cursor"
  python3 - "$CURSOR_LEGADO" <<'PY' | psql_nexus
import json, sys
c = json.load(open(sys.argv[1]))
for conta, ts in c.items():
    print(f"UPDATE nexus.etl_cursor SET ts_ms = LEAST(ts_ms, {int(ts)}), atualizado_em = NOW() "
          f"WHERE conta_id = '{conta}' AND fonte = 'mapa.db';")
PY
  flag NEXUS_ESCRITA_ETL true
  compose up -d --no-deps --force-recreate nexus-worker
  touch "$ESTADO/etl-cutover"
  log "ETL virado: o worker grava no public (cursor do legado guardado em $CURSOR_LEGADO)"
}

cmd_etl_off() {
  flag NEXUS_ESCRITA_ETL false
  compose up -d --no-deps --force-recreate nexus-worker
  pm2 start etl-continuo etl-eficiencia >/dev/null
  pm2 save >/dev/null
  rm -f "$ESTADO/etl-cutover"
  log "rollback do ETL: legado religado (retoma do etl-cursores.json; ON CONFLICT evita duplicata)"
}

cmd_dossie_on() {
  log "parando o dossie.service do legado"
  systemctl disable --now dossie.service
  touch "$ESTADO/dossie-cutover"
  compose --profile cutover up -d --no-deps --force-recreate nexus-dossie
  # O comparador do dossiê não tem mais com quem comparar.
  compose stop nexus-dossie-paridade || true
  log "dossiê virado: nexus-dossie nas portas 8899 (loopback e Tailscale)"
}

cmd_dossie_off() {
  compose --profile cutover stop nexus-dossie || true
  rm -f "$ESTADO/dossie-cutover"
  systemctl enable --now dossie.service
  compose up -d --no-deps nexus-dossie-paridade || true
  log "rollback do dossiê: dossie.service religado"
}

cmd_status() {
  echo "ETL virado:    $([ -f "$ESTADO/etl-cutover" ] && echo sim || echo não)"
  echo "dossiê virado: $([ -f "$ESTADO/dossie-cutover" ] && echo sim || echo não)"
  pm2 ls 2>/dev/null | grep -E 'etl-' | awk -F'│' '{print "  pm2", $3, $10}'
  echo "  dossie.service: $(systemctl is-active dossie.service)"
  compose ps --format '  {{.Name}} {{.Status}}' nexus-worker nexus-dossie 2>/dev/null || true
}

case "${1:-}" in
  grants) cmd_grants ;;
  etl-on) cmd_etl_on ;;
  etl-off) cmd_etl_off ;;
  dossie-on) cmd_dossie_on ;;
  dossie-off) cmd_dossie_off ;;
  status) cmd_status ;;
  *) sed -n '2,10p' "$0"; exit 2 ;;
esac
