# Rotas

Inventário das rotas do sistema legado, que precisam existir no Nexus com o mesmo contrato. Durante a migração, cada rota só é dada como pronta depois que a paridade foi confirmada (veja [testes](testes.md)).

Convenções do legado que mantemos:

- Respostas em JSON, chaves em português e em camelCase.
- Erros como `{ "erro": "mensagem" }`. O formato novo, `{ "erro": { "codigo", "mensagem" } }`, só entra depois do cutover, junto com uma nova versão da API.
- Identificador de ticker: `p:<especie>:<...>:<moeda>` para pokémon e `i:<item>:<moeda>` para item.
- Moeda: `orb` ou `gold`.

## Bolsa

| Método | Rota | Parâmetros | Resposta | Consumidor |
|---|---|---|---|---|
| GET | `/api/bolsa/mercado` | `tipo`, `moeda` | Resumo por sinal (comprar, vender, observar...) e lista de tickers ordenada por volume em 30 dias | painel |
| GET | `/api/bolsa/ticker` | `id` | Ticker completo: indicadores, candles, últimas 30 vendas e até 200 ofertas ativas | painel |
| GET | `/api/bolsa/balao` | `id` | Indicadores resumidos: RSI, Bollinger, MACD, VWAP, drawdown, suporte | painel |
| GET | `/api/bolsa/serie` | `id` | `{ id, vendas: [[timestamp, preco]] }`, até 5000 pontos | painel |
| GET | `/api/bolsa/feedback` | `tipo`, `moeda`, `acao`, `q`, `limite` (10–200), `pagina` | Lista paginada de recomendações | painel |
| GET | `/api/bolsa/dicas` | `tipo`, `moeda` | Top 15 para comprar, top 15 para vender e oportunidades abaixo do valor justo | painel |
| GET | `/api/bolsa/ofertas` | `tipo`, `moeda`, `limite` (≤ 500) | Ofertas ativas avaliadas | painel |
| POST | `/api/bolsa/ofertas` | `{ ofertas: [...] (≤ 5000), completo: bool }` | `{ ok, recebidas, validas, gravadas }` | relay |
| POST | `/api/bolsa/resumo-itens` | `{ resumo: { [itemId]: { minOrb, minGold, anuncios } } }` | `{ ok, itens, gravadas }` | relay |
| GET | `/api/bolsa/alvos-coleta` | `nick`, `max` | Espécies a coletar e faltas | relay |
| GET | `/api/bolsa/meus-itens` | `moeda` | Inventário valorizado, com recomendação de venda | painel |
| POST | `/api/bolsa/meus-itens` | `{ itens, limpar, somar }` | `{ ok, itens }` | painel, relay |
| GET | `/api/bolsa/equipe` | `boss`, `moeda`, `nick`, `idade` | Melhor equipe a comprar. Responde `202` com progresso enquanto calcula | painel |
| GET | `/api/bolsa/equipe-propria` | `boss`, `nick` | Melhor equipe com os pokémon da própria conta | painel |

