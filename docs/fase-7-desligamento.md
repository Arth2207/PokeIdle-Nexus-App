# Fase 7: desligar o legado e tirar o SQLite

Objetivo: nenhum processo Node do legado servindo produção, o `mapa.db` arquivado e o ETL lendo só do Postgres. Cada item sai **um de cada vez**, com autorização do dono e um caminho de volta.

## Inventário do legado vivo (2026-11-13)

### Processos e portas

| Item | Porta / gatilho | O que faz hoje | Quem consome | Substituto no Nexus | Estado |
|---|---|---|---|---|---|
| pm2 `mapper` (`server.js`) | 8181 (todas as interfaces) | `/ingest` → `mapa.db`; bolsa (`/ofertas`, indicadores, veredito) → `fato_oferta_mercado`; equipes, raros, shiny, fila de comandos | relay do PC (escrita); painel via motor (leitura) | Fases 1, 2 e 3 | **produção** |
| pm2 `mapper`, servidor estático | 9999 (público) | páginas e JSONs de `data/static` (inclui `precos-nexus.json`) | painel / navegador | arquivos estáticos no Nginx do Nexus | **produção**; consumidores a medir |
| pm2 `holodex-nexus` (`servidor.js`) | 47821 (loopback e Tailscale), 47822 (loopback) | motor e API para cliente externo; navegadores desligados (`HDX_SEM_BROWSER=1`). Coletor de mercado parado desde 2026-11-10 | nenhuma conexão observada; o PC usa o motor dele | Fase 4 (motor) | ligado, sem uso observado |
| pm2 `scoring` (`scoring.js`) | 8182 (loopback) | scoring da IA | nenhum (Fase 6 aposentada) | nenhum | ligado, sem clientes |
| pm2 `ai-observer` | — | observador de IA | — | nenhum | parado |
| pm2 `etl-continuo`, `etl-eficiencia` | — | ETL `mapa.db` → `fato_kill`, eficiência | — | `nexus-worker` (Fase 5) | parados (virada) |
| `dossie.service` | 8899 | dossiê | — | `nexus-dossie` (Fase 5) | desabilitado (virada) |

### Crons e scripts que usam Node

| Item | Quando | O que faz | Destino |
|---|---|---|---|
| `pm2-status-dump.sh` (crontab do root) | a cada minuto | `pm2 jlist` → `node -e` → `data/pm2-status.json` (lido pelo `mapper`) | sai junto com o `mapper` |
| `lixeira.sh` (crontab do root) | 04:40 | move `data/raw/*.ndjson` velhos para a lixeira e apaga a lixeira com mais de 7 dias | sai junto com o `mapper` (ninguém mais grava em `raw/`) |
| `sincronizar-catalogos.sh` (Nexus, `/etc/cron.d`) | a cada 10 min | usa `node -e` para converter o catálogo de espécies em JSON | **portar antes de remover o Node** do host |
| `precos-nexus.sh` (`/etc/cron.d`) | a cada 10 min | baixa `precos.json` do `nexus-dossie` para `data/static` (servido no 9999) | muda junto com o 9999 |

### Estado em arquivo do legado

| Arquivo | Quem grava | Substituto no Nexus |
|---|---|---|
| `mapa.db` (SQLite) | `mapper` (`/ingest`) | `nexus.evento` (Fase 3) |
| `equipes-estado.json` | `mapper` (equipes) | `nexus.equipes_estado` (Fase 2) |
| `raros-estado.json` | `mapper` (raros) | `nexus.estado_raros` (Fase 3) |
| `items-catalogo.json`, `etl-cursores.json`, `ai-contas.json` | `mapper` / ETL antigo / IA | catálogos do Nexus / `nexus.etl_cursor` / nenhum |
| `holodex-dados.json` (+ backups diários) | `holodex-nexus` | Fase 4 |

## Dependências que travam as viradas

- **Fases 1 e 2 dependem do relay do PC.** A escrita da bolsa (`/ofertas`) e das equipes chega do relay do PC no `mapper:8181`. Para virar só pela VPS seria preciso dividir as rotas do 8181 (proxy na frente do `mapper`), o que mexe em porta do legado. Sem isso, a virada é trocar o endereço no relay do PC.
- **Fases 3 e 4 dependem da A2.** Em 2026-11-13 a A2 não está conectada: `nexus.evento` vazia, nenhum `/ingest` no Nexus.
- **SQLite → PG depende da Fase 3**: o ETL só pode ler de `nexus.evento` depois que a ingestão estiver no Nexus.

## Fases 1 e 2: virada pela VPS (decidido em 2026-11-13)

Sem mexer no PC nem no `mapper`:

1. Container `nexus-borda` (nginx, `network_mode: host`) escutando numa porta livre só no endereço da Tailscale.
2. Regra `iptables -t nat PREROUTING -i tailscale0 --dport 8181 -j REDIRECT` para essa porta. O `mapper` continua no 8181 e o tráfego local não muda.
3. A borda manda as rotas da bolsa e das equipes (escrita e leitura) para o Nginx do Nexus, com o token do relay injetado por ela, e todo o resto (`/ingest`, comandos, raros, shiny) para o `mapper` em loopback.
4. Na mesma janela: `NEXUS_ESCRITA_BOLSA=true` e `NEXUS_ESCRITA_EQUIPES=true`.
5. Script `cutover-fase1-2.sh status|on|off`: `off` remove a regra e desliga as flags (< 1 min).

**Pré-requisito ainda não cumprido:** 3 dias de comparação limpa. Em 2026-11-13 as diferenças da bolsa são de dado ao vivo (contagem de vendas, `alvoC` ±0,5%), mas `equipe` teve "candidatas 18 ≠ 31" às 13:00, acima da tolerância: investigar antes.

## Ordem proposta

1. ~~`scoring`~~: parado em 2026-11-13 (`pm2 stop scoring` + `pm2 save`). Volta atrás: `pm2 start scoring && pm2 save`.
2. `holodex-nexus` da VPS: confirmar por 24 h que ninguém conecta no 47821/47822 e então parar.
3. Fases 1 e 2: definir o caminho (relay do PC ou proxy na VPS) e escrever `cutover-fase1-2.sh`.
4. Portar a conversão do catálogo de espécies (`sincronizar-catalogos.sh`) para Rust.
5. Com a A2: Fases 3 e 4, depois SQLite → PG, e por último `mapper`, 9999, crons e arquivamento do `mapa.db`.
