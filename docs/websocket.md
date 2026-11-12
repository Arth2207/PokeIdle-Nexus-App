# WebSocket

Hoje o relay do PC pergunta ao servidor, de tempos em tempos, se existe comando novo. Isso gera atraso (o comando espera o próximo ciclo) e requisições inúteis quando não há nada. O Nexus troca esse polling por uma conexão WebSocket persistente, e o servidor empurra os eventos assim que eles acontecem.

## Conexão

```
GET /ws
Authorization: Bearer <token>
Sec-WebSocket-Protocol: nexus.v1
```

- Sem token válido, o servidor responde `401` antes do upgrade; token sem escopo `relay` (ou `admin`) leva `403`.
- Sem o subprotocolo `nexus.v1` no pedido, a resposta é `400`.
- Com `NEXUS_ESCRITA_COMANDOS` e `NEXUS_ESPELHO_COMANDOS` desligadas, a resposta é `503`: o WebSocket só existe junto da fila de comandos.
- Token preso a uma conta só recebe (e só confirma) comandos dessa conta; token sem conta recebe os de todas.
- O subprotocolo `nexus.v1` versiona o formato. Uma mudança incompatível vira `nexus.v2`, e os dois convivem durante a transição.
- Um token pode ter mais de uma conexão aberta; cada uma recebe todos os eventos do seu escopo.

## Envelope

Toda mensagem, nos dois sentidos, é um objeto JSON:

```json
{
  "v": 1,
  "tipo": "comando.novo",
  "seq": 18234,
  "ts": 1762866000000,
  "dados": { }
}
```

| Campo | Descrição |
|---|---|
| `v` | Versão do envelope |
| `tipo` | Tipo do evento, no formato `dominio.acao` |
| `seq` | Número de sequência crescente, atribuído pelo servidor. Só aparece em mensagens do servidor |
| `ts` | Timestamp em ms |
| `dados` | Conteúdo, que depende do tipo |

Os tipos Rust de cada mensagem estão em `contrato::ws` (compila para WebAssembly, então o app usa os mesmos).

Formato dos `dados` implementados:

| Tipo | `dados` |
|---|---|
| `sessao.pronta` | `{ "seq": <seq atual> }` |
| `comando.novo` | `{ "id", "conta", "tipo", "params", "prioridade", "expiraEm" }` (`expiraEm` em ms, ou `null`) |
| `comando.cancelado` | `{ "id", "conta", "motivo": "expirado" }` |
| `bolsa.atualizada`, `alvos.atualizados` | `{ "em": <ms> }` |
| `sessao.ressincronizar` | `{ "motivo": "fora_do_buffer" \| "seq_futuro" }` |
| `retomar` | `{ "desde": <seq> }` |
| `comando.resultado` | `{ "id", "resultado" }` (até 200 caracteres) |
| `ack` | `{ "seq": <seq> }` |

O `seq` é global do processo e cresce a cada mensagem do servidor com `seq`. Ele zera quando o servidor reinicia; por isso um `desde` maior que o `seq` atual também gera `sessao.ressincronizar`.

## Tipos de mensagem

**Servidor → cliente**

| Tipo | Quando |
|---|---|
| `sessao.pronta` | Logo após conectar. Traz o `seq` atual |
| `comando.novo` | Entrou um comando na fila para uma conta do cliente |
| `comando.cancelado` | Um comando pendente expirou ou foi cancelado |
| `bolsa.atualizada` | Os tickers foram recalculados |
| `alvos.atualizados` | A lista de alvos de coleta mudou |

**Cliente → servidor**

| Tipo | Quando |
|---|---|
| `retomar` | Ao reconectar, com `{ "desde": <ultimo seq recebido> }` |
| `comando.resultado` | Resultado da execução de um comando no jogo |
| `ack` | Confirma o recebimento até um `seq` |

## Origem dos eventos

Os comandos nascem no Postgres. Um trigger em `nexus.comando` faz `NOTIFY nexus_comando` com `{id, conta, evento}` (`novo`, `devolvido`, `expirado`: só o que alguém consome; reserva, execução e linhas já encerradas do espelho não avisam). O `nexus-api` mantém uma conexão dedicada com `LISTEN nexus_comando` (reconecta com espera de 1 a 30 s) e distribui as notificações às conexões por um `tokio::sync::broadcast`. Quando o listener reconecta, todas as conexões conferem a fila de novo, para não perder o que foi notificado no intervalo.

