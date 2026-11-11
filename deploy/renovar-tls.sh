#!/usr/bin/env bash
# Renova o certificado HTTPS do Tailscale e recarrega o Nginx se ele mudou.
# Roda como root, uma vez por dia (cron instalado por instalar.sh).
# O `tailscale cert` só pede um certificado novo quando o atual está perto de vencer.

set -euo pipefail

BASE=/opt/nexus-deploy
DOMINIO=$(tailscale status --json | python3 -c 'import json,sys; print(json.load(sys.stdin)["Self"]["DNSName"].rstrip("."))')

antes=$(sha256sum "$BASE/tls/cert.pem" 2>/dev/null || true)
install -d -m 755 "$BASE/tls"
tailscale cert --cert-file "$BASE/tls/cert.pem" --key-file "$BASE/tls/key.pem" "$DOMINIO" >/dev/null
chmod 644 "$BASE/tls/cert.pem"
chmod 600 "$BASE/tls/key.pem"
depois=$(sha256sum "$BASE/tls/cert.pem")

if [ "$antes" != "$depois" ] && docker compose -f "$BASE/compose.yml" ps -q --status running nginx | grep -q .; then
  docker compose -f "$BASE/compose.yml" exec -T nginx nginx -s reload
  echo "certificado renovado para $DOMINIO; nginx recarregado"
fi
