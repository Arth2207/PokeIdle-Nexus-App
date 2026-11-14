-- Fase 4 com escrita: o estado do laço do motor (marcas, alertas, última hunt) sobrevive a
-- reinícios e deploys. Uma linha só (id = 1). Aditiva.
CREATE TABLE IF NOT EXISTS nexus.motor_laco (
    id smallint PRIMARY KEY DEFAULT 1 CHECK (id = 1),
    em timestamptz NOT NULL DEFAULT now(),
    dados jsonb NOT NULL
);