No Nexus, as rotas de leitura (`GET`) exigem o escopo `leitura`, e as de escrita (`POST`) exigem `relay` ou `escrita`. As de escrita respondem `503` enquanto `NEXUS_ESCRITA_BOLSA` estiver desligada (veja [operação](operacao.md#escrita-da-bolsa)). Ticker inexistente ou id mal formado dá `404` com `{ "erro": "ticker não encontrado" }`, como no legado.

`equipe`, `equipe-propria` e `alvos-coleta` chegaram na fase 2. O cálculo roda num pool de CPU separado (`NEXUS_CPU_THREADS`): `GET /api/bolsa/equipe` responde direto quando termina em até 1,5 s, senão `202` com o resultado anterior e o progresso, como no legado. Pedidos iguais esperam o mesmo cálculo, e quando o painel troca de boss o cálculo antigo da mesma conta é cancelado. O `GET /api/bolsa/dicas` volta a aquecer o cálculo da equipe padrão.

Comportamentos que precisam ser preservados:

- `POST /api/bolsa/ofertas` com `completo: true` desativa as ofertas que não apareceram nessa coleta.
- `GET /api/bolsa/dicas` dispara em segundo plano o cálculo de equipe, para aquecer o cache.
- `GET /api/bolsa/equipe` é assíncrona: a primeira chamada inicia o job, e as seguintes devolvem `202` até o resultado sair.

## Equipes, raros e shiny

| Método | Rota | Consumidor |
|---|---|---|
| POST | `/api/equipes/ingest` | relay |
| GET | `/api/equipes/bosses` | painel |
| GET | `/api/equipes/plano` | painel (usa o motor de combate) |
| GET | `/api/equipe/melhor` | painel |
| GET | `/api/equipe/melhor/todas` | painel |
| POST | `/api/raros/ingest` | relay |
| GET | `/api/raros`, `/api/raros2` | painel, relay |
| POST | `/api/shiny/registrar` | relay |
| GET | `/api/shiny` | painel |

No Nexus, `GET /api/equipes/bosses` (`nick`) e `GET /api/equipes/plano` (`nick`, `boss`, `perfil=realista|ideal`) exigem o escopo `leitura` e devolvem o mesmo JSON do legado, conferido pelo comparador de paridade. `POST /api/equipes/ingest` exige `relay` ou `escrita` e responde `503` enquanto `NEXUS_ESCRITA_EQUIPES` estiver desligada; até o cutover, o estado chega ao Nexus pela importação do `equipes-estado.json`.

## Fila de comandos e ingestão

| Método | Rota | Função |
|---|---|---|
| GET | `/api/comandos` | Relay busca comandos pendentes. **Substituída pelo WebSocket** (veja [websocket](websocket.md)) |
| POST | `/api/confirmar-comando` | Relay informa o resultado de um comando |
| POST | `/api/inserir-comando` | Insere um comando na fila |
| POST | `/api/hunt-sync` | Sincroniza o estado de hunt |
| POST | `/ingest` | Ingestão de eventos brutos capturados no jogo |

## Automação (motor)

Cerca de 45 rotas que hoje vivem no motor do `holodex-nexus`. São agrupadas assim:

| Grupo | Rotas |
|---|---|
| Estado e configuração | `/api/estado`, `/api/config`, `/api/backup`, `/api/pausa`, `/api/retomar`, `/api/auto-set`, `/api/versao` |
| Builds | `/api/builds`, `/api/builds/sugerir`, `/api/builds/teorica`, `/api/builds/simular`, `/api/builds/aplicar`, `/api/builds/prioridade` |
| Venda | `/api/vender/previa`, `/api/vender/confirmar`, `/api/vender/aplicar` |
| Hunt e combate | `/api/hunt`, `/api/combate`, `/api/espolio`, `/api/historico`, `/api/financeiro` |
| Itens | `/api/pedras/plano`, `/api/pedras/diag`, `/api/mercado/favoritar` |
| Diagnóstico | `/api/diagnostico`, `/api/cache-linhas`, `/api/dev/*` |

O detalhamento dessas rotas entra neste documento quando a fase 4 começar.

## IA

| Método | Rota |
|---|---|
| GET | `/api/ia/status` |
| GET | `/api/ia/dashboard` |
| POST | `/api/ia/controle` |

## Rotas novas no Nexus

| Método | Rota | Parâmetros | Resposta |
|---|---|---|---|
| POST | `/api/boss/tentativa` | `{ nick, boss, nivelBoss?, tiposBoss?, venceu, duracaoS, equipe, danoCausado?, danoRecebido?, eventos? }` | Grava uma tentativa de boss (escopo `relay` ou `escrita`, só com `NEXUS_ESCRITA_EQUIPES`) |
| GET | `/api/bolsa/equipe-hibrida` | `boss`, `nick`, `moeda` | A equipe com os pokémon da conta, comprando só as vagas que faltam: slots com `origem` (`conta` ou `compra`), `custoCompra`, `custoComprarTudo` e `economia` |

### Fase 3: ingestão, raros e fila (atrás de flags)

Escopo `relay` (ou `escrita`) nas rotas de escrita e na fila; `leitura` em `GET /api/raros2` e `GET /api/shiny`. Token preso a uma conta só grava e lê a própria. Corpo vazio ou JSON inválido vira `{}`, como no legado. Desligada a flag da rota, a resposta é `503`.

| Método | Rota | Flag | Contrato |
|---|---|---|---|
| POST | `/ingest` | `NEXUS_ESCRITA_INGESTAO` | `{conta_id, eventos[], nick?}` → `{ok, erros, total}` (`ok` é contagem). Grava em `nexus.evento` com chave de idempotência: reenviar o lote não duplica (exceto evento sem `ts` válido: grava com o instante da chegada e não é idempotente; conta em `nexus_ingestao_sem_ts_total`). A conta gravada é o `nick` normalizado quando vem, senão o `conta_id`; o `conta_id` fica em coluna própria. Token de conta só aceita lote cujo `nick` é a conta dele (lote sem `nick` exige token global, 403) Tipo desconhecido conta em `erros`. Não grava `dim_conta` |
| POST | `/api/hunt-sync` | `NEXUS_ESCRITA_INGESTAO` | `{eventos: [{nick, hunt_slug}]}` → `{atualizados}`, em `nexus.config_hunt`. Nick fora do escopo do token é pulado (não conta em `atualizados`), sem derrubar o lote |
| POST | `/api/raros/ingest` | `NEXUS_ESCRITA_RAROS` | `{raros: {itens}, historico?}` → `{ok, itens}`. Token de conta não grava (o estado é global) |
| POST | `/api/shiny/registrar` | `NEXUS_ESCRITA_RAROS` | `{nick, dia?, shiny?, bolas?}` → `{ok, nick, dia, shiny, bolas}` |
| GET | `/api/raros2` | (leitura) | `nick`, `notaMin`, `meta`. Lê `nexus.estado_raros` e o estado de equipes; não grava `fato_cofre`. Com token de conta, o campo `contas` de cada item só traz a própria conta. A nota da carta vem só da ficha (o `notaFicha` do legado não foi portado) |
| GET | `/api/shiny` | (leitura) | `nick` |
| GET | `/api/comandos` | `NEXUS_ESCRITA_COMANDOS` | `?nick=` → `{nick, comandos: [{id, cmd, params, origem}]}`. Reserva até 10 pendentes, com lease de 120 s |
| POST | `/api/confirmar-comando` | `NEXUS_ESCRITA_COMANDOS` | `{id, resultado?}` → `{ok, id}`. Idempotente. Só confirma comando já entregue (um nunca entregue segue pendente, mesma resposta); token de conta só confirma comando da própria conta (`403`) |
| POST | `/api/inserir-comando` | `NEXUS_ESCRITA_COMANDOS` | `{nick, cmd, params?, origem?, ts_expira_min?}` → `{ok, id}`. `ts_expira_min` acima de 1440 é limitado a 1440 (como no legado) e abaixo de -1440 é `400`; `params: null` é preservado como no legado |

Na fila, o comando reservado e não confirmado em 120 s volta a pendente; depois de 5 reservas sem confirmação sai da fila com `erro:sem confirmacao`. Com `NEXUS_MELHORIAS`, a ordem é prioridade maior primeiro (o legado entrega o menor número primeiro) e `ts_expira_min` fica entre 1 e 1440. `POST /api/boss/tentativa` aceita `chave` (a chave gravada é `sha256(conta|chave)`, sem truncar; sem `chave`, deriva `sha256(conta|boss|em)`, com `em` float arredondado) e responde `{ok, duplicada}`.

`GET /api/bolsa/equipe` ganhou campos novos (o contrato antigo continua igual): `exata` (a melhor equipe por dano pela busca exata, comparada com a da heurística, com `provado` e `distancia` até o ótimo), `curva` (fronteira custo × poder de equipes completas) e, com `orcamento=<gemas>`, `orcamento` (a melhor equipe até esse valor).


| Método | Rota | Função |
|---|---|---|
| GET | `/health/live` | O processo está de pé |
| GET | `/health/ready` | O processo consegue falar com o banco |
| GET | `/metrics` | Métricas Prometheus (acessível só pela rede interna) |
| GET | `/ws` | Upgrade para WebSocket (fase 3, ainda não existe) |
