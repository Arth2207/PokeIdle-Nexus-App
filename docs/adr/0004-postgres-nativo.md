# 0004. PostgreSQL continua fora do Docker

- Status: aceito
- Data: 2026-11-11

## Contexto

O plano inicial colocava o Postgres em container junto com o resto. O banco atual é PostgreSQL 16 nativo, com TimescaleDB, pgvector e pg_trgm, e tem dados de produção com hypertables já particionadas.

Dockerizar exigiria uma imagem com as três extensões nas mesmas versões, um dump e restore com o TimescaleDB (que tem procedimento próprio) e uma janela de indisponibilidade. Tudo isso no meio de uma migração em que o Node e o Nexus dependem do mesmo banco.

## Decisão

O Postgres continua nativo no host. Os containers acessam pelo socket Unix (`/var/run/postgresql` montado como volume), com um papel próprio (`nexus_app`) e permissões mínimas. O banco continua escutando só em localhost.

## Consequências

- Nenhum risco para os dados durante a migração.
- O backup existente continua valendo.
- O desempenho de disco não passa pela camada de volumes do Docker.
- Os testes de integração usam um Postgres com TimescaleDB em container (testcontainers), então o ambiente de teste não depende do host.
- A decisão pode ser revista depois da fase 7, quando o Node não existir mais. Nesse caso, a imagem base é `timescale/timescaledb-ha`, que já inclui o pgvector.
