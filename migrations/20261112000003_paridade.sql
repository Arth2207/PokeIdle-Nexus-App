-- Resultado das rodadas do comparador de paridade com o legado (fase de shadow).
-- O comparador roda num container à parte e grava aqui; a API publica a última rodada de
-- cada rota em /metrics.
CREATE TABLE nexus.paridade (
    id         bigint      GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    em         timestamptz NOT NULL DEFAULT now(),
    rota       text        NOT NULL,
    comparados integer     NOT NULL,
    iguais     integer     NOT NULL,
    so_legado  integer     NOT NULL,
    so_nexus   integer     NOT NULL,
    exemplos   jsonb       NOT NULL DEFAULT '[]'
);
CREATE INDEX paridade_rota_em ON nexus.paridade (rota, em DESC);
