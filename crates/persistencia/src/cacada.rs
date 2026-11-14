//! Leituras da tela Caçada: a sessão da conta por hora (`fato_kill`), o ranking e o mapa de calor
//! das hunts (`fato_hunt_eficiencia`) e os drops recentes (`nexus.evento`).

use serde_json::{json, Value};
use sqlx::PgPool;

/// Contas do app: `conta_id` do `fato_kill` → rótulo da conta no app (`nexus.evento.conta`).
pub const CONTAS_A2: &[(&str, &str)] = &[
    ("mvxorqh6", "alllphha"),
    ("mvxoui28", "sikien"),
    ("mvxoxith", "alllphha2"),
    ("mw002fcf", "alllphha3"),
];

/// `conta_id` do `fato_kill` de um rótulo de conta do app.
pub fn conta_id(rotulo: &str) -> Option<&'static str> {
    CONTAS_A2
        .iter()
        .find(|(_, r)| r.eq_ignore_ascii_case(rotulo))
        .map(|(id, _)| *id)
}

/// A sessão da conta hora a hora nas últimas 24 h, a média por hora dos 7 dias anteriores e a
/// hunt atual (a do último abate).
pub async fn sessao(db: &PgPool, conta_id: &str) -> Result<Value, sqlx::Error> {
    let horas: Vec<Value> = sqlx::query_scalar(
        "SELECT jsonb_build_object(
                 'hora', (extract(epoch FROM date_trunc('hour', ts_at)) * 1000)::bigint,
                 'kills', count(*), 'ouro', coalesce(sum(ouro), 0),
                 'xp', coalesce(sum(xp_treinador), 0))
           FROM public.fato_kill
          WHERE conta_id = $1 AND ts_at > now() - interval '24 hours'
          GROUP BY date_trunc('hour', ts_at) ORDER BY date_trunc('hour', ts_at)",
    )
    .bind(conta_id)
    .fetch_all(db)
    .await?;
    let media: Option<Value> = sqlx::query_scalar(
        "SELECT jsonb_build_object(
                 'kills', count(*)::float8 / greatest(count(DISTINCT date_trunc('hour', ts_at)), 1),
                 'ouro', coalesce(sum(ouro), 0)::float8 / greatest(count(DISTINCT date_trunc('hour', ts_at)), 1),
                 'xp', coalesce(sum(xp_treinador), 0)::float8 / greatest(count(DISTINCT date_trunc('hour', ts_at)), 1))
           FROM public.fato_kill
          WHERE conta_id = $1
            AND ts_at BETWEEN now() - interval '8 days' AND now() - interval '24 hours'",
    )
    .bind(conta_id)
    .fetch_optional(db)
    .await?;
    let atual: Option<String> = sqlx::query_scalar(
        "SELECT hunt_slug FROM public.fato_kill
          WHERE conta_id = $1 AND hunt_slug IS NOT NULL ORDER BY ts_at DESC LIMIT 1",
    )
    .bind(conta_id)
    .fetch_optional(db)
    .await?
    .flatten();
    Ok(json!({"horas": horas, "media": media, "atual": atual}))
}

/// Hunts dos últimos 7 dias (todas as contas): médias por hora de ouro, XP e abates, e o
/// mapa de calor hunt × hora do dia (UTC−3) do ouro por hora das 6 melhores.
pub async fn hunts(db: &PgPool) -> Result<Value, sqlx::Error> {
    let lista: Vec<Value> = sqlx::query_scalar(
        "SELECT jsonb_build_object('hunt', hunt_slug, 'ouro', avg(ouro_por_hora),
                 'xp', avg(xp_tr_por_hora), 'kills', avg(kills_por_hora), 'horas', count(*))
           FROM public.fato_hunt_eficiencia
          WHERE ts_hora > (extract(epoch FROM now() - interval '7 days') * 1000)
            AND duracao_s >= 1800
          GROUP BY hunt_slug ORDER BY avg(ouro_por_hora) DESC LIMIT 15",
    )
    .fetch_all(db)
    .await?;
    let calor: Vec<Value> = sqlx::query_scalar(
        "WITH top AS (
             SELECT hunt_slug FROM public.fato_hunt_eficiencia
              WHERE ts_hora > (extract(epoch FROM now() - interval '14 days') * 1000)
              GROUP BY hunt_slug ORDER BY avg(ouro_por_hora) DESC LIMIT 6)
         SELECT jsonb_build_object('hunt', e.hunt_slug,
                 'hora', extract(hour FROM to_timestamp(e.ts_hora / 1000) - interval '3 hours')::int,
                 'ouro', avg(e.ouro_por_hora))
           FROM public.fato_hunt_eficiencia e JOIN top USING (hunt_slug)
          WHERE e.ts_hora > (extract(epoch FROM now() - interval '14 days') * 1000)
          GROUP BY e.hunt_slug,
                   extract(hour FROM to_timestamp(e.ts_hora / 1000) - interval '3 hours')",
    )
    .fetch_all(db)
    .await?;
    Ok(json!({"lista": lista, "calor": calor}))
}

/// Drops da conta nas últimas 3 h: item, quantidade e a espécie que derrubou.
pub async fn drops(db: &PgPool, rotulo: &str) -> Result<Vec<Value>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT jsonb_build_object('item', dados->>'item_nome',
                 'qtd', sum(coalesce((dados->>'quantidade')::int, 1)),
                 'de', string_agg(DISTINCT dados->>'alvo_especie', ', '))
           FROM nexus.evento
          WHERE tipo = 'drop' AND conta = $1 AND em > now() - interval '3 hours'
          GROUP BY dados->>'item_nome'
          ORDER BY sum(coalesce((dados->>'quantidade')::int, 1)) DESC LIMIT 20",
    )
    .bind(rotulo.to_lowercase())
    .fetch_all(db)
    .await
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn rotulo_para_conta_id() {
        assert_eq!(conta_id("Sikien"), Some("mvxoui28"));
        assert_eq!(conta_id("outra"), None);
    }
}
