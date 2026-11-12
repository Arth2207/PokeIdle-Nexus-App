//! ETL de eficiência: agrega `fato_kill` → `fato_hunt_eficiencia` (ou shadow).
//!
//! Porta fiel de `etl-eficiencia.js`: agrega kills por `(hunt_slug, hora)`
//! nas últimas 24h, horas completas (ainda não a hora em andamento).
//! Idempotente: rodar o mesmo intervalo duas vezes produz o mesmo resultado
//! (ON CONFLICT DO UPDATE substitui os valores).

use anyhow::Result;
use sqlx::PgPool;
use tracing::info;

/// Roda um ciclo do ETL de eficiência.
/// Retorna número de linhas upsertadas.
pub async fn ciclo(db: &PgPool, escrita: bool) -> Result<u64> {
    let tabela = if escrita {
        "public.fato_hunt_eficiencia"
    } else {
        "nexus.fato_hunt_eficiencia_shadow"
    };

    // Verifica se há kills com hunt_slug nas últimas 24h
    let (com_hunt,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM public.fato_kill
          WHERE hunt_slug IS NOT NULL
            AND ts_at >= NOW() - INTERVAL '24 hours'",
    )
    .fetch_one(db)
    .await?;

    if com_hunt == 0 {
        info!("etl_eficiencia: sem kills com hunt_slug nas últimas 24h, pulando");
        return Ok(0);
    }

    // Upsert: agrega por (hunt_slug, hora completa), horas completas das últimas 24h
    // O legado usa especie_ativa='unknown', faixa_nivel=0, tipo_ativo_pri='unknown'
    // porque a fato_kill legada não tem esses campos preenchidos.
    // SAFETY: tabela é 'public.fato_hunt_eficiencia' ou 'nexus.fato_hunt_eficiencia_shadow' — sem entrada do usuário.
    let sql_insert = format!(
        "INSERT INTO {tabela}
           (especie_ativa, faixa_nivel, tipo_ativo_pri, hunt_slug, ts_hora,
            kills_total, xp_tr_total, xp_pk_total, ouro_total, duracao_s)
         SELECT
           'unknown'                                                           AS especie_ativa,
           0                                                                   AS faixa_nivel,
           'unknown'                                                           AS tipo_ativo_pri,
           hunt_slug,
           EXTRACT(epoch FROM date_trunc('hour', ts_at))::bigint * 1000       AS ts_hora,
           COUNT(*)                                                            AS kills_total,
           COALESCE(SUM(xp_treinador), 0)                                     AS xp_tr_total,
           COALESCE(SUM(xp_pokemon),   0)                                     AS xp_pk_total,
           COALESCE(SUM(ouro),         0)                                     AS ouro_total,
           GREATEST(EXTRACT(epoch FROM (MAX(ts_at) - MIN(ts_at)))::int + 60, 60) AS duracao_s
         FROM public.fato_kill
         WHERE hunt_slug IS NOT NULL
           AND ts_at >= NOW() - INTERVAL '24 hours'
           AND ts_at < date_trunc('hour', NOW())
         GROUP BY hunt_slug, date_trunc('hour', ts_at)
         HAVING COUNT(*) >= 10
         ON CONFLICT (especie_ativa, faixa_nivel, tipo_ativo_pri, hunt_slug, ts_hora)
         DO UPDATE SET
           kills_total = EXCLUDED.kills_total,
           xp_tr_total = EXCLUDED.xp_tr_total,
           xp_pk_total = EXCLUDED.xp_pk_total,
           ouro_total  = EXCLUDED.ouro_total,
           duracao_s   = EXCLUDED.duracao_s"
    );
    let result = sqlx::query(sqlx::AssertSqlSafe(sql_insert))
        .execute(db)
        .await?;

    let upsertados = result.rows_affected();

    // Limpa linhas antigas (>48h) com as colunas padrão do legado
    let sql_delete = format!(
        "DELETE FROM {tabela}
          WHERE especie_ativa = 'unknown' AND faixa_nivel = 0
            AND ts_hora < EXTRACT(epoch FROM NOW() - INTERVAL '48 hours')::bigint * 1000"
    );
    // SAFETY: tabela é literal estático, sem entrada do usuário.
    let deletados = sqlx::query(sqlx::AssertSqlSafe(sql_delete))
        .execute(db)
        .await?
        .rows_affected();

    info!(upsertados, deletados, "etl_eficiencia: ciclo ok");

    metrics::gauge!("nexus_worker_efic_linhas").set(upsertados as f64);

    Ok(upsertados)
}
