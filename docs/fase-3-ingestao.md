# Fase 3: ingestão, comandos e WebSocket

Plano da fase 3. Resume o que o legado faz hoje, o que o Nexus vai fazer igual e o que vai fazer melhor. A investigação detalhada (números, arquivos) entra aqui no início da fase.

## O que existe hoje

| Peça | Onde | O que faz |
|---|---|---|
| Ingestão de eventos | `POST /ingest` (`server.js`) | Recebe lotes de eventos do jogo (kills, drops, capturas…) e grava no SQLite do coletor; na primeira vez de uma conta, registra `dim_conta` no Postgres |
| Sincronização de hunt | `POST /api/hunt-sync` | Atualiza a hunt atual de cada conta |
| Fila de comandos | `GET /api/comandos`, `POST /api/confirmar-comando`, `POST /api/inserir-comando` | O relay pergunta de tempos em tempos se há comando; a reserva é atômica (`UPDATE … FOR UPDATE SKIP LOCKED`) na tabela `comando_fila` |
| Raros e shiny | `raros.js`: `POST /api/raros/ingest`, `GET /api/raros`, `/api/raros2`, `POST /api/shiny/registrar`, `GET /api/shiny` | Estado de itens raros por conta e registro de shiny, em `raros-estado.json` |
| Equipes | `POST /api/equipes/ingest` | Já portada na fase 2 (atrás de `NEXUS_ESCRITA_EQUIPES`) |
| Relay do PC | `holodex-relay.js` | Lê o jogo pelo motor local e envia tudo isso por HTTP, sem token |

## Escopo

1. **Rotas com o mesmo contrato**, com escopo `relay` e token, atrás de flags de escrita até o cutover (como nas fases 1 e 2).
2. **WebSocket** (`/ws`, protocolo `nexus.v1`, veja [websocket](websocket.md)): o servidor empurra os comandos; o relay confirma pela mesma conexão. `GET /api/comandos` continua existindo para o relay antigo até a troca.
3. **Eventos brutos no Postgres** (hypertables do TimescaleDB no schema `nexus`, [ADR 0006](adr/0006-consolidar-estado-no-postgres.md)) em vez do SQLite.
4. **Tentativas de boss:** o relay passa a enviar cada luta de boss (equipe com as fichas completas, resultado, duração e os eventos `batalha`) para `POST /api/boss/tentativa`, que já existe. É o que destrava a calibração do modelo do jogo ([ADR 0010](adr/0010-paridade-primeiro-melhorias-depois.md)).
5. **Raros e shiny** no Postgres (`nexus.estado_raros`), substituindo o `raros-estado.json`.

## O que o Nexus faz melhor

- **Comando entregue uma vez, sem polling.** O WebSocket entrega na hora; a confirmação é idempotente (id do comando) e um comando reservado e não confirmado volta para a fila depois de um prazo. Hoje um comando reservado por um relay que caiu fica preso.
- **Backpressure.** Fila por conexão com limite; se o relay não acompanha, o servidor para de empurrar e o comando espera na tabela, sem perder nada.
- **Ingestão idempotente.** Cada evento ganha uma chave (conta, tipo, instante, hash do conteúdo); reenviar o mesmo lote não duplica.
- **Token por conta** (já existe desde a fase 2): o relay de uma conta só grava a própria conta.
- **Lote grande sem travar a API.** A gravação dos eventos vai em `COPY`/`UNNEST` numa task própria; a rota responde assim que o lote é aceito.

## Testes

- **Paridade:** o comparador da fase 1 estendido para `GET /api/raros`, `/api/raros2` e `/api/shiny`.
- **Ingestão:** o mesmo lote gravado pelo legado e pelo Nexus produz as mesmas linhas (contagem por tipo e por conta, em testcontainers com o schema do legado).
- **WebSocket:** reconexão (o relay cai e volta sem perder nem duplicar comando), backpressure (relay lento), confirmação repetida, comando expirado.
- **Carga:** k6 com conexões WebSocket simultâneas e lotes de ingestão.

## Etapas

1. **Levantamento:** contrato exato de cada rota (corpo, respostas, erros), tabelas que cada uma escreve, volume real de eventos por minuto (do SQLite do coletor, só leitura).
2. **Raros e shiny** (leitura + ingestão atrás de flag), com o comparador.
3. **Ingestão de eventos** em hypertables, idempotente, atrás de flag; validação contra o SQLite.
4. **Fila de comandos** no Postgres (schema `nexus`) e as três rotas HTTP com o mesmo contrato.
5. **WebSocket** com reconexão, backpressure e confirmação; testes de falha.
6. **Relay:** versão nova do `holodex-relay.js` com token, WebSocket e envio das tentativas de boss, em paralelo à atual até validar.
7. **Cutover planejado:** quais flags ligar e em que ordem, e como voltar atrás.

**Pronto quando:** os testes de reconexão e backpressure passam; 7 dias sem comando perdido nem duplicado; tentativas de boss chegando em `nexus.boss_tentativa`.

## Fora desta fase

- Motor de automação (builds, venda, hunts): fase 4.
- ETL e dossiê: fase 5.
- Migrar o histórico do SQLite (só os eventos novos entram no Postgres aqui): fase 7.
