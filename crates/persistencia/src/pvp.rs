//! Leituras de PvP, shiny do servidor e fichas de raid em `nexus.evento` (o app grava).

use serde_json::Value;
use sqlx::PgPool;

/// (instante em ms, `dados`) dos eventos de um tipo desde `desde_ms`, do mais velho ao mais novo.
async fn eventos(
    db: &PgPool,
    tipo: &str,
    desde_ms: i64,
    limite: i64,
) -> Result<Vec<(i64, Value)>, sqlx::Error> {
    sqlx::query_as(
        "SELECT (extract(epoch FROM em) * 1000)::bigint, dados
           FROM nexus.evento
          WHERE tipo = $1 AND em >= to_timestamp($2::float8 / 1000)
          ORDER BY em
          LIMIT $3",
    )
    .bind(tipo)
    .bind(desde_ms as f64)
    .bind(limite)
    .fetch_all(db)
    .await
}

/// Limites reais dos rankings na hora de Brasília. A semana recomeça no domingo.
pub async fn calendario(db: &PgPool, mensal: bool) -> Result<(i64, i64), sqlx::Error> {
    sqlx::query_as(
        "WITH relogio AS (SELECT now() AT TIME ZONE 'America/Sao_Paulo' AS local),
         periodo AS (
             SELECT CASE WHEN $1 THEN date_trunc('month', local)
                         ELSE date_trunc('week', local + interval '1 day') - interval '1 day' END AS inicio,
                    CASE WHEN $1 THEN interval '1 month' ELSE interval '7 days' END AS duracao
               FROM relogio
         )
         SELECT (extract(epoch FROM inicio AT TIME ZONE 'America/Sao_Paulo') * 1000)::bigint,
                (extract(epoch FROM (inicio + duracao) AT TIME ZONE 'America/Sao_Paulo') * 1000)::bigint
           FROM periodo",
    )
    .bind(mensal)
    .fetch_one(db)
    .await
}

/// Partidas de PvP das contas do dono.
pub async fn partidas(db: &PgPool, desde_ms: i64) -> Result<Vec<(i64, Value)>, sqlx::Error> {
    sqlx::query_as(
        "SELECT (extract(epoch FROM em) * 1000)::bigint, dados FROM nexus.evento
          WHERE tipo = 'pvp.partida' AND em >= to_timestamp($1::float8 / 1000)
          ORDER BY em DESC LIMIT 20000",
    )
    .bind(desde_ms as f64)
    .fetch_all(db)
    .await
}

/// Avisos de shiny capturado no servidor.
pub async fn shiny_servidor(db: &PgPool, desde_ms: i64) -> Result<Vec<(i64, Value)>, sqlx::Error> {
    eventos(db, "shiny.servidor", desde_ms, 100_000).await
}

/// Último evento de um tipo de uma conta (ou de qualquer conta, com `None`). A conta pode vir
/// pelo rótulo do app ou pelo nick do jogo (o evento é gravado pelo rótulo).
async fn ultimo(
    db: &PgPool,
    tipo: &str,
    conta: Option<&str>,
) -> Result<Option<(i64, Value)>, sqlx::Error> {
    sqlx::query_as(
        "SELECT (extract(epoch FROM em) * 1000)::bigint, dados
           FROM nexus.evento
          WHERE tipo = $1 AND ($2::text IS NULL OR conta IN (SELECT $2 UNION SELECT conta FROM nexus.motor_janela WHERE lower(janela->>'nick') = lower($2)))
          ORDER BY em DESC
          LIMIT 1",
    )
    .bind(tipo)
    .bind(conta)
    .fetch_optional(db)
    .await
}

/// Elo e formação mais recentes da conta (`pvp.estado`).
pub async fn estado_pvp(db: &PgPool, conta: &str) -> Result<Option<(i64, Value)>, sqlx::Error> {
    ultimo(db, "pvp.estado", Some(conta)).await
}

/// Fichas de raid mais recentes (`boss.fichas`).
pub async fn fichas_raid(db: &PgPool) -> Result<Option<(i64, Value)>, sqlx::Error> {
    ultimo(db, "boss.fichas", None).await
}

