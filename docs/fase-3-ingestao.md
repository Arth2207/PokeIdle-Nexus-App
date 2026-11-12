# Fase 3: ingestão, comandos e WebSocket

Plano da fase 3. Resume o que o legado faz hoje, o que o Nexus vai fazer igual e o que vai fazer melhor. O levantamento da etapa 1 está em [Levantamento](#levantamento-etapa-1).

## O que existe hoje

| Peça | Onde | O que faz |
|---|---|---|
| Ingestão de eventos | `POST /ingest` (`server.js`) | Recebe lotes de eventos do jogo (kills, drops, capturas…) e grava no SQLite do coletor; na primeira vez de uma conta, registra `dim_conta` no Postgres |
| Sincronização de hunt | `POST /api/hunt-sync` | Atualiza a hunt atual de cada conta |
| Fila de comandos | `GET /api/comandos`, `POST /api/confirmar-comando`, `POST /api/inserir-comando` | O relay pergunta de tempos em tempos se há comando; a reserva é atômica (`UPDATE … FOR UPDATE SKIP LOCKED`) na tabela `comando_fila` |
| Raros e shiny | `raros.js`: `POST /api/raros/ingest`, `GET /api/raros2`, `POST /api/shiny/registrar`, `GET /api/shiny` | Estado de itens raros por conta e registro de shiny, em `raros-estado.json` |
| Equipes | `POST /api/equipes/ingest` | Já portada na fase 2 (atrás de `NEXUS_ESCRITA_EQUIPES`) |
| Relay do PC | `holodex-relay.js` | Lê o jogo pelo motor local e envia tudo isso por HTTP, sem token |

## Levantamento (etapa 1)

Feito lendo o código do legado e o SQLite do coletor em modo só leitura (novembro de 2026).

### Volume

- O coletor por navegador parou; hoje só o relay alimenta o SQLite. Tipos vivos: `kill` e `perfil`. Capturas são raras; `drop`, `bola`, `batalha`, mercado e inventário não chegam.
- Kills: ~47/min no total (10–13/min por conta), pico de ~260/min; 70–85 mil por dia. `perfil`: ~5/min.
- Lote típico em `/ingest`: ~8 eventos (~1 KB) por conta a cada 30 s. Hypertable com `UNNEST` sobra.
- **7,6% das kills estão duplicadas** (lote reenviado quando o relay perde o cursor ou reinicia). Chave de idempotência: (conta, instante em ms, nome, xp do treinador, xp do pokémon, ouro). O relay novo deve mandar também o `seq` do HUD, que hoje é descartado, para a chave ficar exata.
- Há linhas com instante sentinela (`1234567890000`): descartar.

### Contrato

- **`POST /ingest`** responde `{ok: <contagem>, erros, total}` (`ok` é número). Tipo desconhecido conta como erro, sem derrubar o lote. Registra a conta (`ON CONFLICT DO NOTHING`) antes dos eventos, só se vier o nick. Sem dedup.
- **JSON inválido vira `{}`** em todas as rotas (nunca 400 por corpo malformado); não há limite de corpo. Respostas são JSON indentado; o 500 expõe o stack. O Nexus mantém os status e os campos, mas não o stack.
- **Nick:** `/ingest` normaliza (minúsculas, sem espaços nas pontas); `/hunt-sync` e as rotas de comando usam como veio. O Nexus normaliza na entrada e compara normalizado.
- **`/api/hunt-sync`** nunca responde 400; se o registro da conta falhar, a config já foi gravada mas não entra na contagem.
- **Fila de comandos:** `comando_fila` não tem coluna de status; o estado vem de `ts_reservado`/`ts_executado`. `GET /api/comandos` reserva (`UPDATE … RETURNING`, sem ordem garantida) e **nada devolve à fila um comando reservado e não confirmado**. Comando de tipo desconhecido nunca é confirmado. `ts_expira_min` aceita negativo (comando nasce expirado); `0` vira 5.
- **Tipos de comando:** `config` (hunt, recarregar, limiar de HP, compra no mercado), `reload`, e os de equipe (`add`/`set`). Quem insere: decisão da IA, montador de boss, painel, monitor de sessão e compras; o `coordinator.js` é só relatório.
- **Raros e shiny:** `GET /api/raros` não existe no mapper (é rota do motor local). Estado em memória e em `raros-estado.json` (gravação atômica, debounce de 1,5 s, merge de deltas com tratamento de reset do contador) — **um único escritor por vez**. `GET /api/raros2` grava em `fato_cofre` (1×/h por conta) e soma o total com shiny, mas o registro grava sem shiny (bug do legado, reproduzir só na leitura). `/api/raros2` e `/api/shiny` usam as notas e os valores de ficha da fase 2. O dia do shiny é UTC; os dias restantes usam o fuso local.
- **Auth:** rede confiável passa direto; fora dela, token por query, `x-api-token` ou Bearer.

### Relay

- Node puro, ciclo de 30 s lendo o estado do motor local. Envia `/ingest`, `/api/hunt-sync` (~90 s), `/api/equipes/ingest` (~5 min), `/api/raros/ingest`. Sem retry: só o `/ingest` reenvia (não avança o cursor em erro); o resto é fire-and-forget.
- Polling de comandos a cada ~60 s por conta; confirma com `ok` ou `erro:…`.
- Contas e conta global estão fixas no código: o relay novo lê de configuração.
- **Lutas de boss:** o relay tem as fichas da equipe, mas não detecta início, fim, resultado nem eventos `batalha`. **O envio de tentativas depende de o motor local expor esses eventos.** `POST /api/boss/tentativa` não é idempotente hoje: ganha chave (conta, boss, instante de início) antes do relay novo.

### Consequências para o plano

1. Ingestão com `ON CONFLICT DO NOTHING` na chave acima desde o primeiro dia.
2. Fila com prazo de reserva (lease) e devolução automática; tipo desconhecido é confirmado como erro.
3. Raros: o Nexus lê o `raros-estado.json` em shadow e só passa a escrever no cutover (nunca dois escritores). `fato_cofre` fica com o Node até o cutover.
4. O lado do PC (relay, motor e coletor) é da fase A2 do app. Hoje o coletor do legado descarta as mensagens `batalha`; o coletor novo da A2 é que vai capturar as lutas de boss.

## Escopo

1. **Rotas com o mesmo contrato**, com escopo `relay` e token, atrás de flags de escrita até o cutover (como nas fases 1 e 2).
2. **WebSocket** (`/ws`, protocolo `nexus.v1`, veja [websocket](websocket.md)): o servidor empurra os comandos; o relay confirma pela mesma conexão. `GET /api/comandos` continua existindo para o relay antigo até a troca.
3. **Eventos brutos no Postgres** (hypertables do TimescaleDB no schema `nexus`, [ADR 0006](adr/0006-consolidar-estado-no-postgres.md)) em vez do SQLite.
4. **Tentativas de boss:** `POST /api/boss/tentativa` (já existe) fica idempotente, pronto para o cliente da A2 enviar cada luta. A captura em si é da A2.
5. **Raros e shiny** no Postgres (`nexus.estado_raros`), substituindo o `raros-estado.json`.

## O que o Nexus faz melhor

- **Comando entregue uma vez, sem polling.** O WebSocket entrega na hora; a confirmação é idempotente (id do comando) e um comando reservado e não confirmado volta para a fila depois de um prazo. Hoje um comando reservado por um relay que caiu fica preso.
- **Backpressure.** Fila por conexão com limite; se o relay não acompanha, o servidor para de empurrar e o comando espera na tabela, sem perder nada.
- **Ingestão idempotente.** Cada evento ganha uma chave (conta, tipo, instante, hash do conteúdo); reenviar o mesmo lote não duplica.
- **Token por conta** (já existe desde a fase 2): o relay de uma conta só grava a própria conta.
- **Lote grande sem travar a API.** A gravação dos eventos vai em `COPY`/`UNNEST` numa task própria; a rota responde assim que o lote é aceito.

## Testes

- **Paridade:** o comparador da fase 1 estendido para `GET /api/raros2` e `/api/shiny`.
- **Ingestão:** o mesmo lote gravado pelo legado e pelo Nexus produz as mesmas linhas (contagem por tipo e por conta, em testcontainers com o schema do legado).
- **WebSocket:** reconexão (o relay cai e volta sem perder nem duplicar comando), backpressure (relay lento), confirmação repetida, comando expirado.
- **Carga:** k6 com conexões WebSocket simultâneas e lotes de ingestão.

## Etapas

1. **Levantamento:** contrato exato de cada rota (corpo, respostas, erros), tabelas que cada uma escreve, volume real de eventos por minuto (do SQLite do coletor, só leitura).
2. **Raros e shiny** (leitura + ingestão atrás de flag), com o comparador.
3. **Ingestão de eventos** em hypertables, idempotente, atrás de flag; validação contra o SQLite.
4. **Fila de comandos** no Postgres (schema `nexus`) e as três rotas HTTP com o mesmo contrato.
5. **WebSocket** com reconexão, backpressure e confirmação; testes de falha.
6. **Tipos no `contrato`:** mensagens do WebSocket e corpos das rotas, para o cliente da A2 usar os mesmos tipos.
7. **Cutover planejado:** quais flags ligar e em que ordem, e como voltar atrás.

**Pronto quando:** os testes de reconexão e backpressure passam; 7 dias sem comando perdido nem duplicado (medido com o cliente da A2 em modo sombra). As tentativas de boss chegam com a A2.

## Fora desta fase

- Motor de automação (builds, venda, hunts): fase 4.
- ETL e dossiê: fase 5.
- Migrar o histórico do SQLite (só os eventos novos entram no Postgres aqui): fase 7.

## Ajustes da revisão

- **Espelho da fila:** no primeiro ciclo parte do maior id de `public.comando_fila` (não copia o histórico, que já está executado ou expirado). Depois reprocessa a janela dos 200 ids anteriores ao maior já visto (`ON CONFLICT DO NOTHING`), para pegar commits fora de ordem.
- **Confirmação:** só vale para comando entregue (`reservado_em` preenchido); HTTP e WebSocket usam a mesma checagem de dono.
- **Duplicados no mesmo milissegundo:** evento idêntico no mesmo ms continua deduplicado (é o que remove os 7,6% de duplicatas). O `seq` do HUD, enviado pelo cliente da A2, torna a chave exata.
- **Evento sem `ts`:** aceito com o instante da chegada; a chave inclui o instante, então esses eventos não são idempotentes (métrica `nexus_ingestao_sem_ts_total`).
- **Comparador:** `raros2` e `shiny` comparam sem `dadosEm`/`histEm`/`geradoEm`; as contas do shiny são comparadas por nick. `diasRestantes` usa o mês em UTC.
