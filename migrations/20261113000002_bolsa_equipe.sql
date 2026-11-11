-- Fase 2: equipe da bolsa.
-- Tabela de bosses da wiki do jogo (nível fixo, drops e entrada), na ordem da wiki.
CREATE TABLE nexus.boss_wiki (
    ordem integer PRIMARY KEY,
    nome  text    NOT NULL,
    dados jsonb   NOT NULL
);

-- Faltas de coleta: (boss, perfil) cuja equipe não fechou só com a lista de espécies do
-- perfil. Substitui o .bolsa-faltas.json do legado.
CREATE TABLE nexus.falta_coleta (
    chave text  PRIMARY KEY,
    dados jsonb NOT NULL
);

-- Uma linha por plano de equipe calculado: base da calibração com lutas reais.
-- Substitui o equipe-sugestoes.jsonl do legado.
CREATE TABLE nexus.equipe_sugestao (
    id    bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    em    bigint NOT NULL,
    boss  text   NOT NULL,
    dados jsonb  NOT NULL
);
CREATE INDEX equipe_sugestao_boss_em ON nexus.equipe_sugestao (boss, em DESC);
