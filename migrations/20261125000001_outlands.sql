-- Objetivo explícito do usuário não é sobrescrito pelo papel da conta.
UPDATE nexus.motor_config SET dados=dados || '{"objetivoManual":false}'::jsonb,
  texto=(dados || '{"objetivoManual":false}'::jsonb)::json
WHERE secao='hunt' AND NOT dados ? 'objetivoManual';
-- Catálogo de regras observado em cada login do novo app, preservado por versão.
CREATE TABLE nexus.outland_regra (
  id text PRIMARY KEY, observado_em timestamptz NOT NULL DEFAULT now(), dados jsonb NOT NULL
);
