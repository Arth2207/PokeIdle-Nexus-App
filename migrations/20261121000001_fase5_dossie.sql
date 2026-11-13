-- Fase 5, etapa 6: dossiê em sombra no nexus-worker.
-- Tabelas sombra com a mesma forma das do legado: as rotas de escrita rodam o mesmo SQL
-- aqui, sem tocar no `public`. No cutover, o worker passa a gravar nas tabelas `public`.

CREATE TABLE IF NOT EXISTS nexus.fato_venda_mercado_shadow (
    id          BIGINT PRIMARY KEY,
    em          BIGINT NOT NULL,
    lido_em     BIGINT,
    tipo        TEXT,
    moeda       TEXT,
    preco       BIGINT,
    descricao   TEXT,
    species_id  INT,
    item_id     INT,
    quantidade  INT,
    ficha       JSONB
);
CREATE INDEX IF NOT EXISTS ix_venda_mercado_shadow_em
    ON nexus.fato_venda_mercado_shadow (em);

CREATE TABLE IF NOT EXISTS nexus.pokemon_ficha_shadow (
    id            BIGSERIAL PRIMARY KEY,
    nick          TEXT,
    pokemon_id    TEXT,
    nome          TEXT,
    slug          TEXT,
    dex_id        INT,
    nivel         INT,
    tipo_1        TEXT,
    tipo_2        TEXT,
    potencia      TEXT,
    qualidade     NUMERIC,
    iv_total      INT,
    pve_power     NUMERIC,
    hp_combate    NUMERIC,
    pvp_power     NUMERIC,
    nivel_efetivo INT,
    score         NUMERIC,
    stat_hp       INT,
    stat_atk      INT,
    stat_def      INT,
    stat_spatk    INT,
    stat_spdef    INT,
    stat_spd      INT,
    iv_hp         SMALLINT,
    iv_atk        SMALLINT,
    iv_def        SMALLINT,
    iv_spatk      SMALLINT,
    iv_spdef      SMALLINT,
    iv_spd        SMALLINT,
    capturado_em  DATE,
    capturado_por TEXT,
    raw_text      TEXT,
    ts_salvo      BIGINT DEFAULT ((EXTRACT(epoch FROM now()) * 1000)::bigint),
    oculto        BOOLEAN DEFAULT false
);
CREATE UNIQUE INDEX IF NOT EXISTS ux_pokemon_ficha_shadow_pid
    ON nexus.pokemon_ficha_shadow (pokemon_id) WHERE pokemon_id IS NOT NULL;

-- Comparador de rotas do dossiê (mesma rota no legado e no Nexus).
CREATE TABLE IF NOT EXISTS nexus.dossie_paridade (
    id           BIGSERIAL   PRIMARY KEY,
    em           TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    rota         TEXT        NOT NULL,
    status_node  INT         NOT NULL,
    status_nexus INT         NOT NULL,
    igual        BOOLEAN     NOT NULL,
    detalhe      TEXT
);
CREATE INDEX IF NOT EXISTS ix_dossie_paridade_em ON nexus.dossie_paridade (em DESC);
