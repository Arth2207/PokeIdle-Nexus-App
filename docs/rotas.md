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

| Método | Rota | Função |
|---|---|---|
| GET | `/health/live` | O processo está de pé |
| GET | `/health/ready` | O processo consegue falar com o banco |
| GET | `/metrics` | Métricas Prometheus (acessível só pela rede interna) |
| GET | `/ws` | Upgrade para WebSocket |
