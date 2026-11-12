# syntax=docker/dockerfile:1

# Build em quatro estágios:
#   chef     -> ferramentas de build
#   planner  -> gera a "receita" das dependências (muda pouco)
#   builder  -> compila dependências (com cache) e depois o projeto
#   final    -> só o binário, sobre uma imagem sem shell nem libc
#
# O binário sai estático (musl), então a imagem final não precisa de glibc.

ARG RUST_VERSION=1.99.1

FROM rust:${RUST_VERSION}-alpine AS chef
RUN apk add --no-cache musl-dev \
 && cargo install cargo-chef --locked
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --locked --recipe-path recipe.json
COPY . .
# As queries do sqlx foram validadas no desenvolvimento e estão em .sqlx/
ENV SQLX_OFFLINE=true
RUN cargo build --release --locked -p nexus-api -p nexus-worker \
 && cp target/release/nexus-api /nexus-api \
 && cp target/release/nexus-worker /nexus-worker

FROM gcr.io/distroless/static-debian12:nonroot
LABEL org.opencontainers.image.source="https://github.com/Arth2207/PokeIdle-Nexus-App" \
      org.opencontainers.image.description="Nexus API + Worker" \
      org.opencontainers.image.licenses="MIT"
COPY --from=builder /nexus-api /nexus-api
COPY --from=builder /nexus-worker /nexus-worker
USER nonroot:nonroot
ENV NEXUS_ENDERECO=0.0.0.0 \
    NEXUS_LOG_FORMATO=json
EXPOSE 8190 9190
ENTRYPOINT ["/nexus-api"]
