-- Dados que o precificador da bolsa usa e que no sistema legado vivem em arquivos JS/JSON.

-- Catálogo de espécies do jogo: status base, tipos, golpes e evoluções.
-- `golpes` é uma lista de [nome, tipo, categoria (0 físico, 1 especial), poder, recarga_ms, nivel_minimo].
-- `ordem` preserva a ordem da fonte: a busca por nome fica com a primeira ocorrência.
CREATE TABLE nexus.especie (
    id         integer PRIMARY KEY,
    ordem      integer NOT NULL,
    nome       text    NOT NULL,
    tipos      text[]  NOT NULL DEFAULT '{}',
    base       integer[] NOT NULL CHECK (cardinality(base) = 6),
    golpes     jsonb   NOT NULL DEFAULT '[]',
    evolucoes  integer[] NOT NULL DEFAULT '{}',
    mega_base  integer,
    sprite_id  integer,
    desempate  integer
);

-- Vendas antigas de pokémon (antes da captura com ficha completa), já decompostas pelo
-- precificador: preço base sem adicionais, PvE, família etc.
CREATE TABLE nexus.venda_base (
    em          bigint           NOT NULL,
    preco       bigint           NOT NULL,
    base        double precision NOT NULL,
    adicionais  double precision NOT NULL,
    pve         double precision NOT NULL,
    familia     integer          NOT NULL,
    familia_nome text            NOT NULL,
    shiny       boolean          NOT NULL,
    nivel       integer          NOT NULL,
    especie     integer          NOT NULL,
    nome        text             NOT NULL,
    incerta     boolean          NOT NULL,
    nota        double precision,
    mega        boolean          NOT NULL,
    refinada    boolean          NOT NULL
);
CREATE INDEX venda_base_em ON nexus.venda_base (em);

-- Cotação diária de itens (mediana em orbs) herdada da mesma base.
CREATE TABLE nexus.item_preco_base (
    dia     date             NOT NULL,
    item_id integer          NOT NULL,
    preco   double precision NOT NULL,
    PRIMARY KEY (dia, item_id)
);

-- Parâmetros e pequenos estados (pesos da confiança, viés da correção, carimbos de tempo).
CREATE TABLE nexus.parametro (
    chave         text        PRIMARY KEY,
    valor         jsonb       NOT NULL,
    atualizado_em timestamptz NOT NULL DEFAULT now()
);

-- Inventário de itens informado pelo painel ou pelo relay (antes: .bolsa-meus-itens.json).
CREATE TABLE nexus.meus_itens (
    item_id    integer PRIMARY KEY CHECK (item_id > 0),
    quantidade bigint  NOT NULL CHECK (quantidade > 0)
);
