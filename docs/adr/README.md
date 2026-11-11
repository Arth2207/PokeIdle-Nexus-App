# Registro de decisões de arquitetura

Cada decisão relevante fica registrada num ADR (Architecture Decision Record): o contexto, o que foi decidido e as consequências. Um ADR aceito não é editado. Se a decisão mudar, um novo ADR substitui o anterior, e o antigo é marcado como "substituído".

| # | Decisão | Status |
|---|---|---|
| [0001](0001-rust-puro.md) | Reescrever todo o backend em Rust | aceito |
| [0002](0002-monolito-modular.md) | Monólito modular com dois binários | aceito |
| [0003](0003-nginx-na-borda.md) | Nginx como ponto único de entrada | aceito |
| [0004](0004-postgres-nativo.md) | PostgreSQL continua fora do Docker | aceito |
| [0005](0005-composicao-e-traits.md) | Composição e traits no lugar de herança | aceito |
| [0006](0006-consolidar-estado-no-postgres.md) | Consolidar SQLite e arquivos JSON no Postgres | aceito |
| [0007](0007-websocket-no-lugar-de-polling.md) | WebSocket no lugar do polling da fila de comandos | aceito |
| [0008](0008-build-somente-no-ci.md) | Imagens compiladas só no CI | aceito |
| [0009](0009-app-desktop-tauri-leptos.md) | App desktop em Tauri 2 com interface em Leptos | aceito |

## Modelo

```markdown
# NNNN. Título

- Status: proposto | aceito | substituído por NNNN
- Data: AAAA-MM-DD

## Contexto
## Decisão
## Consequências
```
