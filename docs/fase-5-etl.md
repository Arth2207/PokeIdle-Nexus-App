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

> **Pergunta:** `etl-eficiencia` está parado. Entra na fase 5 ou fica de fora?

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

### Etapa 2 — crate e container `nexus-worker`

- Novo crate `crates/nexus-worker` no workspace.
- Binário com `/health` e `/metrics`.
- Scheduler de jobs (tokio + `CancellationToken`).
- Compose: container separado, limite de CPU/memória, blue/green no deploy.
- CI: imagem gerada no CI (nunca na VPS, como todo o Nexus).

### Etapa 3 — ETL contínuo em sombra

- Lê SQLite `mapa.db` em modo só leitura (`rusqlite`, `OpenFlags::SQLITE_OPEN_READ_ONLY`).
  - **Conceito novo:** `rusqlite` é a crate para SQLite em Rust. Como o arquivo fica em `spawn_blocking` (I/O síncrono fora do tokio), igual ao `rayon` + `oneshot` já visto.
- Cursor próprio em `nexus.etl_cursor` (linha por conta), atualizado **na mesma transação** do INSERT — resolve o problema não-atômico do legado.
  - **Conceito novo:** cursor transacional — ler + inserir + avançar cursor num único `BEGIN/COMMIT`. Se cair no meio, o cursor não avança e o próximo ciclo reprocesa sem duplicar (a chave única garante).
- Tabela `nexus.fato_kill_shadow` (mesma estrutura da `public.fato_kill`), escritor único o `nexus-worker`.
- `INSERT … ON CONFLICT (ts_at, ts_ms, conta_id) DO NOTHING`.
- Descarta `ts = 1234567890000`.

### Etapa 4 — comparador fato × fato

- Mesmo padrão do comparador das fases 3/4: escolhe um intervalo (ex.: última hora completa), conta linhas e soma `xp_treinador + xp_pokemon + ouro` por conta.
- Métricas `nexus_etl_paridade_{contagem,soma}_{total,igual,diferente}` por conta.
- Relatório: `nexus-worker etl-relatorio --desde <horas>`.

### Etapa 5 — ETL de eficiência e preços de mercado *(condicional)*

- ETL eficiência: se scoring ainda usa `fato_hunt_eficiencia`, porta `etl-eficiencia.js` para job horário no worker.
- Preços de mercado: verificar `mercado_msg` no SQLite; se tiver dados, porta `etl-mercado.js`.

### Etapa 6 — dossiê

- Porta as rotas de leitura + escrita listadas acima.
- Motor de preços: reimplementa a lógica de `montarPrecos()` em Rust, lendo de `nexus.venda_base` + `fato_venda_mercado` (já tem SELECT grant).
- Páginas HTML servidas como arquivos estáticos embutidos no binário (`include_str!` ou `rust-embed`).
  - **Conceito novo:** `rust-embed` inclui arquivos estáticos no binário em tempo de compilação, como o `require()` do Node mas com verificação em compile time.
- Comparador: mesma rota chamada nos dois servidores, resultado comparado.

### Etapa 7 — plano de cutover

- Ligar `NEXUS_ESCRITA_ETL=true` → worker passa a escrever em `public.fato_kill`.
- Parar pm2 `etl-continuo`; `fato_venda_mercado` e `pokemon_ficha` passam para o worker.
- Parar `dossie.service`.
- Verificar 7 dias de comparador verde antes de cada cutover.

---

## Critério de pronto

- Rodar o mesmo período duas vezes produz o mesmo resultado.
- Tabelas fato do Nexus batem com as do Node no mesmo intervalo (comparador verde por 7 dias).
- Dossiê com o mesmo conteúdo no comparador.
- CI verde, deploy blue/green ok, `docs/` atualizados.