Isso funciona mesmo quando quem escreve no banco é o Node (o espelho copia `public.comando_fila` para `nexus.comando`).

Só `novo` e `devolvido` disparam entrega; `expirado` vira `comando.cancelado`. Os eventos informativos (`bolsa.atualizada`, quando o mercado é remontado) são publicados pelo próprio processo. `alvos.atualizados` já existe no contrato, mas ainda não tem emissor.

### Entrega de comandos

Entregar é reservar: as mesmas funções do `GET /api/comandos`, com lease de 120 s. Ao conectar, o servidor manda `sessao.pronta` e depois os pendentes das contas do token. Se o relay não confirmar a tempo, o comando volta à fila e chega de novo (pelo menos uma vez; confirmar é idempotente). Duas conexões do mesmo token dividem os comandos: cada comando vai para quem o reservar primeiro.

## Reconexão

Responsabilidade do cliente:

1. Se a conexão cair, reconecta com backoff exponencial: 1 s, 2 s, 4 s... até 30 s, com jitter aleatório de ±20%.
2. Ao reconectar, envia `retomar` com o último `seq` que recebeu.
3. O servidor reenvia o que ficou pendente. Os comandos vêm da própria fila (`nexus.comando`): o que já foi confirmado não volta, e o que está reservado só volta quando o lease vence, ou na hora, se a reconexão for do mesmo token: ao abrir a conexão o servidor devolve à fila os comandos reservados por esse token e não confirmados (conta como tentativa) e os reenvia com a entrega repetida. Eventos informativos, como `bolsa.atualizada`, saem de um buffer circular em memória com os últimos 1000, a partir de `desde`.
4. Se o `seq` pedido já saiu do buffer, o servidor responde `sessao.ressincronizar`, e o cliente recarrega o estado completo pelas rotas HTTP.

## Keepalive

O servidor manda um `Ping` a cada 20 segundos. Se não houver `Pong` em 10 segundos, a conexão é fechada. O Nginx está configurado com timeout de leitura de 75 segundos para o `/ws`, então conexões ociosas não são cortadas no meio do caminho.

## Backpressure

Cada conexão tem uma fila de saída limitada (`mpsc` com capacidade 256). Se o cliente não consumir rápido o bastante e a fila encher, o servidor **fecha a conexão** com o código `1013` (try again later), em vez de acumular memória sem limite. O cliente reconecta e recupera o que faltou com `retomar`.

Assim um cliente lento ou travado nunca afeta o servidor nem os outros clientes. O servidor nunca reserva mais comandos do que cabe na fila livre da conexão; se ela fica cheia, espera 2 s que esvazie antes de fechar com `1013`. O que foi reservado e não pôde ser enviado volta à fila sem contar tentativa. Operações de banco dentro da conexão têm limite de 5 s, para um banco lento não derrubar uma conexão saudável por falta de `Pong`.

Se o canal interno de avisos transborda (cliente lento para o `broadcast`), o servidor envia `sessao.ressincronizar` com motivo `lag` e confere a fila de novo.

A confirmação (`comando.resultado`) só vale para comando já entregue; token de conta só confirma comando da própria conta (negado, o servidor registra e ignora).

## Fechamentos

| Código | Quando |
|---|---|
| `1001` | O servidor está desligando |
| `1011` | Sem `Pong` em 10 s, ou erro interno |
| `1013` | Fila de saída cheia (cliente lento) |

Se o cliente fecha, o servidor só encerra a conexão. Mensagens que o servidor não entende (JSON inválido, tipo desconhecido) são ignoradas e contadas.

## Métricas

`nexus_ws_conexoes_abertas`, `nexus_ws_mensagens_total{sentido,tipo}`, `nexus_ws_fechamentos_total{motivo}` (`cliente`, `fila_cheia`, `sem_pong`, `shutdown`, `erro`) e `nexus_ws_confirmacoes_negadas_total`. As da fila (entregues, duplicados, confirmados) estão em `docs/operacao.md`.

## Testes

- Conexão sem token é rejeitada.
- Comando inserido no banco chega ao cliente em menos de 100 ms.
- Derrubar a conexão e reconectar com `retomar` não perde nenhum comando.
- Um cliente que para de ler é desconectado com `1013` sem afetar os outros.
