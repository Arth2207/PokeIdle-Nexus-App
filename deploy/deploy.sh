#!/usr/bin/env bash
# Deploy blue/green do nexus-api.
#
# Roda como o usuário nexus-deploy. O CI chega aqui por SSH com uma chave que só
# pode executar este script (forced command em authorized_keys).
#
#   deploy <sha>   sobe a imagem ghcr.io/arth2207/nexus-api:<sha> na cor inativa
#   rollback       volta para a cor anterior (que fica parada, mas não removida)
#   status         mostra a cor ativa e o estado dos containers
#
# O token do GHCR, quando necessário, chega pela entrada padrão.

set -euo pipefail

BASE=/opt/nexus-deploy
REGISTRO=ghcr.io/arth2207/nexus-api
cd "$BASE"

exec 9>"$BASE/estado/deploy.lock"
flock -n 9 || { echo "outro deploy em andamento" >&2; exit 1; }

compose() { docker compose -f "$BASE/compose.yml" "$@"; }
log() { printf '[deploy %s] %s\n' "$(date -u +%H:%M:%S)" "$*"; }

cor_ativa() { cat "$BASE/estado/cor-ativa" 2>/dev/null || echo nenhuma; }
outra_cor() { [ "$1" = blue ] && echo green || echo blue; }

esperar_saudavel() {
  local servico="nexus-api-$1" id status
  id=$(compose ps -q "$servico")
  for _ in $(seq 1 30); do
    status=$(docker inspect -f '{{.State.Health.Status}}' "$id" 2>/dev/null || echo ausente)
    [ "$status" = healthy ] && return 0
    sleep 2
  done
  log "$servico não ficou saudável (status: $status)"
  compose logs --tail 50 "$servico" >&2 || true
  return 1
}

apontar_nginx_para() {
  printf 'upstream nexus_api {\n    server nexus-api-%s:8190;\n    keepalive 16;\n}\n' "$1" \
    > "$BASE/nginx/upstream/ativo.conf"
  if [ -n "$(compose ps -q --status running nginx)" ]; then
    compose exec -T nginx nginx -t -q
    compose exec -T nginx nginx -s reload
  else
    compose up -d nginx
  fi
  echo "$1" > "$BASE/estado/cor-ativa"
}

definir_imagem() {
  local var="NEXUS_IMAGEM_${1^^}"
  touch "$BASE/imagens.env"
  sed -i "/^$var=/d" "$BASE/imagens.env"
  echo "$var=$2" >> "$BASE/imagens.env"
}

# Docker compose lê imagens.env também para interpolar ${NEXUS_IMAGEM_*}.
export_imagens() { set -a; . "$BASE/imagens.env"; set +a; }

cmd_deploy() {
  local sha=$1
  [[ $sha =~ ^[0-9a-f]{40}$ ]] || { echo "sha inválido" >&2; exit 2; }
  local imagem="$REGISTRO:$sha" ativa nova
  ativa=$(cor_ativa)
  nova=$(outra_cor "$ativa")

  if [ ! -t 0 ]; then
    docker login ghcr.io -u ci --password-stdin >/dev/null
    trap 'docker logout ghcr.io >/dev/null 2>&1 || true' EXIT
  fi

  log "baixando $imagem"
  docker pull -q "$imagem" >/dev/null

  definir_imagem "$nova" "$imagem"
  export_imagens

  # Migrations são sempre aditivas, então rodam com a versão antiga ainda no ar.
  log "aplicando migrations"
  compose run --rm --no-deps "nexus-api-$nova" migrar

  log "subindo $nova (ativa: $ativa)"
  compose up -d --no-deps --force-recreate "nexus-api-$nova"
  if ! esperar_saudavel "$nova"; then
    compose stop "nexus-api-$nova" || true
    log "deploy abortado; $ativa continua no ar"
    exit 1
  fi

  apontar_nginx_para "$nova"
  log "tráfego agora em $nova"

  if [ "$ativa" != nenhuma ]; then
    # SIGTERM: termina as requisições em andamento antes de sair.
    compose stop -t 30 "nexus-api-$ativa"
  fi
  docker image prune -f >/dev/null
  log "deploy concluído: $sha"
}

cmd_rollback() {
  local ativa anterior
  ativa=$(cor_ativa)
  [ "$ativa" = nenhuma ] && { echo "nada para voltar" >&2; exit 1; }
  anterior=$(outra_cor "$ativa")
  export_imagens
  log "voltando para $anterior"
  compose start "nexus-api-$anterior"
  esperar_saudavel "$anterior"
  apontar_nginx_para "$anterior"
  compose stop -t 30 "nexus-api-$ativa"
  log "rollback concluído"
}

cmd_status() {
  echo "cor ativa: $(cor_ativa)"
  export_imagens 2>/dev/null || true
  compose ps
}

# Via SSH o comando vem em SSH_ORIGINAL_COMMAND; localmente, nos argumentos.
read -r -a ARGS <<< "${SSH_ORIGINAL_COMMAND:-$*}"
case "${ARGS[0]:-}" in
  deploy)   cmd_deploy "${ARGS[1]:-}" ;;
  rollback) cmd_rollback ;;
  status)   cmd_status ;;
  *) echo "uso: deploy <sha> | rollback | status" >&2; exit 2 ;;
esac