/// Resultados da Área de Treinamento da conta, do mais novo ao mais velho.
pub async fn treinos(
    db: &PgPool,
    conta: &str,
    limite: i64,
) -> Result<Vec<(i64, Value)>, sqlx::Error> {
    sqlx::query_as(
        "SELECT (extract(epoch FROM em) * 1000)::bigint, dados
           FROM nexus.evento
          WHERE tipo = 'treino.resultado' AND conta IN (SELECT $1 UNION SELECT conta FROM nexus.motor_janela WHERE lower(janela->>'nick') = lower($1))
          ORDER BY em DESC
          LIMIT $2",
    )
    .bind(conta)
    .bind(limite)
    .fetch_all(db)
    .await
}

/// Extrato do Mercado das contas (`mercado.extrato`, do app a cada 30 min) desde `desde_ms`:
/// (conta, lado "venda"/"compra", linha). A mesma linha volta em várias leituras; sai uma vez.
pub async fn extrato_mercado(
    db: &PgPool,
    desde_ms: i64,
) -> Result<Vec<(String, String, Value)>, sqlx::Error> {
    let linhas: Vec<(String, String, Value)> = sqlx::query_as(
        "SELECT DISTINCT ON (e.conta, e.dados->>'lado', l->>'id')
                e.conta, e.dados->>'lado', l
           FROM nexus.evento e, jsonb_array_elements(e.dados->'linhas') l
          WHERE e.tipo = 'mercado.extrato'
            AND (l->>'em')::float8 >= $1
          ORDER BY e.conta, e.dados->>'lado', l->>'id', e.em DESC",
    )
    .bind(desde_ms as f64)
    .fetch_all(db)
    .await?;
    Ok(linhas)
}

/// Partidas de PvP da conta (rótulo ou nick) na semana do ranking (desde domingo 00:00 de
/// Brasília): vitórias, derrotas, ganho médio na vitória, perda média na derrota e quantas
/// partidas nas últimas 6 h (o ritmo).
pub async fn semana_pvp(db: &PgPool, conta: &str) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT jsonb_build_object(
                  'vitorias', count(*) FILTER (WHERE (dados->>'venci')::boolean),
                  'derrotas', count(*) FILTER (WHERE NOT (dados->>'venci')::boolean),
                  'ganho', avg((dados->>'delta')::float8) FILTER (WHERE (dados->>'venci')::boolean),
                  'perda', avg((dados->>'delta')::float8) FILTER (WHERE NOT (dados->>'venci')::boolean),
                  'ultimas6h', count(*) FILTER (WHERE em > now() - interval '6 hours'),
                  'desde', (extract(epoch FROM min(em)) * 1000)::bigint)
           FROM nexus.evento
          WHERE tipo = 'pvp.partida'
            AND conta IN (SELECT $1 UNION SELECT conta FROM nexus.motor_janela
                           WHERE lower(janela->>'nick') = lower($1))
            AND em >= (date_trunc('day', now() AT TIME ZONE 'America/Sao_Paulo')
                       - make_interval(days => extract(dow FROM now() AT TIME ZONE 'America/Sao_Paulo')::int))
                      AT TIME ZONE 'America/Sao_Paulo'",
    )
    .bind(conta)
    .fetch_one(db)
    .await
}

/// Pontos das posições 10, 100 e 500 do ranking: os da semana atual (eventos `pvp.cortes` e
/// tabelas vistas pela Descoberta) e os do fim da semana anterior (meta de fim de semana).
/// `{atual: {semana, total, cortes}, anterior: {semana, cortes}}`.
pub async fn cortes_pvp(db: &PgPool) -> Result<Value, sqlx::Error> {
    let linhas: Vec<Value> = sqlx::query_scalar(
        "WITH semana AS (
           SELECT (date_trunc('week', (now() AT TIME ZONE 'America/Sao_Paulo') + interval '1 day')
                    - interval '1 day') AT TIME ZONE 'America/Sao_Paulo' AS inicio
         ), periodos AS (
           SELECT inicio, now() AS fim FROM semana UNION ALL
           SELECT inicio - interval '7 days', inicio FROM semana
         )
         SELECT t.leitura FROM periodos CROSS JOIN LATERAL (
           (SELECT jsonb_build_object('semana', dados->>'semana', 'total', dados->'total',
                                     'cortes', dados->'cortes', 'em', em) AS leitura
             FROM nexus.evento WHERE tipo = 'pvp.cortes'
               AND em >= periodos.inicio AND em < periodos.fim ORDER BY em DESC LIMIT 30)
           UNION ALL
           (SELECT jsonb_build_object('semana', dados->'dados'->>'semana',
                     'total', dados->'dados'->'total', 'cortes',
                     (SELECT coalesce(jsonb_object_agg(l->>0, l->4), '{}')
                        FROM jsonb_array_elements(dados->'dados'->'linhas') l
                       WHERE (l->>0)::int IN (10, 50, 100, 500)), 'em', em)
             FROM nexus.evento WHERE tipo = 'ws.bruto' AND dados->>'tipo' = 'pvpTabela'
               AND (dados->'dados'->>'total')::int > 0
               AND em >= periodos.inicio AND em < periodos.fim ORDER BY em DESC LIMIT 40)
         ) t",
    )
    .fetch_all(db)
    .await?;
    let mut cortes = juntar_cortes(linhas);
    let inicio: String = sqlx::query_scalar(
        "SELECT to_char(date_trunc('week', (now() AT TIME ZONE 'America/Sao_Paulo') + interval '1 day')
                         - interval '1 day', 'YYYY-MM-DD')"
    ).fetch_one(db).await?;
    if cortes["atual"]["semana"].as_str() != Some(&inicio) {
        cortes = serde_json::json!({"atual":null,"anterior":cortes["atual"]});
    }
    Ok(cortes)
}

