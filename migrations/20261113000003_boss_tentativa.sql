-- Fase 2: tentativas de boss, base da calibração do modelo de combate do jogo.
-- A captura chega na fase 3 (eventos `batalha` do relay); a tabela e o cálculo nascem aqui.
CREATE TABLE nexus.boss_tentativa (
    id            bigint  GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    em            bigint  NOT NULL,
    conta         text,
    boss          text    NOT NULL,
    nivel_boss    float8  NOT NULL,
    tipos_boss    text[]  NOT NULL DEFAULT '{}',
    venceu        boolean NOT NULL,
    duracao_s     float8  NOT NULL,
    -- Fichas completas das cartas da equipe.
    equipe        jsonb   NOT NULL,
    dano_causado  float8,
    dano_recebido float8,
    -- Eventos `batalha` da luta (dano por golpe, HP do alvo), quando capturados.
    eventos       jsonb
);
CREATE INDEX boss_tentativa_boss_em ON nexus.boss_tentativa (boss, em DESC);
