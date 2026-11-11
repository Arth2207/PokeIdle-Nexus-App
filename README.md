# Nexus (Rust)

Reescrita do backend do Holodex Nexus em Rust. O sistema atual é um conjunto de processos Node.js que coleta dados de mercado e de combate do jogo PokeIdle, calcula indicadores de preço, monta equipes contra bosses e coordena a automação das contas a partir de um app no PC.

O Nexus substitui todos esses processos por dois binários Rust que compartilham um núcleo de domínio comum, rodando em Docker numa VPS pequena (2 vCPU, 8 GB de RAM). O app do PC, hoje em Electron, também é reescrito: Tauri 2 com núcleo em Rust e interface em Leptos ([ADR 0009](docs/adr/0009-app-desktop-tauri-leptos.md)).

## Status

| Fase | Escopo | Situação |
|---|---|---|
| 0 | Fundação (config, erros, logs, health, métricas, auth, Docker, CI) | concluída |
| 1 | Bolsa | — |
| 2 | Motor de combate e otimizador de equipe | — |
| 3 | Ingestão, fila de comandos e WebSocket | — |
| 4 | Motor de automação | — |
| 5 | ETL e dossiê | — |
| 6 | Scoring e rotinas de IA | — |
| 7 | Desligamento do legado | — |

**App desktop**

| Fase | Escopo | Situação |
|---|---|---|
| A1 | Esqueleto Tauri + Leptos, telas da bolsa (só leitura) | — |
| A2 | Núcleo do relay em Rust, janela do jogo com o coletor | — |
| A3 | Painel completo de automação e versão web do painel | — |
| A4 | Instalador e atualização automática pelo CI | — |

Detalhes de cada fase e o critério de "pronto" estão em [docs/migracao.md](docs/migracao.md).

## Por que reescrever

Dois trechos do sistema são pesados em CPU: o motor de combate, que simula duelos de 400 ticks contra mais de mil espécies, e o otimizador de equipe, que faz beam search seguido de busca local e hoje já esbarra no limite de 3 segundos. No Node os dois rodam num único worker, em série. Em Rust, com `rayon`, os duelos e as sementes do otimizador rodam em paralelo, e o ganho por núcleo fica entre 10x e 50x.

O resto (rotas, ETL, scoring) não é gargalo, mas migrar tudo resolve outro problema: hoje há oito processos, dois bancos (Postgres e SQLite), estado espalhado em arquivos JSON e nenhum log de requisição. A reescrita é a oportunidade de deixar isso com cara de sistema de produção.

## Stack

- **Linguagem:** Rust (edição 2021)
- **HTTP:** axum + tower
- **Runtime async:** tokio
- **Paralelismo de CPU:** rayon
- **Banco:** PostgreSQL 16 + TimescaleDB via sqlx (queries checadas em tempo de compilação, modo offline)
- **Serialização:** serde / serde_json
- **Erros:** thiserror no domínio, anyhow nas bordas
- **Logs:** tracing (JSON em produção)
- **Agendamento:** tokio-cron-scheduler
- **Infra:** Docker multi-stage, Docker Compose, Nginx, GitHub Actions
- **App desktop:** Tauri 2, Leptos (WebAssembly)

## Documentação

| Documento | Conteúdo |
|---|---|
| [Arquitetura](docs/arquitetura.md) | Componentes, crates, fluxo de dados, concorrência |
| [Sistema legado](docs/sistema-legado.md) | Como o sistema em Node funciona hoje |
| [Rotas](docs/rotas.md) | Inventário de endpoints, com parâmetros e consumidores |
| [Banco de dados](docs/banco-de-dados.md) | Schema, volumes, quem escreve o quê |
| [WebSocket](docs/websocket.md) | Protocolo de push para o PC |
| [Segurança](docs/seguranca.md) | Autenticação, borda, segredos |
| [Infraestrutura](docs/infraestrutura.md) | Docker, Nginx, CI/CD, limites de recurso, observabilidade |
| [Migração](docs/migracao.md) | Fases, shadow traffic, cutover e rollback |
| [Operação](docs/operacao.md) | Deploy, rollback, tokens, logs: os comandos do dia a dia |
| [Testes](docs/testes.md) | Estratégia de testes e paridade |
| [Glossário](docs/glossario.md) | Termos do jogo e do domínio |
| [Telas do app](docs/design/telas.md) | Arquitetura de telas, KPIs e padrões de dashboard do app |
| [Decisões (ADRs)](docs/adr/README.md) | Registro das decisões de arquitetura |

## Estrutura do repositório (alvo)

```
nexus-rs/
├── crates/
│   ├── nucleo/          # domínio puro: bolsa, combate, otimizador, scoring
│   ├── persistencia/    # implementações Postgres dos repositórios
│   ├── nexus-api/       # binário HTTP + WebSocket
│   ├── nexus-worker/    # binário de ETL, scoring e rotinas agendadas
│   └── contrato/        # tipos compartilhados entre servidor e app (API e WebSocket)
├── apps/
│   └── desktop/         # app Tauri 2 + Leptos (substitui o Electron e o relay)
├── migrations/          # sqlx migrations (somente aditivas)
├── deploy/              # compose, nginx, deploy.sh, instalar.sh, monitoring
├── tests/               # integração e paridade
└── docs/
```
