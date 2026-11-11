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

### `nexus.item_catalogo`

Catálogo de itens do jogo (id, nome, categoria, raro, preço no NPC, ícone). Cerca de 420 linhas. Quase todas as vendas de item chegam sem `item_id`, só com a descrição (`"3 × Rare Candy"`), e o ticker é achado pelo nome neste catálogo. A coluna `ordem` guarda a ordem da fonte, porque há nomes repetidos e vale o último, como no legado. Carga e atualização em [operação](operacao.md#dados-da-bolsa).

### Precificador e inventário (schema `nexus`)

| Tabela | Conteúdo | Origem no legado |
|---|---|---|
| `nexus.especie` | Catálogo de espécies: status base, tipos, golpes, evoluções, megas. Cerca de 1.400 linhas | `engine/precificador/catalogo/especies*.js` |
| `nexus.venda_base` | Vendas antigas de pokémon já decompostas pelo precificador (preço base, adicionais, PvE, família). Cerca de 25 mil linhas | `engine/data/precos.json` |
| `nexus.item_preco_base` | Cotação diária de itens em orbs, da mesma base | `precos.json` |
| `nexus.parametro` | Pequenos valores: pesos da confiança, viés do precificador, carimbo do inventário | `precos.json`, `.bolsa-meus-itens.json` |
| `nexus.meus_itens` | Inventário de itens (item → quantidade) | `.bolsa-meus-itens.json` |

### Equipes (schema `nexus`)

| Tabela | Conteúdo | Origem no legado |
|---|---|---|
| `nexus.pokedex_motor` | Pokédex do motor de combate no formato compacto, na ordem do arquivo (a ordem desempata a equipe teórica). Cerca de 1.300 linhas | `engine/data/pokedex.json` |
| `nexus.equipes_estado` | Lista viva de bosses enviada pelo relay (linha única) | `equipes-estado.json` |
| `nexus.equipes_conta` | Por conta: nível de treinador, bosses vencidos e coleção, na ordem de chegada | `equipes-estado.json` |
| `nexus.boss_wiki` | Bosses da wiki do jogo: nível fixo, drops e entrada | `data/POKEIDLE-wiki.txt` |
| `nexus.falta_coleta` | Faltas de coleta por (boss, perfil): pedem mais espécies aos alvos | `.bolsa-faltas.json` |
| `nexus.equipe_sugestao` | Uma linha por plano de equipe calculado, base da calibração | `equipe-sugestoes.jsonl` |
| `nexus.boss_tentativa` | Tentativas de boss (equipe completa, resultado, duração, dano e eventos), para calibrar o modelo do jogo. Recebe dados a partir da fase 3 | (novo) |

Carga e atualização em [operação](operacao.md#dados-da-bolsa).

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

## Catálogo de hunts

A tabela `dim_hunt` tem hoje só 94 hunts, com os campos de nível e espécies vazios. O jogo envia o catálogo completo na mensagem inicial (`hunts`, com 1.042 entradas: área, região, nível, espécies com peso, coordenada no mapa). O Nexus passa a gravar esse catálogo numa tabela própria (`nexus.hunt`), com o bioma derivado, a partir da fase 3 (ingestão).

## Migrations

- Gerenciadas com `sqlx-cli` em `migrations/`.
- **Até o cutover final, as migrations são somente aditivas**: tabelas novas no schema `nexus`, índices novos, triggers de notificação. Nada é alterado ou removido no schema `public`, porque o Node ainda depende dele.
- As tabelas do `public` que o Nexus usa têm uma baseline em `migrations-teste/`, só para os testes de integração. Ela nunca é embutida no binário. Veja [testes](testes.md#integração).

## O que entra no Postgres

| Hoje | Destino |
|---|---|
| SQLite `mapa.db` (~600 MB, eventos brutos do coletor) | Hypertables no schema `nexus`, com compressão após 7 dias e retenção configurável |
| `meus-itens` (JSON) | `nexus.meus_itens` |
| `equipes-estado.json` | `nexus.equipes_estado`, `nexus.equipes_conta` |
| `raros-estado.json` | `nexus.estado_raros` |
| `holodex-dados.json` (estado do motor) | Tabelas por responsabilidade no schema `nexus`, desenhadas na fase 4 |

Motivo na [ADR 0006](adr/0006-consolidar-estado-no-postgres.md).
