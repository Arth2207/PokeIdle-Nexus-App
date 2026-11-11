# 0007. WebSocket no lugar do polling da fila de comandos

- Status: aceito
- Data: 2026-11-11

## Contexto

O relay do PC pergunta periodicamente ao servidor se há comandos novos. Isso tem dois custos: o comando espera até o próximo ciclo para ser executado, e a maioria das consultas volta vazia.

A fila tem vários produtores (API, rotinas de IA, montagem de builds) e durante a migração alguns deles continuam em Node.

## Decisão

Uma conexão WebSocket persistente entre o relay e o `nexus-api`, alimentada por `LISTEN/NOTIFY` do Postgres:

- triggers aditivos fazem `NOTIFY` quando entra um comando, seja quem for que gravou;
- o `nexus-api` escuta e repassa para os clientes conectados;
- cada mensagem tem número de sequência, e o cliente pode retomar do último que recebeu;
- a fila de saída de cada cliente tem limite, e um cliente lento é desconectado em vez de acumular memória.

O protocolo completo está em [websocket](../websocket.md).

## Consequências

- O comando chega ao PC em milissegundos, não no próximo ciclo.
- Funciona enquanto produtores Node e Rust convivem, porque a notificação nasce no banco.
- Nenhum comando se perde numa queda de conexão: a fonte da verdade continua sendo a tabela, e o WebSocket é só o aviso.
- Custo: o relay do PC precisa ser atualizado para falar o protocolo novo. A rota de polling continua disponível até a fase 7, como plano B.
