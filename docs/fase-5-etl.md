# Fase 5: ETL e dossiê

Plano da fase 5. Levantamento feito em novembro de 2026.

## Levantamento (etapa 1)

### ETL contínuo (`etl-continuo.js`, pm2 `online`)

**Fonte:** SQLite `mapa.db`, tabela `evento_kill` (~523 k linhas, inclui sentinela `ts=1234567890000`).  
**Destino:** `public.fato_kill` (~482 k linhas; diferença = sentinelas descartadas + linhas ainda por migrar).  
**Frequência:** ciclo a cada 5 min, batch de 5 000 por conta.  
**Contas hardcoded:** `mvxorqh6` → sessao_etl 100, `mvxoui28` → 101, `mvxoxith` → 102, `mw002fcf` → 103.  
**Cursor:** arquivo `data/etl-cursores.json` (não transacional — não avança se INSERT falha).

**Mapeamento SQLite → Postgres:**

| evento_kill | fato_kill | Observação |
|---|---|---|
| `ts` | `ts_at`, `ts_ms` | `to_timestamp(ts/1000.0)`, descarta `ts=1234567890000` |
| (fixo por conta) | `conta_id` | |
| (sessao_etl) | `sessao_id` | 100–103, cria em `fato_sessao_contexto` se não existe |
| `hunt_slug` | `hunt_slug` | insere em `dim_hunt` se novo |
| `xp_treinador` | `xp_treinador` | |
| `xp_pokemon` | `xp_pokemon` | |
| `ouro` | `ouro` | |
| `ouro_loot_bonus` | `ouro_loot_bonus` | |
| `loot_boost` | `loot_boost` | int → bool |
| `shiny` | `shiny` | int → bool |
| (fixo) | `source` | `'vps'` |
| `nome`, `nivel`, `slot`, `pokemon_ativo_*`, `drops` | — | **não usados** pelo legado |

**Idempotência:** `UNIQUE (ts_at, ts_ms, conta_id)` + `ON CONFLICT DO NOTHING` — rodar o mesmo intervalo duas vezes produz o mesmo resultado.

**Campos não preenchidos pelo legado:** `especie_alvo`, `nivel_alvo`, `especie_ativa`, `nivel_ativo` (FK para `dim_especie`). O ETL não os tem porque o SQLite não os guarda.

---

### ETL de eficiência (`etl-eficiencia.js`, pm2 `stopped`)

**Fonte:** `fato_kill` (últimas 24 h, horas completas, `hunt_slug IS NOT NULL`, `>= 10 kills` por grupo).  
**Destino:** `public.fato_hunt_eficiencia`, upsert por `(especie_ativa, faixa_nivel, tipo_ativo_pri, hunt_slug, ts_hora)`.  
**Frequência:** horária (pm2 cron), hoje parado.  
**Chave atual:** escreve `especie_ativa='unknown'`, `faixa_nivel=0`, `tipo_ativo_pri='unknown'` — só agrega por `(hunt_slug, hora)`.  
**Limpeza:** deleta linhas com `ts_hora < NOW()-48h` onde `especie_ativa='unknown'`.  
**Idempotência:** `ON CONFLICT DO UPDATE` (upsert total) — idempotente.

> Parece parado no pm2, mas roda por `cron_restart` a `:05` (ver etapa 5).

---

### Dossiê (`dossie-server.js`, systemd `dossie.service`, porta 8899, `active`)

**Tabelas escritas:**

| Tabela | Rota | Mecanismo |
|---|---|---|
| `public.pokemon_ficha` | `POST /api/dossie/ficha` | `INSERT … ON CONFLICT (pokemon_id) DO UPDATE` |
| `public.fato_venda_mercado` | `POST /api/dossie/vendas` | `ON CONFLICT (id) DO NOTHING` — idempotente |

**Tabelas lidas:** `fato_venda_mercado`, `pokemon_ficha`, `dim_especie`, `fato_kill`, `dim_conta`, `ia_memoria`, `ia_playbook`, `v_ia_performance`, `config_ia`, `ia_estado`, `comando_fila`.

**Rotas de leitura relevantes para o comparador:**