/// Junta as leituras (mais novas primeiro em cada fonte) por semana: em cada semana vale a
/// leitura mais nova de cada posição.
fn juntar_cortes(mut linhas: Vec<Value>) -> Value {
    let em = |v: &Value| {
        v.get("em")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    linhas.sort_by_key(|v| std::cmp::Reverse(em(v)));
    let mut semanas: Vec<(String, i64, serde_json::Map<String, Value>)> = Vec::new();
    for l in linhas {
        let s = l
            .get("semana")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        if s.is_empty() {
            continue;
        }
        let i = match semanas.iter().position(|x| x.0 == s) {
            Some(i) => i,
            None => {
                semanas.push((
                    s,
                    l.get("total").and_then(Value::as_i64).unwrap_or(0),
                    Default::default(),
                ));
                semanas.len() - 1
            }
        };
        for (p, v) in l
            .get("cortes")
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
        {
            semanas[i].2.entry(p.clone()).or_insert_with(|| v.clone());
        }
    }
    semanas.sort_by(|a, b| b.0.cmp(&a.0));
    let sem = |x: Option<&(String, i64, serde_json::Map<String, Value>)>| {
        x.map_or(
            Value::Null,
            |(s, t, c)| serde_json::json!({"semana": s, "total": t, "cortes": c}),
        )
    };
    serde_json::json!({"atual": sem(semanas.first()), "anterior": sem(semanas.get(1))})
}

/// Nick do jogo (minúsculo) de uma conta pelo rótulo do app (`alllphha` → `alllphhha`), pela
/// janela do motor. Sem janela com esse rótulo, `None` (o nome já é o nick).
pub async fn nick_do_jogo(db: &PgPool, nome: &str) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT lower(janela->>'nick') FROM nexus.motor_janela
          WHERE lower(conta) = lower($1) AND coalesce(janela->>'nick', '') <> ''",
    )
    .bind(nome)
    .fetch_optional(db)
    .await
}

/// Linhas do mensal (eventos `pvp.mensal` do app e capturas `pvpMensal` da Descoberta) no
/// mesmo formato: `{mes, fimEm, eu, faixas, total, cortes, em}`, das mais novas.
const SQL_MENSAL: &str = "
    SELECT conta, x FROM (
      SELECT conta, em, jsonb_build_object('mes', dados->>'mes', 'fimEm', dados->'fimEm',
               'eu', dados->'eu', 'faixas', dados->'faixas', 'total', dados->'total',
               'cortes', coalesce(dados->'cortes', '{}'), 'em', em) x
        FROM nexus.evento WHERE tipo = 'pvp.mensal' AND em >= now() - interval '75 days'
      UNION ALL
      SELECT conta, em, jsonb_build_object('mes', d->>'mes', 'fimEm', d->'fimEm', 'eu', d->'eu',
               'faixas', d->'faixas', 'total', d->'tabela'->'total',
               'cortes', (SELECT coalesce(jsonb_object_agg(l->>0, l->3), '{}')
                            FROM jsonb_array_elements(coalesce(d->'tabela'->'linhas', '[]')) l
                           WHERE (l->>0)::int IN (10, 50, 100, 500, 2000)), 'em', em)
        FROM (SELECT conta, em, dados->'dados' d FROM nexus.evento
               WHERE tipo = 'ws.bruto' AND dados->>'tipo' = 'pvpMensal' AND em >= now() - interval '75 days') b
    ) u";

