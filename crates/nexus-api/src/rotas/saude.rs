//! Health checks.
//!
//! - `/health/live`: o processo está de pé. O Docker usa para decidir se reinicia.
//! - `/health/ready`: o processo consegue atender (banco acessível). O Nginx usa para decidir
//!   se manda tráfego.

use std::time::Duration;

use axum::extract::State;
use axum::http::StatusCode;

use crate::estado::Estado;

const TIMEOUT_BANCO: Duration = Duration::from_secs(2);

pub async fn vivo() -> &'static str {
    "ok"
}

pub async fn pronto(State(estado): State<Estado>) -> (StatusCode, &'static str) {
    let checagem = sqlx::query("SELECT 1").execute(&estado.db);

    match tokio::time::timeout(TIMEOUT_BANCO, checagem).await {
        Ok(Ok(_)) => (StatusCode::OK, "ok"),
        Ok(Err(erro)) => {
            tracing::warn!(%erro, "readiness: banco indisponível");
            (StatusCode::SERVICE_UNAVAILABLE, "banco indisponível")
        }
        Err(_) => {
            tracing::warn!(timeout = ?TIMEOUT_BANCO, "readiness: banco não respondeu a tempo");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                "banco não respondeu a tempo",
            )
        }
    }
}