| Rota | O que faz |
|---|---|
| `GET /api/dossie/fichas` | lista fichas por nick/nicks |
| `GET /api/dossie/especie` | média de stats por espécie |
| `GET /api/dossie/dataset` | export para IA |
| `GET /api/dossie/cacada-medidas` | XP por abate medido (lê `fato_kill`) |
| `GET /api/dossie/calc/lite` | catálogo de espécies + status do precificador |
| `POST /api/dossie/calc/preco` | precificação no servidor |
| `GET /precificador/precos.json` | precos.json mesclado (privado, TTL 10 min) |
| `GET /api/ia/memoria` | histórico de decisões |
| `GET /api/ia/performance` | performance da IA |
| `GET /api/ia/estado` | config + pendentes |

**Motor de preços:** carrega JS em `vm.createContext` (módulos do `engine/precificador`). Combina `precos.json` antigo + vendas recentes do banco. TTL 10 min. No Nexus, isso vira código Rust (ou template HTML servido do `nexus-worker`).

**Arquivos estáticos:** `data/static/` — páginas públicas + fotos em `static/fotos/`.

---

### Preços de mercado (`etl-mercado.js`, **não está no pm2**)

**Fonte:** SQLite `mercado_msg` (mensagens `mercado.itens`, `mercado.especies`, `mercado.listar`, `mercado.favPreco`).  
**Destino:** `public.fato_preco_mercado` (hypertable, **hoje vazia** — 0 linhas).  
**Frequência:** o script existe mas não está no pm2 — aparentemente nunca rodou em produção.  
**Schema:** `(ts, item_id, nome, preco_min, preco_mediano, preco_max, volume_listado, variacao_7d_pct, alerta_raro, fonte)`, `UNIQUE (item_id, ts)`.

> `fato_preco_mercado` está vazia. Verificar se `mercado_msg` existe no SQLite antes de decidir se entra na fase 5.

---

### GRANTs necessários

`nexus_app` tem SELECT em `fato_venda_mercado`. Faltam:

```sql
GRANT SELECT ON public.fato_kill           TO nexus_app;
GRANT SELECT ON public.pokemon_ficha       TO nexus_app;
GRANT SELECT ON public.dim_especie         TO nexus_app;
GRANT SELECT ON public.dim_conta           TO nexus_app;
GRANT SELECT ON public.dim_hunt            TO nexus_app;
GRANT SELECT ON public.fato_sessao_contexto TO nexus_app;
GRANT SELECT ON public.fato_hunt_eficiencia TO nexus_app;
```

---

### Volume e frequência

| Tabela | Linhas | Ritmo |
|---|---|---|
| SQLite `evento_kill` | 522 k | ~70–85 k kills/dia |
| `fato_kill` | 482 k | mesma fonte; ciclo a cada 5 min |
| `fato_venda_mercado` | 53 k | a cada envio do relay |
| `fato_hunt_eficiencia` | 216 | horário (parado) |
| `fato_preco_mercado` | 0 | nunca rodou |
| `pokemon_ficha` | (não medido) | manual via dossiê |

---

## Escopo da fase 5

### O que entra

1. **ETL contínuo em sombra** — replica `etl-continuo.js` em Rust no `nexus-worker`, com cursor próprio no Postgres (transacional), gravando em `nexus.fato_kill_shadow`. Comparador conta × conta, intervalo × intervalo.
2. **ETL de eficiência** — depende de resposta: entra se o scoring ainda usa `fato_hunt_eficiencia`.
3. **Preços de mercado** — depende de resposta: `fato_preco_mercado` vazia; verificar `mercado_msg` no SQLite.
4. **Dossiê** — rotas de leitura + escrita portadas no `nexus-worker` (ou `nexus-api`, decidir abaixo), comparador de rotas.

### ETL contínuo vs. dossiê: onde hospedar?

