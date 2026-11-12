-- Fase 4, etapa 5: decisões do motor do Nexus em sombra. Só aditiva.
-- Com NEXUS_ESCRITA_MOTOR desligada, o laço calcula as decisões a cada ciclo e as grava aqui,
-- sem enfileirar comando. O relatório confronta com as `acoes` do motor do legado.
CREATE TABLE nexus.motor_decisao (
    id      bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    em      timestamptz NOT NULL DEFAULT now(),
    -- Instante do ciclo (ms), o `agora` em que as regras rodaram.
    ciclo   bigint NOT NULL,
    nick    text   NOT NULL,
    -- Regra do legado que decidiu (`motorPocao`, `motorHunt`...).
    regra   text   NOT NULL,
    -- alerta | comando | registro | recarregarPainel | trocarHunt
    decisao text   NOT NULL,
    dados   jsonb  NOT NULL
);
CREATE INDEX motor_decisao_ciclo ON nexus.motor_decisao (ciclo, id);
CREATE INDEX motor_decisao_nick_ciclo ON nexus.motor_decisao (nick, ciclo DESC);

-- Ações do motor do legado vistas pela sombra a cada ciclo. O legado guarda só as últimas 300;
-- aqui elas se acumulam para o confronto cobrir dias. Sem repetição pela chave natural. A
-- chave usa o md5 do texto: um texto longo estouraria o limite de tamanho do índice.
CREATE TABLE nexus.motor_acao_legado (
    t         bigint NOT NULL,
    nick      text   NOT NULL,
    tipo      text   NOT NULL,
    texto     text   NOT NULL,
    texto_md5 text   GENERATED ALWAYS AS (md5(texto)) STORED,
    ok        boolean,
    dados     jsonb  NOT NULL,
    PRIMARY KEY (t, nick, tipo, texto_md5)
);
