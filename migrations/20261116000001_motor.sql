-- Fase 4: estado do motor de automação, importado do holodex-dados.json e do contas.json do
-- legado. Só aditiva. Até o cutover o arquivo é a fonte: a importação substitui tudo.

-- Contas do motor (contas.json), na ordem do arquivo.
CREATE TABLE nexus.motor_conta (
    id    text PRIMARY KEY,
    nome  text NOT NULL,
    cor   text,
    ordem int  NOT NULL
);

-- Configuração por conta: uma linha por seção (`hunt`, `compra`, `pedras`...), para comparar
-- e versionar seção por seção. O formato de cada seção ainda varia: fica em jsonb.
CREATE TABLE nexus.motor_config (
    nick  text  NOT NULL,
    secao text  NOT NULL,
    dados jsonb NOT NULL,
    PRIMARY KEY (nick, secao)
);

-- Histórico de ações (`acoes`). Os campos variam com o tipo: o registro inteiro fica em
-- `dados`, e as colunas servem para filtrar. `id` cresce do mais antigo para o mais novo.
CREATE TABLE nexus.motor_acao (
    id    bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    nick  text,
    t     bigint NOT NULL,
    tipo  text,
    ok    boolean,
    dados jsonb  NOT NULL
);
CREATE INDEX motor_acao_nick_t ON nexus.motor_acao (nick, t DESC);

-- Demais chaves de topo do arquivo (`dia`, `saldo`, `horas`, `builds`, `calib2`, `pausa`...).
-- `acesso` (e-mails do túnel) não é do motor e não é importada.
CREATE TABLE nexus.motor_estado (
    chave text  PRIMARY KEY,
    dados jsonb NOT NULL
);

-- Registro de cada importação, para saber de qual arquivo veio o estado.
CREATE TABLE nexus.motor_importacao (
    id     bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    em     timestamptz NOT NULL DEFAULT now(),
    tipo   text NOT NULL,
    sha256 text NOT NULL,
    linhas int  NOT NULL
);
