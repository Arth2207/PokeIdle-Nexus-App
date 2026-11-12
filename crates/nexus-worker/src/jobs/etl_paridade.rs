//! Comparador de paridade: public.fato_kill × nexus.fato_kill_shadow.
//! Roda a cada hora; grava em nexus.etl_paridade para análise posterior.

use anyhow::Result;
use sqlx::PgPool;
use tracing::info;

const CONTAS: &[&str] = &["mvxorqh6", "mvxoui28", "mvxoxith", "mw002fcf"];
const JANELAS: &[(&str, &str)] = &[("1h", "1 hour"), ("6h", "6 hours"), ("24h", "24 hours")];

pub async fn ciclo(db: &PgPool) -> Result<usize> {
    let mut ok = 0usize;
    for conta_id in CONTAS {
        for (label, intervalo) in JANELAS {
            if let Err(e) = comparar(db, conta_id, label, intervalo).await {
                tracing::warn!(
                    conta = conta_id,
                    janela = label,
                    erro = %e,
                    "etl_paridade: erro ao comparar"
                );
            } else {
                ok += 1;
            }
        }
    }
    info!(comparacoes = ok, "etl_paridade: ciclo ok");
    Ok(ok)
}

async fn comparar(db: &PgPool, conta_id: &str, label: &str, intervalo: &str) -> Result<()> {
    let (linhas_node, soma_xp_node): (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*)::bigint,
                COALESCE(SUM(COALESCE(xp_treinador,0)::bigint + COALESCE(xp_pokemon,0)::bigint), 0)
           FROM public.fato_kill
          WHERE conta_id = $1 AND ts_at >= NOW() - $2::interval",
    )
    .bind(conta_id)
    .bind(intervalo)
    .fetch_one(db)
    .await?;

    let (linhas_nexus, soma_xp_nexus): (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*)::bigint,
                COALESCE(SUM(COALESCE(xp_treinador,0)::bigint + COALESCE(xp_pokemon,0)::bigint), 0)
           FROM nexus.fato_kill_shadow
          WHERE conta_id = $1 AND ts_at >= NOW() - $2::interval",
    )
    .bind(conta_id)
    .bind(intervalo)
    .fetch_one(db)
    .await?;

    sqlx::query(
        "INSERT INTO nexus.etl_paridade
                (janela, conta_id, linhas_node, linhas_nexus, soma_xp_node, soma_xp_nexus)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(label)
    .bind(conta_id)
    .bind(linhas_node)
    .bind(linhas_nexus)
    .bind(soma_xp_node)
    .bind(soma_xp_nexus)
    .execute(db)
    .await?;

    Ok(())
}

/// Registros mais recentes de etl_paridade das últimas `horas` horas, um por (conta_id, janela).
pub async fn relatorio(db: &PgPool, horas: u32) -> Result<Vec<LinhaRelatorio>> {
    let linhas = sqlx::query_as::<_, LinhaRelatorio>(
        "SELECT DISTINCT ON (conta_id, janela)
                em, janela, conta_id, linhas_node, linhas_nexus,
                soma_xp_node, soma_xp_nexus, igual
           FROM nexus.etl_paridade
          WHERE em >= NOW() - ($1::int * interval '1 hour')
          ORDER BY conta_id, janela, em DESC",
    )
    .bind(horas as i32)
    .fetch_all(db)
    .await?;
    Ok(linhas)
}

#[derive(sqlx::FromRow, Debug)]
pub struct LinhaRelatorio {
    #[allow(dead_code)]
    pub em: chrono::DateTime<chrono::Utc>,
    pub janela: String,
    pub conta_id: String,
    pub linhas_node: i64,
    pub linhas_nexus: i64,
    pub soma_xp_node: i64,
    pub soma_xp_nexus: i64,
    pub igual: bool,
}
