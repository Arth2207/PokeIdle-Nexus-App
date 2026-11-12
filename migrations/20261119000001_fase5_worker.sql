-- Fase 5: tabelas do nexus-worker (ETL contínuo, eficiência, comparador)

-- Cursor do ETL por (conta, fonte). Atualizado na mesma transação do INSERT,
-- tornando o processo tolerante a falhas sem duplicatas.
CREATE TABLE IF NOT EXISTS nexus.etl_cursor (
    conta_id      TEXT        NOT NULL,
    fonte         TEXT        NOT NULL,
    ts_ms         BIGINT      NOT NULL DEFAULT 0,
    atualizado_em TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (conta_id, fonte)
);

-- Shadow da fato_kill (mesma estrutura, sem FKs externas — permite testes isolados)
CREATE TABLE IF NOT EXISTS nexus.fato_kill_shadow (
    ts_at           TIMESTAMPTZ NOT NULL,
    ts_ms           BIGINT      NOT NULL,
    conta_id        TEXT        NOT NULL,
    sessao_id       INTEGER     NOT NULL,
    hunt_slug       TEXT,
    xp_treinador    INTEGER,
    xp_pokemon      INTEGER,
    ouro            INTEGER,
    ouro_loot_bonus INTEGER,
    loot_boost      BOOLEAN,
    shiny           BOOLEAN     NOT NULL DEFAULT FALSE,
    source          TEXT,
    UNIQUE (ts_at, ts_ms, conta_id)
);

CREATE INDEX IF NOT EXISTS ix_kill_shadow_ts
    ON nexus.fato_kill_shadow (ts_at DESC);

CREATE INDEX IF NOT EXISTS ix_kill_shadow_conta
    ON nexus.fato_kill_shadow (conta_id, ts_at DESC);

-- Shadow da fato_hunt_eficiencia
CREATE TABLE IF NOT EXISTS nexus.fato_hunt_eficiencia_shadow (
    especie_ativa  TEXT    NOT NULL,
    faixa_nivel    INTEGER NOT NULL,
    tipo_ativo_pri TEXT    NOT NULL,
    hunt_slug      TEXT    NOT NULL,
    ts_hora        BIGINT  NOT NULL,
    kills_total    INTEGER NOT NULL DEFAULT 0,
    xp_tr_total    BIGINT  NOT NULL DEFAULT 0,
    xp_pk_total    BIGINT  NOT NULL DEFAULT 0,
    ouro_total     BIGINT  NOT NULL DEFAULT 0,
    duracao_s      INTEGER,
    PRIMARY KEY (especie_ativa, faixa_nivel, tipo_ativo_pri, hunt_slug, ts_hora)
);

-- Resultados do comparador ETL fato × fato
CREATE TABLE IF NOT EXISTS nexus.etl_paridade (
    id           BIGSERIAL   PRIMARY KEY,
    em           TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    janela       TEXT        NOT NULL,
    conta_id     TEXT        NOT NULL,
    linhas_node  BIGINT      NOT NULL,
    linhas_nexus BIGINT      NOT NULL,
    soma_xp_node BIGINT      NOT NULL,
    soma_xp_nexus BIGINT     NOT NULL,
    igual        BOOLEAN     NOT NULL
                             GENERATED ALWAYS AS (
                                 linhas_node = linhas_nexus
                                 AND soma_xp_node = soma_xp_nexus
                             ) STORED
);

CREATE INDEX IF NOT EXISTS ix_etl_paridade_em
    ON nexus.etl_paridade (em DESC);
