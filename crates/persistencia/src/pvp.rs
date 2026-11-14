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

/// Partidas de PvP das contas do dono.
pub async fn partidas(db: &PgPool, desde_ms: i64) -> Result<Vec<(i64, Value)>, sqlx::Error> {
    eventos(db, "pvp.partida", desde_ms, 20_000).await
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
