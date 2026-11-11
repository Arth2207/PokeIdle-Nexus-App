# Banco de dados

## Visão geral

- **PostgreSQL 16** com as extensões **TimescaleDB 2.31**, **pgvector 0.6** e **pg_trgm**.
- O banco continua rodando nativo no host, fora do Docker. Motivo na [ADR 0004](adr/0004-postgres-nativo.md).
- Timestamps são `bigint` em milissegundos desde a época Unix, herança do legado. O Nexus mantém esse formato e converte para `time::OffsetDateTime` só na camada de domínio.
- Não há funções nem triggers definidos pelo projeto. Toda a lógica fica na aplicação.

## Tabelas da bolsa

### `fato_venda_mercado`

Vendas concluídas no mercado do jogo. Cerca de 43 mil linhas, 27 MB.

| Coluna | Tipo | Observação |
|---|---|---|
| `id` | bigint | PK, id da venda no jogo |
| `em` | bigint | momento da venda, indexado |
| `lido_em` | bigint | quando capturamos |
| `tipo` | text | `pokemon` ou `item` |
| `moeda` | text | `orb` ou `gold` |
| `preco` | bigint | |
| `descricao` | text | |
| `species_id` | integer | |
| `item_id` | integer | |
| `quantidade` | integer | |
| `ficha` | jsonb | atributos do pokémon vendido |

### `fato_oferta_mercado`

Anúncios ativos. Cerca de 23 mil linhas, 23 MB. A mesma oferta é vista várias vezes, por isso a gravação é um upsert pelo `id`.

| Coluna | Tipo | Observação |
|---|---|---|
| `id` | bigint | PK |
| `ts_primeiro` | bigint | primeira vez que vimos |
| `ts_visto` | bigint | última vez que vimos |
| `ativo` | boolean | índice `(ativo, ts_visto DESC)` |
| `tipo`, `moeda`, `preco`, `species_id`, `item_id`, `quantidade`, `nome` | | |
| `ficha` | jsonb | |
| `expira_em` | bigint | |

### `fato_resumo_item`

Preço mínimo por item e moeda, por instante. PK `(ts, item_id, moeda)`; a gravação usa `ON CONFLICT DO NOTHING` e por isso é idempotente.

## Hypertables (TimescaleDB)

| Tabela | Tamanho | Chunks |
|---|---|---|
| `fato_kill` | 115 MB | 8 |
| `fato_compra_venda` | 4,4 MB | 28 |
| `fato_inventario_snapshot` | 1,2 MB | 13 |
| `raw_evento`, `fato_boss_tentativa`, `fato_preco_mercado`, `fato_rodizio_sessao` | vazias | 0 |

Nenhuma tem compressão nem política de retenção configurada. Ligar as duas faz parte da fase 7.

## Views

| View | Uso |
|---|---|
| `v_hunt_recomendacao` | Base do scoring. Junta `dim_conta`, `fato_calibracao` e `fato_sessao_contexto` |
| `v_scoring_params_vigentes` | Parâmetros ativos do scoring |
| `v_dossier_pendente` | Pendências do dossiê |
| `v_radar_raros` | Radar de raros |
| `v_inventario_atual` | Último inventário por conta |
| `v_ia_performance` | Desempenho das decisões da IA |

## Quem escreve o quê

Durante a migração, Node e Rust escrevem no mesmo banco. Este mapa define quem é o dono de cada tabela em cada momento. **Uma tabela só muda de dono no cutover da fase correspondente.** Nunca há dois escritores ativos com lógicas diferentes.

| Tabela | Escritor no legado | Muda para o Nexus na fase |
|---|---|---|
| `fato_oferta_mercado`, `fato_resumo_item` | mapper (bolsa) | 1 |
| `fato_venda_mercado`, `pokemon_ficha` | dossie | 5 |
| `comando_fila` | mapper, ai-decisao, boss-builder, inserir-comando | 3 (inserção pela API), 6 (rotinas de IA) |
| `config_ia` | mapper, compras-ai, boss-builder, sessao-monitor, ai-decisao | 6 |
| `dim_conta` | mapper, ai-contas, scoring | 3 |
| `fato_kill`, `fato_sessao_contexto`, `dim_hunt` | etl-continuo | 5 |
| `fato_hunt_eficiencia` | etl-eficiencia | 5 |
| `fato_preco_mercado` | etl-mercado | 5 |
| `ia_decisao`, `ia_scoring_params`, `raw_evento` | scoring | 6 |
| `ia_observacao` | ai-observer | 6 |
| `experimento_hunt` | ai-decisao | 6 |
| `fato_cofre` | raros | 3 |

### Fila de comandos

A `comando_fila` tem quatro produtores e um consumidor. Para o consumo continuar seguro enquanto Node e Rust convivem, a reserva de um comando usa `SELECT ... FOR UPDATE SKIP LOCKED`: dois consumidores nunca pegam o mesmo comando. Um trigger aditivo publica `NOTIFY comando_fila` a cada inserção, e é isso que alimenta o WebSocket.

## Migrations

- Gerenciadas com `sqlx-cli` em `migrations/`.
- **Até o cutover final, as migrations são somente aditivas**: tabelas novas no schema `nexus`, índices novos, triggers de notificação. Nada é alterado ou removido no schema `public`, porque o Node ainda depende dele.
- A primeira migration é um baseline gerado a partir do schema atual, para que um banco vazio suba idêntico ao de produção nos testes.

## O que entra no Postgres

| Hoje | Destino |
|---|---|
| SQLite `mapa.db` (~600 MB, eventos brutos do coletor) | Hypertables no schema `nexus`, com compressão após 7 dias e retenção configurável |
| `meus-itens` (JSON) | `nexus.meus_itens` |
| `equipes-estado.json`, `raros-estado.json` | `nexus.estado_equipes`, `nexus.estado_raros` |
| `holodex-dados.json` (estado do motor) | Tabelas por responsabilidade no schema `nexus`, desenhadas na fase 4 |

Motivo na [ADR 0006](adr/0006-consolidar-estado-no-postgres.md).