- ETL contínuo → `nexus-worker` (job agendado, sem rota pública).
- Dossiê: o dossiê serve páginas HTML estáticas + API JSON. Duas opções:
  - **A) `nexus-worker`** com servidor HTTP mínimo na porta interna — mantém a separação de CPU; mais simples de operar.
  - **B) `nexus-api`** — zero infra extra, mas mistura tráfego de usuário com ETL.
  - **Recomendação:** dossiê no `nexus-worker` (já tem `/health` e `/metrics`; basta adicionar a porta do dossiê). A decisão final é do dono do projeto.

---

## Etapas

### Etapa 1 — levantamento ✓ (este doc)

### Etapa 2 — crate e container `nexus-worker` ✓

- Crate `crates/nexus-worker`; binário `/nexus-worker` na mesma imagem do `nexus-api`
  (o compose usa `entrypoint: ["/nexus-worker"]`, não `command`, porque o ENTRYPOINT da imagem é a API).
- `/health/ready` e `/metrics` na porta 9191 (só rede interna + `monitoring`).
- Healthcheck por socket HTTP/1.0 (a imagem distroless não tem `wget`).
- Container único, **sem blue/green**: o cursor é transacional e o INSERT idempotente,
  então recriar o worker no deploy não perde nem duplica dados.
- Limite: 0,5 CPU / 256 MB (uso real ~3 MB em repouso).

### Etapa 3 — ETL contínuo em sombra

- Lê SQLite `mapa.db` em modo só leitura (`rusqlite`, `OpenFlags::SQLITE_OPEN_READ_ONLY`).
  - **Conceito novo:** `rusqlite` é a crate para SQLite em Rust. Como o arquivo fica em `spawn_blocking` (I/O síncrono fora do tokio), igual ao `rayon` + `oneshot` já visto.
- Cursor próprio em `nexus.etl_cursor` (linha por conta), atualizado **na mesma transação** do INSERT — resolve o problema não-atômico do legado.
  - **Conceito novo:** cursor transacional — ler + inserir + avançar cursor num único `BEGIN/COMMIT`. Se cair no meio, o cursor não avança e o próximo ciclo reprocesa sem duplicar (a chave única garante).
- Tabela `nexus.fato_kill_shadow` (mesma estrutura da `public.fato_kill`), escritor único o `nexus-worker`.
- `INSERT … ON CONFLICT (ts_at, ts_ms, conta_id) DO NOTHING`.
- Descarta `ts = 1234567890000`.
- **Status ✓** em produção desde 2026-11-12 (primeiro ciclo: 19.999 abates nas 4 contas).
- `mapa.db` aberto com URI `file:…?immutable=1`. O banco está em modo WAL, e o SQLite
  exige escrita no `-shm` mesmo em só leitura; no container só o `.db` está montado (`:ro`).
  - Consequência: lê só o arquivo principal. Abates que ainda estão no WAL (antes do
    checkpoint) entram num ciclo seguinte. Um checkpoint durante a leitura pode dar erro
    transitório ("malformed"); o ciclo falha e repete sem avançar o cursor.
  - Risco a medir no comparador: abate gravado no WAL com `ts` menor que o cursor já
    avançado seria pulado. Se o comparador mostrar falta sistemática, trocar por montar o
    diretório e um `-shm` gravável, ou copiar o `.db` para `/tmp` antes de ler.

- **Bug corrigido (sessão 6):** desde 2026-11-05 15h ~99% das linhas têm `loot_boost`
  NULL. `row.get::<i32>` falhava e `filter_map(|r| r.ok())` descartava a linha em silêncio:
  o ETL perdeu esses abates e, quando o lote inteiro era NULL, parou de avançar.
  Agora: `Option` + erro propagado (o ciclo falha sem avançar o cursor, nunca pula linha),
  e `0`/`''` viram NULL como o `valor || null` do legado. Cursor recuado para 2026-11-05
  00:00 (backup em `nexus.etl_cursor_bak_20261113`) para reprocessar.
- Gauge `nexus_worker_etl_cursor_atraso_s{conta}`: se só cresce, o ETL travou.
- Diferença conhecida: o Nexus descarta `ts = 1234567890000`; o legado grava (linha de
  2009, fora de qualquer janela).

### Etapa 4 — comparador fato × fato ✓ (validação de 7 dias em andamento)

