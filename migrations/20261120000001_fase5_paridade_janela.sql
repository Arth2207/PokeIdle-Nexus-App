-- Fase 5: comparador com janela fechada (substitui nexus.etl_paridade, que fica sem uso).
-- Uma linha por (tabela, conta, janela, fim); recalcular a mesma janela sobrescreve (idempotente).
-- `node`/`nexus` são vetores de totais na mesma ordem (ver `METRICAS` em etl_paridade.rs):
--   fato_kill:            linhas, xp_treinador, xp_pokemon, ouro, ouro_loot_bonus,
--                         loot_boost, shiny, linhas com NULL
--   fato_hunt_eficiencia: linhas, xp_tr_total, xp_pk_total, ouro_total, kills_total,
--                         duracao_s, 0, 0
CREATE TABLE IF NOT EXISTS nexus.etl_paridade_janela (
    tabela   TEXT        NOT NULL,  -- 'fato_kill' | 'fato_hunt_eficiencia'
    conta_id TEXT        NOT NULL,  -- '*' quando a tabela não tem conta
    janela   TEXT        NOT NULL,  -- '1h' | '6h' | '24h'
    ini      TIMESTAMPTZ NOT NULL,
    fim      TIMESTAMPTZ NOT NULL,
    em       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    node     BIGINT[]    NOT NULL,
    nexus    BIGINT[]    NOT NULL,
    igual    BOOLEAN     NOT NULL GENERATED ALWAYS AS (node = nexus) STORED,
    PRIMARY KEY (tabela, conta_id, janela, fim)
);

CREATE INDEX IF NOT EXISTS ix_etl_paridade_janela_em
    ON nexus.etl_paridade_janela (em DESC);
