#!/usr/bin/env bash
# Reimporta no schema nexus os catálogos que ainda vivem em arquivos do legado, só quando o
# arquivo mudou desde a última importação (compara o sha256).
#
# Roda como root pelo cron (/etc/cron.d/nexus-catalogos), porque os arquivos do legado não são
# legíveis pelo usuário nexus-deploy. Só lê o legado; a importação grava apenas no schema
# nexus, pela API ativa (`nexus-api importar`).
#
# O catálogo de espécies é um arquivo JS: os dois .js vão concatenados para
# `nexus-api importar especies-js`, que extrai os dados sem Node.

set -euo pipefail

BASE=/opt/nexus-deploy
LEGADO=${NEXUS_LEGADO:-/opt/mapper}
ESTADO="$BASE/estado/catalogos"
mkdir -p "$ESTADO"

log() { echo "$*"; }

# Não disputa com um deploy em andamento: tenta de novo na próxima rodada.
exec 9>"$BASE/estado/deploy.lock"
flock -n 9 || { log "deploy em andamento; fica para a próxima"; exit 0; }

cor=$(cat "$BASE/estado/cor-ativa" 2>/dev/null || true)
case "$cor" in
  blue|green) ;;
  *) log "sem cor ativa"; exit 1 ;;
esac

importar() { # <tipo> <arquivo>
  timeout 300 sudo -u nexus-deploy docker compose -f "$BASE/compose.yml" --project-directory "$BASE" \
    exec -T "nexus-api-$cor" /nexus-api importar "$1" < "$2"
}

sincronizar() { # <tipo> <arquivo já pronto para importar> <hash da origem>
  local tipo=$1 arquivo=$2 hash=$3
  [ "$(cat "$ESTADO/$tipo.sha256" 2>/dev/null || true)" = "$hash" ] && return 0
  if importar "$tipo" "$arquivo"; then
    echo "$hash" > "$ESTADO/$tipo.sha256"
    log "$tipo reimportado"
  else
    log "falha ao importar $tipo"
    falhou=1
  fi
}

falhou=0
for par in \
  "itens:$LEGADO/data/items-catalogo.json" \
  "base:$LEGADO/engine/data/precos.json" \
  "meus-itens:$LEGADO/data/.bolsa-meus-itens.json" \
  "pokedex:$LEGADO/engine/data/pokedex.json" \
  "equipes:$LEGADO/data/equipes-estado.json" \
  "wiki:$LEGADO/data/POKEIDLE-wiki.txt" \
  "faltas:$LEGADO/data/.bolsa-faltas.json" \
  "raros:$LEGADO/data/raros-estado.json"; do
  tipo=${par%%:*}
  arquivo=${par#*:}
  # Depois do cutover das fases 1 e 2 o Nexus é o dono desses estados: o arquivo do
  # legado para de ser a fonte (cutover-fase1-2.sh).
  if [ -f "$BASE/estado/fase12-cutover" ]; then
    case "$tipo" in equipes|faltas|meus-itens) continue ;; esac
  fi
  [ -r "$arquivo" ] || { log "$tipo: $arquivo não encontrado"; continue; }
  sincronizar "$tipo" "$arquivo" "$(sha256sum < "$arquivo" | cut -d' ' -f1)"
done

catalogo="$LEGADO/engine/precificador/catalogo"
if [ -r "$catalogo/especies.js" ]; then
  tmp=$(mktemp)
  trap 'rm -f "$tmp"' EXIT
  # Os dois .js concatenados; a API extrai PI_ESPECIES e o complemento (sem Node).
  cat "$catalogo/especies.js" "$catalogo/especies-extra.js" > "$tmp" 2>/dev/null || true
  sincronizar especies-js "$tmp" "$(sha256sum < "$tmp" | cut -d' ' -f1)"
fi

# Constantes do servidor: o último `welcome` gravado pelo coletor do legado (SQLite, só leitura).
mapa="$LEGADO/data/mapa.db"
if [ -r "$mapa" ] && command -v sqlite3 > /dev/null; then
  tmp_w=$(mktemp)
  if sqlite3 -readonly "$mapa" "SELECT payload FROM welcome_dump ORDER BY id DESC LIMIT 1" > "$tmp_w" 2>/dev/null && [ -s "$tmp_w" ]; then
    sincronizar welcome "$tmp_w" "$(sha256sum < "$tmp_w" | cut -d' ' -f1)"
  fi
  rm -f "$tmp_w"
fi

exit "$falhou"
