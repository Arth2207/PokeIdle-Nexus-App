-- Timescale em nexus.evento: compressão e agregado contínuo por hora. Só aditiva.
-- O agregado contínuo só pode ser criado dentro de transação com `WITH NO DATA` (como abaixo).
-- Sem a extensão, tudo é ignorado. Funções qualificadas pelo schema da extensão (o role do
-- Nexus tem search_path=nexus e a extensão mora em public).
-- SEM política de retenção: apagar dados exige autorização. Opção, se um dia autorizado:
--   SELECT <esquema>.add_retention_policy('nexus.evento', interval '180 days');

-- Compressão: segmenta por conta e tipo, ordena por tempo decrescente, comprime após 7 dias.
DO $$
DECLARE
    esquema text;
BEGIN
    SELECT extnamespace::regnamespace::text INTO esquema
      FROM pg_extension WHERE extname = 'timescaledb';
    IF esquema IS NOT NULL THEN
        ALTER TABLE nexus.evento SET (
            timescaledb.compress,
            timescaledb.compress_segmentby = 'conta,tipo',
            timescaledb.compress_orderby = 'em DESC'
        );
        EXECUTE format(
            'SELECT %I.add_compression_policy(%L::regclass, interval %L, if_not_exists => true)',
            esquema, 'nexus.evento', '7 days');
    END IF;
END
$$;

-- Agregado contínuo: contagem de eventos por hora, conta e tipo. Em tempo real
-- (`materialized_only = false`): as horas ainda não materializadas saem direto de `nexus.evento`.
DO $$
DECLARE
    esquema text;
BEGIN
    SELECT extnamespace::regnamespace::text INTO esquema
      FROM pg_extension WHERE extname = 'timescaledb';
    IF esquema IS NOT NULL THEN
        EXECUTE format($v$
            CREATE MATERIALIZED VIEW IF NOT EXISTS nexus.evento_hora
            WITH (timescaledb.continuous, timescaledb.materialized_only = false) AS
            SELECT %1$I.time_bucket(interval '1 hour', em) AS hora,
                   conta,
                   tipo,
                   count(*) AS total
              FROM nexus.evento
             GROUP BY 1, 2, 3
            WITH NO DATA
        $v$, esquema);
        EXECUTE format(
            'SELECT %I.add_continuous_aggregate_policy(%L::regclass, start_offset => interval %L, end_offset => interval %L, schedule_interval => interval %L, if_not_exists => true)',
            esquema, 'nexus.evento_hora', '3 days', '1 hour', '30 minutes');
    END IF;
END
$$;
