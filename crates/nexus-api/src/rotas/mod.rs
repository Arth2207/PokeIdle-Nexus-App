//! Montagem do roteador HTTP.

mod bolsa;
mod comandos;
mod equipes;
mod eventos;
pub(crate) mod ingestao;
mod motor;
mod pvp;
mod raros;
mod saude;
mod vendas;

use axum::extract::DefaultBodyLimit;
use axum::http::{header, HeaderValue, Method};
use axum::middleware;
use axum::routing::{get, post};
use axum::{Extension, Json, Router};
use tower_http::compression::CompressionLayer;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::trace::{DefaultOnResponse, TraceLayer};
use tracing::Level;

use crate::auth::{self, Cliente, Escopo};
use crate::estado::Estado;
use crate::{etag, limite, metricas};

/// O corpo como JSON. Como no legado, corpo vazio ou inválido vira `{}` (nunca dá 400 por
/// JSON malformado: cai nas validações de campo de cada rota).
pub(crate) fn corpo_json(corpo: &[u8]) -> serde_json::Value {
    serde_json::from_slice(corpo).unwrap_or_else(|_| serde_json::json!({}))
}

/// Escopos aceitos em cada grupo de rotas.
const LEITURA: &[Escopo] = &[Escopo::Leitura];
const RELAY: &[Escopo] = &[Escopo::Relay];
const INGESTAO: &[Escopo] = &[Escopo::Relay, Escopo::Escrita];

const LIMITE_CORPO: usize = 2 * 1024 * 1024;
const LIMITE_CORPO_INGESTAO: usize = 10 * 1024 * 1024;

pub fn router(estado: Estado) -> Router {
    // Leitura da bolsa: escopo `leitura`, ETag e CORS para o painel web.
    // Os middlewares de rota rodam de baixo para cima: token → rate limit → escopo → handler.
    let leitura = Router::new()
        .route("/api/bolsa/mercado", get(bolsa::mercado))
        .route("/api/bolsa/ticker", get(bolsa::ticker))
        .route("/api/bolsa/balao", get(bolsa::balao))
        .route("/api/bolsa/serie", get(bolsa::serie))
        .route("/api/bolsa/feedback", get(bolsa::feedback))
        .route("/api/bolsa/dicas", get(bolsa::dicas))
        .route("/api/bolsa/ofertas", get(bolsa::ofertas))
        .route("/api/bolsa/meus-itens", get(bolsa::meus_itens))
        .route("/api/equipes/bosses", get(equipes::bosses))
        .route("/api/equipes/plano", get(equipes::plano))
        .route("/api/bolsa/equipe", get(equipes::equipe_bolsa))
        .route("/api/bolsa/equipe-propria", get(equipes::equipe_propria))
        .route("/api/bolsa/alvos-coleta", get(equipes::alvos_coleta))
        .route("/api/bolsa/equipe-hibrida", get(equipes::equipe_hibrida))
        .route("/api/raros2", get(raros::raros2))
        .route("/api/shiny", get(raros::shiny))
        .route("/api/eventos", get(eventos::historico))
        .route("/api/pvp/meta", get(pvp::meta_pvp))
        .route("/api/pvp/sugestao", get(pvp::sugestao_pvp))
        .route("/api/pvp/treinos", get(pvp::treinos))
        .route("/api/shiny/servidor", get(pvp::shiny_servidor))
        .route("/api/boss/fichas", get(pvp::fichas_raid))
        .merge(motor::rotas())
        .route_layer(middleware::from_fn(|req, next| {
            auth::exigir_escopo(LEITURA, req, next)
        }))
        .route_layer(middleware::from_fn(etag::etag));

    // Ingestão: escopo `relay` ou `escrita`, corpo de até 10 MB, sem CORS (o relay não é
    // navegador). As rotas só gravam com NEXUS_ESCRITA_BOLSA=true.
    let ingestao = Router::new()
        .route("/api/bolsa/ofertas", post(bolsa::gravar_ofertas))
        .route("/api/bolsa/resumo-itens", post(bolsa::gravar_resumo))
        .route("/api/bolsa/meus-itens", post(bolsa::gravar_meus_itens))
        .route("/api/bolsa/vendas", post(vendas::gravar))
        .route("/api/equipes/ingest", post(equipes::ingest))
        .route("/api/boss/tentativa", post(equipes::tentativa))
        .route("/ingest", post(ingestao::ingest))
        .route("/api/hunt-sync", post(ingestao::hunt_sync))
        .route("/api/raros/ingest", post(raros::ingest))
        .route("/api/shiny/registrar", post(raros::registrar))
        .route("/api/comandos", get(comandos::listar))
        .route("/api/confirmar-comando", post(comandos::confirmar))
        .route("/api/inserir-comando", post(comandos::inserir))
        .route("/api/comandos/falhos", get(comandos::falhos))
        .route("/api/comandos/{id}/reenviar", post(comandos::reenviar))
        .route_layer(middleware::from_fn(|req, next| {
            auth::exigir_escopo(INGESTAO, req, next)
        }))
        .layer(DefaultBodyLimit::max(LIMITE_CORPO_INGESTAO));

    // WebSocket: token com escopo `relay`; sem ele, 401/403 antes do upgrade.
    let ws = Router::new()
        .route("/ws", get(crate::ws::handler))
        .route_layer(middleware::from_fn(|req, next| {
            auth::exigir_escopo(RELAY, req, next)
        }));

    // Cada grupo recebe token e rate limit. O CORS fica por fora do token nas leituras,
    // porque o preflight (OPTIONS) do navegador não manda token.
    let leitura = protegido(leitura, &estado);
    let leitura = match cors(&estado.origens_cors) {
        Some(c) => leitura.layer(c),
        None => leitura,
    };
    let protegidas = Router::new()
        .merge(protegido(Router::new().route("/api/eu", get(eu)), &estado))
        .merge(leitura)
        .merge(protegido(ingestao, &estado))
        .merge(protegido(ws, &estado));

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
        .layer(DefaultBodyLimit::max(LIMITE_CORPO))
        .layer(CompressionLayer::new())
        .layer(middleware::from_fn(metricas::registrar))
        .layer(log_requisicoes)
        .layer(PropagateRequestIdLayer::x_request_id())
        .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
        .with_state(estado)
}

