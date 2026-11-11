# 0002. Monólito modular com dois binários

- Status: aceito
- Data: 2026-11-11

## Contexto

O legado tem 6 processos pm2, um serviço systemd e vários scripts. A primeira ideia para a reescrita foi um serviço por responsabilidade (API, ETL, scoring), na linha de microserviços.

Os números reais: um cliente, cerca de 0,1 req/s, 2 vCPU divididos com outras cargas e uma pessoa mantendo tudo.

Microserviços resolvem problemas que este projeto não tem: vários times precisando fazer deploy de forma independente, e partes que precisam escalar de forma diferente em várias máquinas. Em troca, cobram latência de rede entre as partes, falhas parciais, rastreamento distribuído e mais processos disputando CPU.

## Decisão

Um workspace Cargo com crates bem separados (`nucleo`, `persistencia`, `nexus-api`, `nexus-worker`) que geram **dois binários**:

- `nexus-api`: tráfego HTTP e WebSocket, incluindo os cálculos chamados por rota;
- `nexus-worker`: jobs agendados (ETL, scoring, IA, dossiê).

A separação em dois processos existe por isolamento de recurso: um pico de ETL não pode atrasar uma resposta da API.

## Consequências

- Comunicação entre os módulos por chamada de função, sem serialização nem rede.
- Só dois artefatos para fazer deploy, monitorar e depurar.
- As fronteiras entre módulos são garantidas pelo compilador (crates e visibilidade `pub`), não por convenção.
- Se um módulo um dia precisar virar serviço, por exemplo o motor de combate, basta extrair o crate e colocar uma API na frente. A decisão é reversível.
- Risco: o motor de combate (rayon) disputar CPU com o tokio dentro da API. Isso é tratado com pool de threads limitado, `spawn_blocking`/`oneshot` e fila com limite (veja [arquitetura](../arquitetura.md#concorrência)).
