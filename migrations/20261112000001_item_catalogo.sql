-- Catálogo de itens do jogo (id, nome, ícone). A bolsa usa para achar o id de um item
-- pelo nome, já que a maioria das vendas de item chega sem item_id.
-- `ordem` preserva a ordem da fonte: se dois itens tiverem o mesmo nome, vale o último,
-- como no sistema legado.
CREATE TABLE nexus.item_catalogo (
    id            integer     PRIMARY KEY,
    ordem         integer     NOT NULL,
    nome          text        NOT NULL,
    categoria     text,
    raro          boolean     NOT NULL DEFAULT false,
    preco_npc     bigint,
    icone         text,
    atualizado_em timestamptz NOT NULL DEFAULT now()
);

COMMENT ON TABLE nexus.item_catalogo IS 'Catálogo de itens do jogo, importado de assets/items.json.';
