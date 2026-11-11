-- Fase 2: equipes por boss.
-- Pokédex do motor de combate (formato compacto do legado), na ordem do arquivo: a ordem
-- decide empates na escolha da equipe teórica.
CREATE TABLE nexus.pokedex_motor (
    id    bigint  PRIMARY KEY,
    ordem integer NOT NULL,
    dados jsonb   NOT NULL
);

-- Lista viva de bosses que o relay envia (linha única).
CREATE TABLE nexus.equipes_estado (
    id        smallint PRIMARY KEY DEFAULT 1 CHECK (id = 1),
    bosses    jsonb    NOT NULL DEFAULT '[]',
    bosses_em bigint   NOT NULL DEFAULT 0
);

-- Por conta: nível de treinador, bosses vencidos e coleção (fichas como o relay envia).
-- `ordem` guarda a ordem de chegada das contas, que decide a conta padrão.
CREATE TABLE nexus.equipes_conta (
    nick  text    PRIMARY KEY,
    ordem integer NOT NULL,
    dados jsonb   NOT NULL
);
