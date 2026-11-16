-- Disponibiliza o controle Safari para as contas já configuradas, sem ativá-lo.
INSERT INTO nexus.motor_config (nick, secao, dados, texto)
SELECT DISTINCT nick, 'safari', '{"ativo":false}'::jsonb, '{"ativo":false}'::json
FROM nexus.motor_config
ON CONFLICT (nick, secao) DO NOTHING;
