//! Métricas no formato Prometheus.
//!
//! Ficam numa porta separada (`NEXUS_PORTA_METRICAS`), que o Nginx nunca expõe.
//! Só o Prometheus, na rede interna do Docker, chega nela.

use std::time::Instant;

use axum::extract::{MatchedPath, Request, State};
use axum::middleware::Next;
use axum::response::Response;
use axum::routing::get;
use axum::Router;
use metrics_exporter_prometheus::{BuildError, Matcher, PrometheusBuilder, PrometheusHandle};

use crate::estado::Estado;

const BALDES_SEGUNDOS: &[f64] = &[
    0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
];

/// Instala o coletor global de métricas. Chamar uma vez, no início do processo.
pub fn instalar() -> Result<PrometheusHandle, BuildError> {
    let handle = PrometheusBuilder::new()
        .set_buckets_for_metric(
            Matcher::Suffix("_duracao_segundos".to_owned()),
            BALDES_SEGUNDOS,
        )?
        .install_recorder()?;

    metrics::gauge!("nexus_build_info", "versao" => env!("CARGO_PKG_VERSION")).set(1.0);
    Ok(handle)
}

/// Middleware: conta requisições e mede a latência por método, rota e status.
pub async fn registrar(req: Request, next: Next) -> Response {
    let inicio = Instant::now();
    let metodo = req.method().to_string();
    // Usamos o padrão da rota ("/api/bolsa/ticker"), nunca o caminho real, para que
    // ids e parâmetros não criem uma série nova a cada requisição.
    let rota = req
        .extensions()
        .get::<MatchedPath>()
        .map_or("nao_mapeada", MatchedPath::as_str)
        .to_owned();

    let resposta = next.run(req).await;

    let rotulos = [
        ("metodo", metodo),
        ("rota", rota),
        ("status", resposta.status().as_u16().to_string()),
    ];
    metrics::counter!("nexus_http_requisicoes_total", &rotulos).increment(1);
    metrics::histogram!("nexus_http_duracao_segundos", &rotulos)
        .record(inicio.elapsed().as_secs_f64());

    resposta
}

#[derive(Clone)]
struct EstadoMetricas {
    handle: PrometheusHandle,
    app: Estado,
}

pub fn router(handle: PrometheusHandle, app: Estado) -> Router {
    Router::new()
        .route("/metrics", get(exportar))
        .with_state(EstadoMetricas { handle, app })
}

async fn exportar(State(estado): State<EstadoMetricas>) -> String {
    // Medidas "do momento" são lidas na hora da coleta.
    let pool = &estado.app.db;
    metrics::gauge!("nexus_db_conexoes").set(f64::from(pool.size()));
    metrics::gauge!("nexus_db_conexoes_ociosas").set(pool.num_idle() as f64);
    estado.handle.render()
}