/// Ranking mensal da conta (rótulo ou nick) e os cortes do mês, juntando todas as contas:
/// `{eu: {...} | null, cortes: {atual: {semana: mês, total, cortes}, anterior}}`.
pub async fn mensal_pvp(db: &PgPool, conta: &str) -> Result<Value, sqlx::Error> {
    // SAFETY: SQL_MENSAL é constante; a conta vai como parâmetro ($1).
    let eu: Option<(String, Value)> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{SQL_MENSAL} WHERE conta IN (SELECT $1 UNION SELECT conta FROM nexus.motor_janela
                                       WHERE lower(janela->>'nick') = lower($1))
                        AND x->'eu' IS NOT NULL AND x->'eu' <> 'null'
                        AND x->>'mes' = to_char(now() AT TIME ZONE 'America/Sao_Paulo', 'YYYY-MM')
          ORDER BY x->>'em' DESC LIMIT 1"
    )))
    .bind(conta)
    .fetch_optional(db)
    .await?;
    let linhas: Vec<(String, Value)> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "WITH leituras AS ({SQL_MENSAL}), meses AS (
             SELECT DISTINCT x->>'mes' AS mes FROM leituras WHERE x->>'mes' IS NOT NULL
             ORDER BY mes DESC LIMIT 2
         )
         SELECT l.conta, l.x FROM meses CROSS JOIN LATERAL
            (SELECT conta, x FROM leituras WHERE x->>'mes' = meses.mes
             ORDER BY x->>'em' DESC LIMIT 80) l"
    )))
    .fetch_all(db)
    .await?;
    let ritmo: Option<f64> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "WITH leituras AS ({SQL_MENSAL}), amostra AS (
            SELECT (x#>>'{{eu,pontos}}')::float8 AS pontos, (x->>'em')::timestamptz AS em
            FROM leituras
            WHERE conta IN (SELECT $1 UNION SELECT conta FROM nexus.motor_janela
                            WHERE lower(janela->>'nick') = lower($1))
              AND x->>'mes' = to_char(now() AT TIME ZONE 'America/Sao_Paulo', 'YYYY-MM')
              AND x#>>'{{eu,pontos}}' IS NOT NULL
              AND (x->>'em')::timestamptz >= now() - interval '7 days'
         ), antigo AS (SELECT * FROM amostra ORDER BY em LIMIT 1),
            recente AS (SELECT * FROM amostra ORDER BY em DESC LIMIT 1)
         SELECT CASE WHEN recente.em - antigo.em >= interval '24 hours'
                     THEN (recente.pontos - antigo.pontos) * 86400 / extract(epoch FROM recente.em - antigo.em)
                END FROM antigo CROSS JOIN recente"
    )))
    .bind(conta)
    .fetch_optional(db)
    .await?
    .flatten();
    // O mês faz o papel da semana na junção dos cortes.
    let como_semana: Vec<Value> = linhas
        .into_iter()
        .map(|(_, mut x)| {
            if let Value::Object(o) = &mut x {
                let mes = o.get("mes").cloned().unwrap_or(Value::Null);
                o.insert("semana".into(), mes);
            }
            x
        })
        .collect();
    Ok(
        serde_json::json!({"eu": eu.map(|x| x.1), "cortes": juntar_cortes(como_semana), "saldoDia": ritmo}),
    )
}

#[cfg(test)]
mod testes_cortes {
    use super::*;
    use serde_json::json;

    #[test]
    fn cada_semana_fica_com_a_leitura_mais_nova_de_cada_posicao() {
        let v = juntar_cortes(vec![
            json!({"semana": "2026-11-08", "total": 1000, "cortes": {"10": 8817, "100": 8059}, "em": "2026-11-15T02:47"}),
            json!({"semana": "2026-11-08", "total": 1000, "cortes": {"10": 8000, "500": 7000}, "em": "2026-11-14T10:00"}),
            json!({"semana": "2026-11-15", "total": 50, "cortes": {"10": 120}, "em": "2026-11-15T04:00"}),
        ]);
        assert_eq!(v["atual"]["semana"], "2026-11-15");
        assert_eq!(
            v["anterior"]["cortes"],
            json!({"10": 8817, "100": 8059, "500": 7000})
        );
    }
}
