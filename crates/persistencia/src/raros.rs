//! Estado de raros e shiny (`nexus.estado_raros`), no lugar do `raros-estado.json`.
//!
//! O estado inteiro é uma linha só (`global`). Quem altera lê com `FOR UPDATE`, aplica e
//! grava na mesma transação, então dois ingests simultâneos não perdem atualização.

use nucleo::raros::LinhaCofre;
use serde_json::Value;
use sqlx::{PgPool, Row};

/// O estado atual (um objeto vazio se ainda não há nada).
pub async fn carregar(db: &PgPool) -> Result<Value, sqlx::Error> {
    let v: Option<Value> =
        sqlx::query_scalar("SELECT estado FROM nexus.estado_raros WHERE chave = 'global'")
            .fetch_optional(db)
            .await?;
    Ok(v.unwrap_or_else(|| Value::Object(Default::default())))
}

/// Lê o estado com a linha travada, aplica `f` e grava. Se `f` devolver erro, nada é gravado.
pub async fn modificar<T, E>(
    db: &PgPool,
    f: impl FnOnce(&mut Value) -> Result<T, E>,
) -> Result<Result<T, E>, sqlx::Error> {
    let mut tx = db.begin().await?;
    sqlx::query(
        "INSERT INTO nexus.estado_raros (chave, estado) VALUES ('global', '{}') ON CONFLICT DO NOTHING",
    )
    .execute(&mut *tx)
    .await?;
    let mut estado: Value = sqlx::query_scalar(
        "SELECT estado FROM nexus.estado_raros WHERE chave = 'global' FOR UPDATE",
    )
    .fetch_one(&mut *tx)
    .await?;
    match f(&mut estado) {
        Ok(v) => {
            sqlx::query(
                "UPDATE nexus.estado_raros SET estado = $1, atualizado_em = now() WHERE chave = 'global'",
            )
            .bind(&estado)
            .execute(&mut *tx)
            .await?;
            tx.commit().await?;
            Ok(Ok(v))
        }
        Err(e) => {
            tx.rollback().await?;
            Ok(Err(e))
        }
    }
}

/// Substitui o estado inteiro (a importação do `raros-estado.json`).
pub async fn substituir(db: &PgPool, estado: &Value) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO nexus.estado_raros (chave, estado) VALUES ('global', $1)
         ON CONFLICT (chave) DO UPDATE SET estado = EXCLUDED.estado, atualizado_em = now()",
    )
    .bind(estado)
    .execute(db)
    .await?;
    Ok(())
}

/// Arremessos de ultra ball para capturar um shiny: `[mínimo, mediana, máximo]` das espécies
/// (`public.dim_especie`, só leitura). `None` se a tabela não existe ou está vazia.
pub async fn captura_shiny(db: &PgPool) -> Option<[f64; 3]> {
    let l = sqlx::query(
        "SELECT min(shiny_1_em_ultra_ball)::float8 AS mn,
                (percentile_cont(0.5) WITHIN GROUP (ORDER BY shiny_1_em_ultra_ball))::float8 AS md,
                max(shiny_1_em_ultra_ball)::float8 AS mx
         FROM public.dim_especie WHERE shiny_1_em_ultra_ball > 0",
    )
    .fetch_one(db)
    .await
    .ok()?;
    let md: f64 = l
        .try_get::<Option<f64>, _>("md")
        .ok()
        .flatten()
        .filter(|m| *m > 0.0)?;
    Some([
        l.try_get::<Option<f64>, _>("mn").ok().flatten()?,
        md,
        l.try_get::<Option<f64>, _>("mx").ok().flatten()?,
    ])
}

/// O histórico do cofre desde `desde_ms` (`public.fato_cofre`, só leitura; é o legado que
/// grava). Sem a tabela, a lista vem vazia.
pub async fn historico_cofre(db: &PgPool, desde_ms: i64) -> Vec<LinhaCofre> {
    let linhas = sqlx::query(
        "SELECT ts, nick, coalesce(itens, 0) AS itens, coalesce(pokemons, 0) AS pokemons,
                coalesce(shiny, 0) AS shiny, total
         FROM public.fato_cofre WHERE ts >= $1 AND total > 0 ORDER BY ts",
    )
    .bind(desde_ms)
    .fetch_all(db)
    .await
    .unwrap_or_default();
    linhas
        .iter()
        .filter_map(|l| {
            Some(LinhaCofre {
                ts: l.try_get("ts").ok()?,
                nick: l.try_get("nick").ok()?,
                itens: l.try_get("itens").ok()?,
                pokemons: l.try_get("pokemons").ok()?,
                shiny: l.try_get("shiny").ok()?,
                total: l.try_get("total").ok()?,
            })
        })
        .collect()
}
