#!/usr/bin/env bash
# Monta a imagem do nexus-api na VPS (quando o CI do GitHub não está disponível).
#   imagem-local.sh   → ghcr.io/arth2207/nexus-api:<sha do HEAD>, depois: deploy.sh deploy <sha>
# Antes: fmt, clippy, testes e `cargo build --release -p nexus-api -p nexus-worker` no repo.
set -euo pipefail
REPO=/opt/nexus-rs
cd "$REPO"
[ -z "$(git status --porcelain -- crates migrations Cargo.toml Cargo.lock)" ] || { echo "há mudanças não commitadas"; exit 1; }
SHA=$(git rev-parse HEAD)
CTX=$(mktemp -d)
trap 'rm -rf "$CTX"' EXIT
cp target/release/nexus-api target/release/nexus-worker deploy/Dockerfile.local "$CTX/"
docker build -q -f "$CTX/Dockerfile.local" -t "ghcr.io/arth2207/nexus-api:$SHA" "$CTX"
echo "$SHA"
