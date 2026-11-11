//! Montagem do roteador HTTP.

mod saude;

use axum::routing::get;
use axum::Router;
use tower_http::trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer};
use tracing::Level;

use crate::estado::Estado;

pub fn router(estado: Estado) -> Router {
    // Uma linha de log por requisição: método, caminho, status e latência.
    let log_requisicoes = TraceLayer::new_for_http()
        .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
        .on_response(DefaultOnResponse::new().level(Level::INFO));

    Router::new()
        .route("/health/live", get(saude::vivo))
        .route("/health/ready", get(saude::pronto))
        .layer(log_requisicoes)
        .with_state(estado)
}

#[cfg(test)]
mod testes {
    use std::time::Duration;

    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use sqlx::postgres::PgPoolOptions;
    use tower::ServiceExt;

    use super::*;

    /// Estado apontando para uma porta onde não há banco nenhum.
    fn estado_sem_banco() -> Estado {
        let db = PgPoolOptions::new()
            .acquire_timeout(Duration::from_millis(300))
            .connect_lazy("postgres://ninguem:nada@127.0.0.1:1/inexistente")
            .unwrap();
        Estado { db }
    }

    async fn status_de(caminho: &str) -> StatusCode {
        let requisicao = Request::builder().uri(caminho).body(Body::empty()).unwrap();
        router(estado_sem_banco())
            .oneshot(requisicao)
            .await
            .unwrap()
            .status()
    }

    #[tokio::test]
    async fn live_responde_mesmo_sem_banco() {
        assert_eq!(status_de("/health/live").await, StatusCode::OK);
    }

    #[tokio::test]
    async fn ready_fica_indisponivel_sem_banco() {
        assert_eq!(
            status_de("/health/ready").await,
            StatusCode::SERVICE_UNAVAILABLE
        );
    }

    #[tokio::test]
    async fn rota_inexistente_da_404() {
        assert_eq!(status_de("/nao-existe").await, StatusCode::NOT_FOUND);
    }
}