- Job horário `etl_paridade` → `nexus.etl_paridade_janela` (a antiga `nexus.etl_paridade`
  fica sem uso). Upsert por `(tabela, conta, janela, fim)`: recalcular dá o mesmo resultado.
- **Janela fechada:** termina na hora cheia de `agora − atraso` (30 min no `fato_kill`,
  2 h na eficiência). Atraso é tolerância de tempo (cadências diferentes e WAL não lido).
- Vetor de totais `node`/`nexus` (coluna `igual` gerada = vetores iguais):
  - `fato_kill` por conta, janelas 1h/6h/24h: linhas, xp_treinador, xp_pokemon, ouro,
    ouro_loot_bonus, loot_boost, shiny, linhas com NULL.
  - `fato_hunt_eficiencia`, janelas 1h/6h: linhas, xp_tr, xp_pk, ouro, kills, duracao_s.
    Sem 24h: os dois códigos usam `ts_at >= NOW() − 24h`, então a hora mais antiga é
    regravada parcial conforme o minuto em que cada um roda (legado `:05`). Efeito de agenda.
- Métricas: `nexus_etl_paridade_igual{tabela,conta,janela}` e
  `nexus_etl_paridade_diff{…,metrica}` (Node − Nexus).
- Relatório: `nexus-worker etl-relatorio [--desde <horas>]` (mostra as posições que
  divergem e quantas janelas de 1h divergiram no período).
- Validação manual (sessão 6): `fato_kill` bate 100% hora a hora até 05/11 14h (antes do
  bug); eficiência bate exato na janela de 6h.
- **Melhoria proposta (`NEXUS_MELHORIAS`):** eficiência com
  `ts_at >= date_trunc('hour', NOW()) − 24h` (só horas inteiras). Hoje toda linha final do
  legado perde os primeiros ~5 min da hora.

### Etapa 5 — ETL de eficiência ✓ (sombra) · preços de mercado ✗ (fora)

- Eficiência portada em sombra (`nexus.fato_hunt_eficiencia_shadow`, job horário). O pm2
  `etl-eficiencia` aparece `stopped` mas roda por `cron_restart` `5 * * * *`.
- Preços de mercado **fora da fase 5** (decisão 2026-11-13): `mercado_msg` com 0 linhas,
  `fato_preco_mercado` vazia, nenhum consumidor. Reavaliar se o relay passar a gravar.

### Etapa 6 — dossiê

Hospedado no `nexus-worker` (decisão 2026-11-13), módulo `crates/nexus-worker/src/dossie/`.
Porta interna 9192, publicada só em `127.0.0.1:18899` para o comparador.

**6a ✓ (sessão 6)**
- Despacho igual ao `handler` do legado: rota exata `MÉTODO caminho` → estático (GET) → 405;
  CORS `*` em GET, OPTIONS, limite por IP compartilhado entre rotas, chave em `x-chave`/`?chave=`.
- Estáticos: lidos do diretório do legado montado só leitura no mesmo caminho, com a mesma
  lista pública. Não usei `rust-embed`: o conteúdo muda fora do repositório (páginas e
  fotos), tem ~23 MB, e embutir faria o Nexus servir uma cópia velha. Reavaliar no cutover,
  se as páginas passarem a viver no repositório.
- Leituras (`fichas`, `especie`, `dataset`, `cacada-medidas`, `fotos`): mesmas consultas no
  `public`. O JSON de cada linha é montado no Postgres com `json_build_object` e casts que
  imitam o node-pg: `bigint`/`numeric` → string, `date` → ISO à meia-noite UTC,
  `timestamptz` → ISO com ms, `real` → texto do Postgres. Ordem de chaves preservada.
- Escritas em sombra: `vendas` → `nexus.fato_venda_mercado_shadow`, `ficha` →
  `nexus.pokemon_ficha_shadow` (mesmo SQL, parâmetros `$n::text::tipo` = coerção do
  node-pg), uploads em tmpfs `/dados` (somem no restart; teto de 24 MB e um por vez, porque
  o worker tem 256 MB com o ETL no mesmo processo; o legado aceita 200 MB no `/arquivo`).
