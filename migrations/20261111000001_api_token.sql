-- Tokens de acesso à API. Guardamos só o hash SHA-256; o token em si é mostrado
-- uma única vez, na criação.
CREATE TABLE nexus.api_token (
    id          bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    nome        text        NOT NULL UNIQUE,
    hash        bytea       NOT NULL UNIQUE CHECK (length(hash) = 32),
    escopos     text[]      NOT NULL CHECK (
                    cardinality(escopos) > 0
                    AND escopos <@ ARRAY['leitura', 'escrita', 'relay', 'admin']
                ),
    criado_em   timestamptz NOT NULL DEFAULT now(),
    expira_em   timestamptz,
    revogado_em timestamptz
);

COMMENT ON TABLE nexus.api_token IS 'Tokens Bearer da API. Só o hash é armazenado.';
