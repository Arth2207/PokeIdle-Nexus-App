#!/usr/bin/env bash
# Prepara a VPS para receber deploys do Nexus. Rodar como root, a partir da raiz do repo.
# Pode ser executado de novo para atualizar compose, nginx.conf e deploy.sh.
#
#   sudo deploy/instalar.sh
#
# Na primeira vez também:
#   - cria o usuário nexus-deploy (grupo docker, sem senha)
#   - gera a chave SSH que o GitHub Actions usa, restrita ao deploy.sh
#   - cria o secret database_url a partir de NEXUS_DATABASE_URL_SOCKET (variável de ambiente)

set -euo pipefail

BASE=/opt/nexus-deploy
USUARIO=nexus-deploy
RAIZ=$(cd "$(dirname "$0")/.." && pwd)

[ "$(id -u)" -eq 0 ] || { echo "rode como root" >&2; exit 1; }

if ! id "$USUARIO" >/dev/null 2>&1; then
  useradd --system --create-home --shell /bin/bash --groups docker "$USUARIO"
  passwd -l "$USUARIO" >/dev/null
fi

install -d -o "$USUARIO" -g "$USUARIO" -m 750 "$BASE" "$BASE/estado" "$BASE/nginx" "$BASE/nginx/upstream"
install -d -o root -g root -m 755 "$BASE/secrets"
install -o root -g root -m 755 "$RAIZ/deploy/deploy.sh" "$BASE/deploy.sh"
install -o "$USUARIO" -g "$USUARIO" -m 640 "$RAIZ/deploy/compose.yml" "$BASE/compose.yml"
install -o "$USUARIO" -g "$USUARIO" -m 644 "$RAIZ/deploy/nginx/nginx.conf" "$BASE/nginx/nginx.conf"
# O container do nginx precisa conseguir ler a pasta do upstream.
chmod 755 "$BASE" "$BASE/nginx" "$BASE/nginx/upstream"

# Secret do banco: legível só pelo usuário dos containers (nonroot = 65532).
if [ ! -f "$BASE/secrets/database_url" ]; then
  : "${NEXUS_DATABASE_URL_SOCKET:?defina NEXUS_DATABASE_URL_SOCKET na primeira instalação}"
  printf '%s' "$NEXUS_DATABASE_URL_SOCKET" > "$BASE/secrets/database_url"
  chown 65532:65532 "$BASE/secrets/database_url"
  chmod 400 "$BASE/secrets/database_url"
fi

# Chave do CI: só pode rodar o deploy.sh, sem shell, sem túneis.
CHAVE=/home/$USUARIO/.ssh/ci_deploy
if [ ! -f "$CHAVE" ]; then
  install -d -o "$USUARIO" -g "$USUARIO" -m 700 "/home/$USUARIO/.ssh"
  ssh-keygen -t ed25519 -N "" -C "github-actions-deploy" -f "$CHAVE" -q
  printf 'command="%s",no-port-forwarding,no-X11-forwarding,no-agent-forwarding,no-pty %s\n' \
    "$BASE/deploy.sh" "$(cat "$CHAVE.pub")" > "/home/$USUARIO/.ssh/authorized_keys"
  chown -R "$USUARIO:$USUARIO" "/home/$USUARIO/.ssh"
  chmod 600 "/home/$USUARIO/.ssh/authorized_keys"
  echo
  echo ">>> Cadastre a chave privada abaixo no GitHub como secret VPS_SSH_KEY"
  echo ">>> e depois apague o arquivo: rm $CHAVE"
  echo
  cat "$CHAVE"
fi

echo "instalação concluída em $BASE"
