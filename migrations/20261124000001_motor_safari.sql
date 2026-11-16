-- Disponibiliza o controle Safari para as contas já configuradas, sem ativá-lo.
INSERT INTO nexus.motor_config (nick, secao, dados, texto)
SELECT nick, 'safari', '{"ativo":false}'::jsonb, '{"ativo":false}'::json
FROM (SELECT DISTINCT nick FROM nexus.motor_config) AS contas
ON CONFLICT (nick, secao) DO NOTHING;