- Coerções do JS (`Number()`, `String()`, `||`, `??`, `slice` UTF-16, `parseInt` do SQLSTATE)
  em `dossie/js.rs`. Diferenças só com entrada patológica estão no topo de `escrita.rs`.
- Validação local: 32 rotas (estáticos, APIs, erros 401/404/405/500, OPTIONS) iguais byte a
  byte ao legado.
- Comparador `nexus-dossie-paridade` (rede do host, a cada 30 min) → `nexus.dossie_paridade`;
  relatório `nexus-worker dossie-relatorio [--desde <horas>]`. Só GETs e POST sem chave
  (401 antes de gravar); teste trava a lista. `POST /api/dossie/ficha` grava sem chave e
  nunca entra.
- Incidente 2026-11-13 06:26–10:33: o `unattended-upgrade` reiniciou o containerd e a porta
  publicada (18899) parou de encaminhar até o reboot automático; o worker e o ETL seguiram
  normais. O comparador registra isso como divergência ("nexus: … timed out"); ao contar os
  7 dias, separar falha de transporte de diferença de conteúdo.

**6b ✓ — precificador** (`dossie/precos.rs`)
- `GET /precificador/precos.json` (`montarPrecos()`): lê o `precos.json` base do legado
  (só leitura) + `fato_venda_mercado` dos últimos 30 dias, cotação diária dos itens,
  `prepare` com os motivos de descarte, mescla e `JSON.stringify` compacto. Cache de 10 min
  separado do cache do cálculo, como no legado (`precosCache` × `precosObj`).
- `calc/lite` e `calc/preco` (`priceSheet` detalhado: faixas por nível, vendas da família,
  comparáveis, confiança, trava de nota mínima).
- Núcleo: `vizinhos_detalhe` (influência, partes, incertas), `Ajuste::nivel` (`fit.level`),
  `lendario` público. Nada muda no resumo que a API usa.
- **Correção de paridade no núcleo:** a confiança somava o intercepto antes dos termos;
  o JS soma no fim. Mudava o último bit de `confidence.chance` (também na bolsa da API).
- `serde_json` com `float_roundtrip` (unificado com a API): ler floats com arredondamento
  correto, como o `JSON.parse`. Sem isso o último bit de preços/PvE do arquivo base podia mudar.
- Serializador `dossie/jsonjs.rs`: números como o JS (`123` e não `123.0`, `1e+21`). Vale
  para todas as respostas do dossiê.
- Validação: `precos.json` idêntico (9,3 MB), `calc/lite` idêntico, 82 fichas no
  `calc/preco` idênticas (70 de vendas reais + bordas).

- Memória: cada linha de venda é resumida ao chegar do banco (a ficha inteira como `Value`
  levava o pico a ~800 MB); pico medido ~260 MB. Worker com 512M e `MALLOC_ARENA_MAX=2`.

**6c ✓ — cálculo do dossiê** (`dossie/calc/`, feito em paralelo por um subagente)
- `calc/catalogo` e `calc/dossie`: porte do `sim.js` + `dossie-render.js` (HTML) reusando o
  combate do núcleo. Lê `engine/data/pokedex.json`, `evolucoes.json` e o
  `equipes-estado.json` (relido a cada chamada).
- Validação: catálogo idêntico; 90 casos do `calc/dossie` idênticos (40 viraram teste de
  regressão em `calc/testes/`).

**Comparador** (casos novos): `precos.json` (legado pela interface do Tailscale, que ele
trata como privado; Nexus com a chave própria `secrets/dossie_chave`), `calc/lite`,
`calc/catalogo`, 3× `calc/dossie`, 2× `calc/preco`. Divergência com os caches de preço
montados em instantes diferentes (`geradoEm` diferente) é gravada com `tempo:` e não conta
como divergência de lógica no relatório.

**Pendente**
- `/api/ia/*`: precisa de `GRANT SELECT` em `ia_memoria`, `ia_playbook`, `v_ia_performance`,
  `config_ia`, `ia_estado` (pedir ao dono). Com chave válida responde 501.