/// Exige token válido e aplica o rate limit por cliente (token roda primeiro).
fn protegido(r: Router<Estado>, estado: &Estado) -> Router<Estado> {
    r.route_layer(middleware::from_fn_with_state(
        estado.clone(),
        limite::limitar,
    ))
    .route_layer(middleware::from_fn_with_state(
        estado.clone(),
        auth::exigir_token,
    ))
}

/// CORS só para as origens configuradas; sem origem configurada, nenhum cabeçalho CORS.
/// A ingestão não precisa: o relay não é navegador.
fn cors(origens: &[String]) -> Option<CorsLayer> {
    let origens: Vec<HeaderValue> = origens
        .iter()
        .filter_map(|o| HeaderValue::from_str(o).ok())
        .collect();
    (!origens.is_empty()).then(|| {
        CorsLayer::new()
            .allow_origin(AllowOrigin::list(origens))
            .allow_methods([Method::GET])
            .allow_headers([header::AUTHORIZATION])
    })
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
    async fn equipes_sem_token_da_401() {
        for caminho in ["/api/equipes/bosses", "/api/equipes/plano?boss=articuno"] {
            assert_eq!(status_de(get(caminho)).await, StatusCode::UNAUTHORIZED);
        }
        let ingest = Request::post("/api/equipes/ingest")
            .header("content-type", "application/json")
            .body(Body::from("{}"))
            .unwrap();
        assert_eq!(status_de(ingest).await, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn rotas_da_fase_3_sem_token_dao_401() {
        for (metodo, caminho) in [
            ("POST", "/ingest"),
            ("POST", "/api/hunt-sync"),
            ("POST", "/api/raros/ingest"),
            ("POST", "/api/shiny/registrar"),
            ("GET", "/api/raros2"),
            ("GET", "/api/shiny"),
            ("GET", "/api/comandos?nick=x"),
            ("POST", "/api/confirmar-comando"),
            ("POST", "/api/inserir-comando"),
        ] {
            let req = Request::builder()
                .method(metodo)
                .uri(caminho)
                .body(Body::empty())
                .unwrap();
            assert_eq!(status_de(req).await, StatusCode::UNAUTHORIZED, "{caminho}");
        }
    }

    #[test]
    fn corpo_invalido_vira_objeto_vazio() {
        assert_eq!(corpo_json(b"nao e json"), serde_json::json!({}));
        assert_eq!(corpo_json(b""), serde_json::json!({}));
        assert_eq!(corpo_json(br#"{"a":1}"#), serde_json::json!({"a": 1}));
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
