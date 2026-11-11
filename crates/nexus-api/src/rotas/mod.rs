//! Montagem do roteador HTTP.

mod bolsa;
mod saude;

use axum::middleware;
use axum::routing::get;
use axum::{Extension, Json, Router};
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::trace::{DefaultOnResponse, TraceLayer};
use tracing::Level;

use crate::auth::{self, Cliente, Escopo};
use crate::estado::Estado;
use crate::metricas;

pub fn router(estado: Estado) -> Router {
    // Leitura da bolsa: exige o escopo `leitura`. O middleware é uma closure que fixa a
    // lista de escopos e repassa a requisição para `exigir_escopo`.
    let bolsa_leitura = Router::new()
        .route("/api/bolsa/serie", get(bolsa::serie))
        .route_layer(middleware::from_fn(|req, next| {
            auth::exigir_escopo(&[Escopo::Leitura], req, next)
        }));

    // Rotas que exigem token. O `route_layer` aplica o middleware só a elas. Como ele é
    // adicionado por último, roda antes do de escopo: primeiro "quem é", depois "pode?".
    let protegidas = Router::new()
        .route("/api/eu", get(eu))
        .merge(bolsa_leitura)
        .route_layer(middleware::from_fn_with_state(
            estado.clone(),
            auth::exigir_token,
        ));

    // Uma linha de log por requisição: método, caminho, status, latência e request id.
    let log_requisicoes = TraceLayer::new_for_http()
        .make_span_with(|req: &axum::http::Request<_>| {
            let request_id = req
                .headers()
                .get("x-request-id")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("-");
            tracing::info_span!(
                "requisicao",
                metodo = %req.method(),
                caminho = %req.uri().path(),
                request_id,
                cliente = tracing::field::Empty,
            )
        })
        .on_response(DefaultOnResponse::new().level(Level::INFO));

    // Camadas são aplicadas de baixo para cima: a última adicionada é a primeira a rodar.
    Router::new()
        .route("/health/live", get(saude::vivo))
        .route("/health/ready", get(saude::pronto))
        .merge(protegidas)
        .layer(middleware::from_fn(metricas::registrar))
        .layer(log_requisicoes)
        .layer(PropagateRequestIdLayer::x_request_id())
        .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
        .with_state(estado)
}

/// Mostra quem é o dono do token. Útil para testar se um token funciona.
async fn eu(Extension(cliente): Extension<Cliente>) -> Json<Cliente> {
    Json(cliente)
}

#[cfg(test)]
mod testes {
    use std::time::Duration;

    use axum::body::Body;
    use axum::http::{header, Request, StatusCode};
    use sqlx::postgres::PgPoolOptions;
    use tower::ServiceExt;

    use super::*;

    /// Estado apontando para uma porta onde não há banco nenhum.
    fn estado_sem_banco() -> Estado {
        let db = PgPoolOptions::new()
            .acquire_timeout(Duration::from_millis(300))
            .connect_lazy("postgres://ninguem:nada@127.0.0.1:1/inexistente")
            .unwrap();
        Estado::com_pool(db)
    }

    async fn status_de(req: Request<Body>) -> StatusCode {
        router(estado_sem_banco())
            .oneshot(req)
            .await
            .unwrap()
            .status()
    }

    fn get(caminho: &str) -> Request<Body> {
        Request::builder().uri(caminho).body(Body::empty()).unwrap()
    }

    fn get_com_token(caminho: &str, token: &str) -> Request<Body> {
        Request::builder()
            .uri(caminho)
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap()
    }

    #[tokio::test]
    async fn live_responde_mesmo_sem_banco() {
        assert_eq!(status_de(get("/health/live")).await, StatusCode::OK);
    }

    #[tokio::test]
    async fn ready_fica_indisponivel_sem_banco() {
        assert_eq!(
            status_de(get("/health/ready")).await,
            StatusCode::SERVICE_UNAVAILABLE
        );
    }

    #[tokio::test]
    async fn rota_inexistente_da_404() {
        assert_eq!(status_de(get("/nao-existe")).await, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn rota_protegida_sem_token_da_401() {
        assert_eq!(status_de(get("/api/eu")).await, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn token_sem_prefixo_da_401_sem_ir_ao_banco() {
        let req = get_com_token("/api/eu", "qualquer-coisa");
        assert_eq!(status_de(req).await, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn token_valido_no_formato_mas_banco_fora_da_503() {
        let req = get_com_token("/api/eu", "nxs_0000");
        assert_eq!(status_de(req).await, StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn bolsa_sem_token_da_401() {
        assert_eq!(
            status_de(get("/api/bolsa/serie?id=i:1:orb")).await,
            StatusCode::UNAUTHORIZED
        );
    }

    #[tokio::test]
    async fn resposta_carrega_request_id() {
        let resposta = router(estado_sem_banco())
            .oneshot(get("/health/live"))
            .await
            .unwrap();
        assert!(resposta.headers().contains_key("x-request-id"));
    }
}
