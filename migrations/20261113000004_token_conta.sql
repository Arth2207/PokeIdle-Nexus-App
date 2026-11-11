-- Token preso a uma conta do jogo: nas rotas de equipes ele só enxerga essa conta.
-- Sem conta (NULL), o token vê todas, como antes.
ALTER TABLE nexus.api_token ADD COLUMN conta text;