- Chave do dossiê no Nexus (`NEXUS_DOSSIE_CHAVE_FILE`) só no cutover; até lá rotas com chave → 401.
- Tailscale: o legado também libera tudo para quem entra pela interface do Tailscale; no
  cutover a porta do Tailscale passa a apontar para o worker.

### Arrumação (sessão 6)

- Worker roda como uid/gid 65532 (confirmado em `/proc/<pid>/status`).
- `group_add: ["986"]` **continua necessário**: `mapa.db` é `660` do grupo `dossie`;
  `immutable=1` não muda permissão de arquivo.
- Pico de memória da API no boot: medido 823 MB (VmHWM) contra ~333 MB estável. Causa: duas
  montagens do mercado em paralelo (o lock do carregador era solto antes do `montar`).
  Corrigido com lock de montagem (`fdaa10e`): pico medido no boot seguinte 563 MB
  (estável ~303 MB). Limite de 1G pode voltar a 768M (folga de ~200 MB).

### Etapa 7 — cutover ✓ (executado em 2026-11-13, por decisão do dono)

O dono decidiu virar no mesmo dia, sem esperar 7 dias de comparador (estava verde em todas
as janelas desde o fim do reprocessamento). Script: `/opt/nexus-deploy/cutover-fase5.sh`
(`grants`, `etl-on`/`etl-off`, `dossie-on`/`dossie-off`, `status`).

1. **GRANTs** de escrita para o `nexus_app`: INSERT em `fato_kill`, `fato_sessao_contexto`,
   `dim_hunt`, `fato_venda_mercado`; INSERT/UPDATE/DELETE em `fato_hunt_eficiencia` e
   `pokemon_ficha`; USAGE na sequência `pokemon_ficha_id_seq`.
2. **ETL** (`etl-on`): pm2 `etl-continuo` e `etl-eficiencia` parados (`pm2 save`); cursor do
   Nexus recuado para o do legado (`etl-cursores.json`, backup em
   `nexus.etl_cursor_bak_cutover`); `NEXUS_ESCRITA_ETL=true`. O comparador fato × fato
   desliga sozinho nesse modo.
   - Junto: o ETL passou a abrir o SQLite com `mode=ro` (lê o WAL). Com o diretório inteiro
     montado só leitura, o SQLite abre o WAL read-only; o `immutable=1` atrasava o dado até o
     checkpoint. Atraso agora ~1 min (era 5 min no legado).
   - Conferido: janela da virada com contagem igual ao SQLite nas 4 contas e nenhuma duplicata.
3. **Dossiê** (`dossie-on`): `dossie.service` desabilitado; serviço `nexus-dossie`
   (`dossie-servir`, usuário `dossie`, perfil `cutover`) nas portas do legado: loopback
   (cloudflared) e a interface do Tailscale, que vai para a porta privada (libera as rotas
   protegidas sem chave, como o legado). `NEXUS_ESCRITA_DOSSIE=true`: escritas no `public`,
   fotos e uploads na pasta do legado (único volume gravável), teto de 200 MB, 1G de memória.
   Chave do legado copiada para `secrets/dossie_chave_legado`. O `nexus-dossie-paridade` fica
   parado. Conferido: páginas, APIs, `precos.json` (404 público, 200 privado e com chave) e o
   relay gravando vendas pelo Nexus.
4. **Deploy:** o `deploy.sh` recria o `nexus-dossie` (em vez do comparador) quando existe
   `estado/dossie-cutover`.

**Rollback:** `cutover-fase5.sh etl-off` e/ou `dossie-off` (religam o legado; o ETL do legado
retoma do `etl-cursores.json` e o `ON CONFLICT` evita duplicata).

**Ainda depende do legado:** o `mapa.db` é escrito pelo `mapper` (`server.js`, ingestão do
relay). O ETL lê dele até a ingestão virar para o Nexus (Fase 3) e o SQLite sair (Fase 7).

## Critério de pronto

- Rodar o mesmo período duas vezes produz o mesmo resultado.
- Tabelas fato do Nexus batem com as do Node no mesmo intervalo (comparador verde por 7 dias).
- Dossiê com o mesmo conteúdo no comparador.
- CI verde, deploy blue/green ok, `docs/` atualizados.
