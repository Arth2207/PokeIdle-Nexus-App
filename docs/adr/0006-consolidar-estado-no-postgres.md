# 0006. Consolidar SQLite e arquivos JSON no Postgres

- Status: aceito
- Data: 2026-11-11

## Contexto

O estado do legado está espalhado:

- Postgres: mercado, tabelas fato e IA.
- SQLite de cerca de 600 MB: eventos brutos capturados do jogo.
- Arquivos JSON: estado do motor de automação (cerca de 470 KB), estado de equipes e raros, inventário manual, cursores de ETL.

Com isso, backup, consistência e consultas cruzadas dependem de três mecanismos diferentes. A escrita em arquivo JSON também não é atômica: uma queda no meio corrompe o arquivo.

## Decisão

Todo o estado vai para o Postgres, no schema `nexus`:

- os eventos brutos viram hypertables do TimescaleDB, com compressão depois de 7 dias e retenção configurável;
- cada arquivo JSON vira uma ou mais tabelas com schema explícito.

Durante a migração, as tabelas novas são criadas de forma aditiva, e os dados são copiados no cutover da fase dona de cada uma.

## Consequências

- Um backup só, transações de verdade e consultas cruzadas possíveis.
- A compressão do TimescaleDB deve reduzir bastante o volume dos eventos brutos.
- O sqlx valida as queries em tempo de compilação, o que não era possível com os arquivos JSON.
- Custo: desenhar o schema de dados que hoje são livres, principalmente o estado do motor. Isso é feito no começo da fase 4.
