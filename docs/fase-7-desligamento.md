# Fase 7: desligar o legado e tirar o SQLite

Objetivo: nenhum processo Node do legado servindo produção, o `mapa.db` arquivado e o ETL lendo só do Postgres. Cada item sai **um de cada vez**, com autorização do dono e um caminho de volta.

## Inventário do legado vivo (2026-11-13)

### Processos e portas

| Item | Porta / gatilho | O que faz hoje | Quem consome | Substituto no Nexus | Estado |
|---|---|---|---|---|---|
| pm2 `mapper` (`server.js`) | 8181 (todas as interfaces) | `/ingest` → `mapa.db`; bolsa (`/ofertas`, indicadores, veredito) → `fato_oferta_mercado`; equipes, raros, shiny, fila de comandos | relay do PC (escrita); painel via motor (leitura) | Fases 1, 2 e 3 | **produção** |
| pm2 `mapper`, servidor estático | 9999 (público) | páginas e JSONs de `data/static` (inclui `precos-nexus.json`) | painel / navegador | arquivos estáticos no Nginx do Nexus | **produção**; consumidores a medir |
| pm2 `holodex-nexus` (`servidor.js`) | 47821 (loopback e Tailscale), 47822 (loopback) | motor e API para cliente externo; navegadores desligados (`HDX_SEM_BROWSER=1`). Coletor de mercado parado desde 2026-11-10 | comparador da Fase 4 (`nexus-paridade` lê o motor em loopback) | Fase 4 (motor) | **necessário até a virada da Fase 4** |
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
5. Script `/opt/nexus-deploy/cutover-fase1-2.sh` (cópia em `deploy/`):
   - `grants`: `INSERT, UPDATE` do `nexus_app` em `fato_oferta_mercado` e `fato_resumo_item`.
   - `token`: token `borda-fase12` (leitura + escrita) em `borda/token.conf` (600).
   - `teste`: sobe a borda sem desviar tráfego e testa os dois lados.
   - `on`: flags, recria a API ativa e o Nginx, testa, instala o timer `nexus-borda-guarda.timer` (a cada 15 s: regra só com a marca **e** a borda respondendo `/borda-saude`; se a borda cair, o 8181 volta direto ao `mapper`) e derruba as conexões keep-alive da Tailscale no 8181.
   - `off` (best-effort): tira o timer e a regra, desliga as flags e para a borda (< 1 min). Falha no meio do `on` chama o `off` sozinha.
   - A borda responde CORS como o legado (`OPTIONS` 204, `Access-Control-Allow-Origin: *`).
   - **Rollback não devolve ao legado** o que o Nexus gravou em equipes, faltas e meus-itens: o relay reenvia equipes e faltas; edições de meus-itens feitas durante a virada se perdem. Ofertas, vendas e resumo ficam no Postgres, comum aos dois.
   - Rate limit: um token só para relay e painel (10 req/s, rajada 40, no Nexus); a carga medida é ~0,1 req/s.
6. Com a marca `estado/fase12-cutover`, o `sincronizar-catalogos.sh` para de importar `equipes`, `faltas` e `meus-itens` dos arquivos do legado.
7. O comparador continua lendo o `mapper` em loopback; depois da virada, as diferenças de equipes (estado parado no legado) são esperadas.

**Pré-requisito:** 3 dias de comparação limpa. Investigação de 2026-11-13 (3 dias de `nexus.paridade`):

| Diferença | Vezes | Causa | Ação |
|---|---|---|---|
| `faltaSugestao.custoAproximado` | 34 | **lógica**: o Nexus nunca renovava o cálculo de compra (custo congelado por horas); o legado renova a cada 5 min | corrigido |
| `perfis.max.custoOportunidade` | 7 | cache do valor justo no legado (10 a 30 min) | tolerância de 15% só nos custos, com comentário |
| `candidatas` diferentes | 4 | o legado usa cálculo de até 5 min atrás; o relay muda as ofertas | acima de 10% continua aparecendo; conjuntos diferentes não comparam qualidade |
| perfil `max`/`eco`: custo fora de ±5%, poder < 99% | 47 | provavelmente cache de cotações do legado; **não provado** (o par 2238 × 2018 ficou estável por horas) | conferir depois do deploy: se persistir com o mesmo número de candidatas, é lógica da seleção (`selecionar`) e precisa de fixture do JS |

As diferenças da bolsa (`mercado`, `ticker`, `balao`...) são de dado ao vivo (contagem de vendas, `alvoC` ±0,5%).

## Ordem proposta

1. ~~`scoring`~~: parado em 2026-11-13 (`pm2 stop scoring` + `pm2 save`). Volta atrás: `pm2 start scoring && pm2 save`.
2. `holodex-nexus` da VPS: só sai com a Fase 4 (é a referência do comparador do motor).
3. Fases 1 e 2: `cutover-fase1-2.sh` (abaixo), depois de corrigir a paridade de `equipe`.
4. ~~Catálogo de espécies sem Node~~: `nexus-api importar especies-js` (saída idêntica à do Node).
5. Com a A2: Fases 3 e 4, depois SQLite → PG, e por último `mapper`, 9999, crons e arquivamento do `mapa.db`.

## Concluída (2026-11-14)

Nenhum processo Node do legado serve produção. O app A2 é o relay; o motor roda no Nexus (`nexus-paridade`, só o laço do motor); o ETL lê só do Postgres; o site Leptos (`/painel/` na borda) substitui o painel antigo.

| Item | Como saiu | Volta atrás |
|---|---|---|
| pm2 `mapper` (8181/9999) | `pm2 stop mapper` + `pm2 save`; nenhum hostname do túnel aponta para 8181/9999 | `pm2 start mapper && pm2 save` (dump antigo em `/root/.pm2/dump.pm2.bak-20261114-mapper`) |
| pm2 `holodex-nexus`, `ai-observer`, `etl-eficiencia` | parados com a Fase 4 | `pm2 start /opt/mapper/ecosystem.config.js --only <nome>` |
| Comparador com o legado | `NEXUS_PARIDADE_URL` fora do compose; sem ela o `paridade-continua` só roda o motor | recolocar `NEXUS_PARIDADE_URL` (backup `compose.yml.bak-20261114-legado`) |
| Crons `pm2-status-dump.sh` e `lixeira.sh` | comentados no crontab do root | `crontab /root/crontab.bak-20261114-legado` |
| Cron `precos-nexus` | renomeado para `/etc/cron.d/precos-nexus.desligado-20261114` | tirar o sufixo |
| `mapa.db` e JSONs de estado | cópia em `/opt/mapper/arquivo-20261114/` (`.backup` do SQLite, `integrity_check` ok); originais intactos | — |
| Tabelas `nexus.*_shadow` | sem escrita (todas as flags de escrita em produção); mantidas, sem drop | — |
| Token `telas-teste` | revogado (`revogado_em`) | — |

Pendente: medir RAM/CPU do app com 4 contas no PC (meta ≤ 1,4 GB e ≤ 25%); remover o Node do host quando o dono decidir (o `mapper` segue instalado para a volta atrás).
