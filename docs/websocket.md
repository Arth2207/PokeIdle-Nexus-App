# WebSocket

Hoje o relay do PC pergunta ao servidor, de tempos em tempos, se existe comando novo. Isso gera atraso (o comando espera o próximo ciclo) e requisições inúteis quando não há nada. O Nexus troca esse polling por uma conexão WebSocket persistente, e o servidor empurra os eventos assim que eles acontecem.

## Conexão

```
GET /ws
Authorization: Bearer <token>
Sec-WebSocket-Protocol: nexus.v1
```

- Sem token válido, o servidor responde `401` antes do upgrade.
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

Os eventos nascem no Postgres. Triggers aditivos fazem `NOTIFY` nos canais `comando_fila` e `bolsa`. O `nexus-api` mantém uma conexão dedicada com `LISTEN` nesses canais e distribui as notificações para os clientes conectados por um `tokio::sync::broadcast`.

Isso funciona mesmo quando quem escreve no banco é o Node, o que é importante durante a migração.

## Reconexão

Responsabilidade do cliente:

1. Se a conexão cair, reconecta com backoff exponencial: 1 s, 2 s, 4 s... até 30 s, com jitter aleatório de ±20%.
2. Ao reconectar, envia `retomar` com o último `seq` que recebeu.
3. O servidor reenvia o que ficou pendente desde esse `seq`. Os comandos vêm da própria `comando_fila`; eventos informativos, como `bolsa.atualizada`, saem de um buffer circular em memória com os últimos 1000.
4. Se o `seq` pedido já saiu do buffer, o servidor responde `sessao.ressincronizar`, e o cliente recarrega o estado completo pelas rotas HTTP.

## Keepalive

O servidor manda um `Ping` a cada 20 segundos. Se não houver `Pong` em 10 segundos, a conexão é fechada. O Nginx está configurado com timeout de leitura de 75 segundos para o `/ws`, então conexões ociosas não são cortadas no meio do caminho.

## Backpressure

Cada conexão tem uma fila de saída limitada (`mpsc` com capacidade 256). Se o cliente não consumir rápido o bastante e a fila encher, o servidor **fecha a conexão** com o código `1013` (try again later), em vez de acumular memória sem limite. O cliente reconecta e recupera o que faltou com `retomar`.

Assim um cliente lento ou travado nunca afeta o servidor nem os outros clientes.

## Testes

- Conexão sem token é rejeitada.
- Comando inserido no banco chega ao cliente em menos de 100 ms.
- Derrubar a conexão e reconectar com `retomar` não perde nenhum comando.
- Um cliente que para de ler é desconectado com `1013` sem afetar os outros.
