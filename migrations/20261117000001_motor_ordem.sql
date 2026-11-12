-- Fase 4: ordem das chaves do estado do motor. Só aditiva.
--
-- O motor do legado itera objetos na ordem de inserção (a ordem do arquivo), e isso decide
-- resultados: empates nos drops do dia, a ordem das contas no financeiro. O jsonb guarda as
-- chaves na ordem dele. `texto` (json guarda o texto como veio) e `ordem` (posição no
-- arquivo) preservam a do legado; `dados` continua sendo a coluna para consultas.
-- Linhas importadas antes desta migration ficam com `texto` nulo até a próxima importação.

ALTER TABLE nexus.motor_config ADD COLUMN texto json, ADD COLUMN ordem int;
ALTER TABLE nexus.motor_acao ADD COLUMN texto json;
ALTER TABLE nexus.motor_estado ADD COLUMN texto json, ADD COLUMN ordem int;
