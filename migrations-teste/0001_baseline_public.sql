-- Baseline SÓ PARA TESTES: as tabelas do schema `public` que a bolsa usa, como estão no
-- banco de produção (pg_dump --schema-only). Em produção elas são do sistema legado e já
-- existem; este arquivo nunca é embutido no binário (só `migrations/` é).
--
-- Também cria o schema `nexus`, que em produção é criado na instalação (dono: nexus_app).

CREATE SCHEMA IF NOT EXISTS nexus;

CREATE TABLE public.dim_item (
    item_id integer NOT NULL,
    nome text NOT NULL,
    categoria text,
    custo_refino_formula text,
    atualizado_em timestamp with time zone DEFAULT now(),
    preco_npc_compra bigint,
    preco_npc_venda bigint,
    raridade text,
    id_jogo integer,
    CONSTRAINT dim_item_raridade_check CHECK ((raridade = ANY (ARRAY['comum'::text, 'incomum'::text, 'raro'::text, 'epico'::text, 'lendario'::text])))
);

CREATE TABLE public.fato_oferta_mercado (
    id bigint NOT NULL,
    ts_primeiro bigint NOT NULL,
    ts_visto bigint NOT NULL,
    ativo boolean DEFAULT true,
    tipo text,
    moeda text,
    preco bigint,
    species_id integer,
    item_id integer,
    quantidade integer DEFAULT 1,
    nome text,
    ficha jsonb,
    expira_em bigint
);

CREATE TABLE public.fato_resumo_item (
    ts bigint NOT NULL,
    item_id integer NOT NULL,
    moeda text NOT NULL,
    anuncios integer,
    preco_min bigint
);

CREATE TABLE public.fato_venda_mercado (
    id bigint NOT NULL,
    em bigint NOT NULL,
    lido_em bigint,
    tipo text,
    moeda text,
    preco bigint,
    descricao text,
    species_id integer,
    item_id integer,
    quantidade integer,
    ficha jsonb
);

ALTER TABLE ONLY public.dim_item ADD CONSTRAINT dim_item_pkey PRIMARY KEY (item_id);
ALTER TABLE ONLY public.fato_oferta_mercado ADD CONSTRAINT fato_oferta_mercado_pkey PRIMARY KEY (id);
ALTER TABLE ONLY public.fato_resumo_item ADD CONSTRAINT fato_resumo_item_pkey PRIMARY KEY (ts, item_id, moeda);
ALTER TABLE ONLY public.fato_venda_mercado ADD CONSTRAINT fato_venda_mercado_pkey PRIMARY KEY (id);
CREATE INDEX fato_venda_mercado_em ON public.fato_venda_mercado USING btree (em);
CREATE INDEX ix_oferta_ativo ON public.fato_oferta_mercado USING btree (ativo, ts_visto DESC);
