-- Fase 4: janela de cada conta para o motor do Nexus, montada pelo app (A2) com o que lê do
-- WebSocket do jogo (o mesmo objeto do /api/nexus/janela do motor do legado). Uma linha por
-- conta, substituída a cada 30 s; o login (pesado) chega de vez em quando e fica guardado à parte.
CREATE TABLE IF NOT EXISTS nexus.motor_janela (
    conta     text PRIMARY KEY,
    em        timestamptz NOT NULL DEFAULT now(),
    janela    jsonb NOT NULL,
    login     jsonb,
    login_em  timestamptz
);
