WITH limites AS (
    SELECT to_timestamp($2::float8 / 1000) agora,
           (date_trunc('week', (to_timestamp($2::float8 / 1000) AT TIME ZONE 'America/Sao_Paulo') + interval '1 day')
               - interval '1 day') AT TIME ZONE 'America/Sao_Paulo' inicio
), conta AS (
    SELECT $1::text rotulo,
           lower(janela->>'nick') nick FROM nexus.motor_janela WHERE conta = $1
    UNION ALL SELECT $1::text, $1::text
), observados AS (
    SELECT min(e.em) primeira FROM nexus.evento e CROSS JOIN limites l
    WHERE e.em >= l.inicio AND e.em <= l.agora
      AND (e.conta IN (SELECT nick FROM conta) OR e.conta = $1 OR e.conta_id = $1)
), brutas AS MATERIALIZED (
    SELECT e.* FROM nexus.evento e CROSS JOIN limites l
    WHERE e.tipo = 'capturado' AND e.em >= l.inicio AND e.em <= l.agora
      AND (e.conta IN (SELECT nick FROM conta) OR e.conta = $1 OR e.conta_id = $1)
), capturas AS MATERIALIZED (
    -- WS tem id do pokémon; HUD tem seq. Evita contar as duas fontes da mesma captura.
    SELECT DISTINCT ON (coalesce(nullif(dados->>'id', ''), encode(chave, 'hex')))
           em, dados
    FROM brutas b
    WHERE nullif(b.dados->>'id', '') IS NOT NULL OR NOT EXISTS (
        SELECT 1 FROM brutas ws WHERE nullif(ws.dados->>'id', '') IS NOT NULL
          AND ws.dados->>'especie' = b.dados->>'especie'
          AND ws.em BETWEEN b.em - interval '3 seconds' AND b.em + interval '3 seconds'
    )
    ORDER BY coalesce(nullif(dados->>'id', ''), encode(chave, 'hex')), em
), classificadas AS MATERIALIZED (
    SELECT c.em, c.dados, p.id especie_id, p.dados->>'n' nome,
           p.dados->>'c' = 'pokebola' AND NOT (p.id >= 2000 AND p.id < 3000)
               AND lower(coalesce(c.dados->>'bola', '')) NOT LIKE '%beast%' elegivel
    FROM capturas c LEFT JOIN nexus.pokedex_motor p
      ON c.dados->>'speciesId' = p.id::text
      OR (nullif(c.dados->>'speciesId', '') IS NULL
          AND lower(btrim(c.dados->>'especie')) = lower(p.dados->>'n'))
), primeiras AS MATERIALIZED (
    SELECT especie_id, min(em) em FROM classificadas WHERE elegivel GROUP BY especie_id
), top_resumo AS (
    SELECT count(*) especies,
           count(*) FILTER (WHERE p.em >= l.agora - interval '24 hours') recentes
    FROM primeiras p CROSS JOIN limites l
), catalogo AS (
    SELECT count(*) total FROM nexus.pokedex_motor
    WHERE dados->>'c' = 'pokebola' AND NOT (id >= 2000 AND id < 3000)
), resumo AS (
    SELECT count(*) total,
           count(*) FILTER (WHERE em >= l.agora - interval '24 hours') recentes,
           count(DISTINCT dados->>'especie') especies
    FROM capturas CROSS JOIN limites l
), ritmo AS (
    SELECT CASE WHEN o.primeira IS NULL THEN 0 ELSE
           greatest(0, least(24, extract(epoch FROM (l.agora - o.primeira)) / 3600)) END::float8 horas
    FROM limites l CROSS JOIN observados o
), dias AS (
    SELECT d::date dia, count(c.em) total
    FROM limites l CROSS JOIN LATERAL generate_series(
        l.inicio AT TIME ZONE 'America/Sao_Paulo',
        (l.inicio AT TIME ZONE 'America/Sao_Paulo') + interval '6 days', interval '1 day') d
    LEFT JOIN capturas c ON (c.em AT TIME ZONE 'America/Sao_Paulo')::date = d::date
    GROUP BY d
), especies AS (
    SELECT coalesce(nullif(dados->>'especie', ''), 'Desconhecida') nome, count(*) total
    FROM capturas GROUP BY 1 ORDER BY total DESC, nome LIMIT 15
), ultimas AS (
    SELECT c.em, c.dados, c.especie_id, c.elegivel, (c.elegivel AND c.em = p.em) nova
    FROM classificadas c LEFT JOIN primeiras p USING (especie_id)
    ORDER BY c.em DESC LIMIT 20
)
SELECT jsonb_build_object(
    'inicio', extract(epoch FROM l.inicio) * 1000,
    'fim', extract(epoch FROM (l.inicio + interval '7 days')) * 1000,
    'agora', $2::bigint, 'total', s.total, 'especies', s.especies,
    'recentes', s.recentes, 'horas_observadas', r.horas,
    'por_hora', CASE WHEN r.horas >= 1 THEN s.recentes / r.horas END,
    'projecao', CASE WHEN r.horas >= 1 THEN s.total + s.recentes / r.horas *
        greatest(0, extract(epoch FROM (l.inicio + interval '7 days' - l.agora)) / 3600) END,
    'dias', (SELECT jsonb_agg(jsonb_build_object('dia', dia, 'total', total) ORDER BY dia) FROM dias),
    'top_catch', jsonb_build_object(
        'inicio', extract(epoch FROM l.inicio) * 1000,
        'fim', extract(epoch FROM (l.inicio + interval '7 days')) * 1000,
        'catalogo_disponivel', cat.total > 0, 'catalogo_total', cat.total,
        'especies', tc.especies, 'recentes', tc.recentes,
        'por_hora', CASE WHEN r.horas >= 1 THEN tc.recentes / r.horas END,
        'projecao', CASE WHEN r.horas >= 1 THEN least(cat.total,
            tc.especies + tc.recentes / r.horas * greatest(0,
                extract(epoch FROM (l.inicio + interval '7 days' - l.agora)) / 3600)) END,
        'capturadas_ids', coalesce((SELECT jsonb_agg(especie_id ORDER BY especie_id) FROM primeiras), '[]'),
        'sem_classificacao', (SELECT count(*) FROM classificadas WHERE especie_id IS NULL),
        'descartadas', (SELECT count(*) FROM classificadas WHERE elegivel IS FALSE),
        'dias', (SELECT jsonb_agg(jsonb_build_object('dia', d.dia, 'total',
            (SELECT count(*) FROM primeiras p WHERE (p.em AT TIME ZONE 'America/Sao_Paulo')::date = d.dia)) ORDER BY d.dia) FROM dias d)
    ),
    'por_especie', coalesce((SELECT jsonb_agg(jsonb_build_object('nome', nome, 'total', total) ORDER BY total DESC, nome) FROM especies), '[]'),
    'ultimas', coalesce((SELECT jsonb_agg(jsonb_build_object('em', extract(epoch FROM em) * 1000,
        'especie', dados->>'especie', 'nivel', dados->>'nivel',
        'classificada', especie_id IS NOT NULL, 'elegivel', elegivel, 'nova', nova) ORDER BY em DESC) FROM ultimas), '[]')
) FROM limites l CROSS JOIN resumo s CROSS JOIN ritmo r CROSS JOIN top_resumo tc CROSS JOIN catalogo cat
