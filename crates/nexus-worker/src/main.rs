//! nexus-worker: scheduler de jobs do Nexus (ETL, eficiência, dossiê).
//!
//! Uso:
//!   nexus-worker              sobe o worker
//!   nexus-worker healthcheck  checa o /health/ready local (usado pelo Docker)

mod configuracao;
mod jobs;
mod metricas;

use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;

use anyhow::{bail, Context};
use axum::http::StatusCode;
use axum::routing::get;
use axum::Router;
use sqlx::postgres::PgPoolOptions;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

use crate::configuracao::Configuracao;

const USO: &str = "uso: nexus-worker [healthcheck]";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    let cfg = Configuracao::carregar().context("configuração inválida")?;
    let args: Vec<String> = std::env::args().skip(1).collect();

    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        [] => executar(cfg).await,
        ["healthcheck"] => healthcheck(&cfg).await,
        _ => {
            eprintln!("{USO}");
            std::process::exit(2);
        }
    }
}

async fn executar(cfg: Configuracao) -> anyhow::Result<()> {
    iniciar_logs(cfg.log_formato);

    let handle = metricas::instalar().context("não consegui instalar as métricas")?;

    let db = PgPoolOptions::new()
        .max_connections(3)
        .acquire_timeout(Duration::from_secs(30))
        .connect(&cfg.database_url)
        .await
        .context("não consegui conectar ao banco")?;

    let parar = CancellationToken::new();

    // Sinal de desligamento (SIGTERM/Ctrl-C)
    {
        let parar = parar.clone();
        tokio::spawn(async move {
            tokio::signal::ctrl_c().await.ok();
            info!("sinal recebido, encerrando");
            parar.cancel();
        });
    }

    // Servidor HTTP mínimo: /health/ready e /metrics
    let servidor = iniciar_servidor(cfg.porta_metricas, db.clone(), handle, parar.clone());
    tokio::spawn(servidor);

    info!(
        escrita_etl = cfg.escrita_etl,
        sqlite = %cfg.sqlite_path,
        "nexus-worker iniciado"
    );

    // Scheduler: dois loops independentes com CancellationToken
    let etl = loop_etl_continuo(
        db.clone(),
        cfg.sqlite_path.clone(),
        cfg.etl_intervalo,
        cfg.escrita_etl,
        parar.clone(),
    );
    let efic = loop_etl_eficiencia(
        db.clone(),
        cfg.etl_efic_intervalo,
        cfg.escrita_etl,
        parar.clone(),
    );

    tokio::join!(etl, efic);

    info!("nexus-worker encerrado");
    Ok(())
}

async fn loop_etl_continuo(
    db: sqlx::PgPool,
    sqlite_path: String,
    intervalo: Duration,
    escrita: bool,
    parar: CancellationToken,
) {
    // Roda imediatamente no boot, depois dorme o intervalo
    loop {
        let inicio = tokio::time::Instant::now();
        match jobs::etl_continuo::ciclo(&db, &sqlite_path, escrita).await {
            Ok(n) => {
                metrics::gauge!("nexus_worker_etl_ultima_ok").set(1.0);
                if n > 0 {
                    info!(inseridos = n, "etl_continuo: ciclo ok");
                }
            }
            Err(e) => {
                metrics::gauge!("nexus_worker_etl_ultima_ok").set(0.0);
                error!(erro = %e, "etl_continuo: ciclo falhou");
            }
        }
        metrics::gauge!("nexus_worker_etl_duracao_s").set(inicio.elapsed().as_secs_f64());

        tokio::select! {
            _ = tokio::time::sleep(intervalo) => {}
            _ = parar.cancelled() => break,
        }
    }
}

async fn loop_etl_eficiencia(
    db: sqlx::PgPool,
    intervalo: Duration,
    escrita: bool,
    parar: CancellationToken,
) {
    // Dorme primeiro: a primeira rodada é 1 hora (ou o intervalo configurado) depois do boot
    loop {
        tokio::select! {
            _ = tokio::time::sleep(intervalo) => {}
            _ = parar.cancelled() => break,
        }

        match jobs::etl_eficiencia::ciclo(&db, escrita).await {
            Ok(n) => info!(upsertados = n, "etl_eficiencia: ciclo ok"),
            Err(e) => warn!(erro = %e, "etl_eficiencia: ciclo falhou"),
        }
    }
}

async fn iniciar_servidor(
    porta: u16,
    db: sqlx::PgPool,
    handle: metrics_exporter_prometheus::PrometheusHandle,
    parar: CancellationToken,
) {
    let app = Router::new()
        .route("/health/ready", get(move || health(db)))
        .route("/metrics", get(move || async move { handle.render() }));

    let addr = SocketAddr::new(Ipv4Addr::UNSPECIFIED.into(), porta);
    let listener = match TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            error!(erro = %e, "não consegui abrir a porta de métricas");
            return;
        }
    };

    info!(porta, "servidor de métricas ouvindo");

    axum::serve(listener, app)
        .with_graceful_shutdown(async move { parar.cancelled().await })
        .await
        .ok();
}

async fn health(db: sqlx::PgPool) -> StatusCode {
    // Pronto quando consegue pingar o banco
    match sqlx::query("SELECT 1").execute(&db).await {
        Ok(_) => StatusCode::OK,
        Err(_) => StatusCode::SERVICE_UNAVAILABLE,
    }
}

async fn healthcheck(cfg: &Configuracao) -> anyhow::Result<()> {
    // Usado pelo Docker healthcheck: faz GET em /health/ready e sai com 0 ou 1
    let url = format!("http://127.0.0.1:{}/health/ready", cfg.porta_metricas);
    let status = std::process::Command::new("wget")
        .args(["-qO-", "--timeout=5", &url])
        .status()
        .context("wget não disponível")?;

    if status.success() {
        Ok(())
    } else {
        bail!("worker não está pronto ({url})")
    }
}

fn iniciar_logs(formato: configuracao::FormatoLog) {
    use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};
    const FILTRO: &str = "info,sqlx=warn";
    let filtro = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(FILTRO));
    let registro = tracing_subscriber::registry().with(filtro);
    match formato {
        configuracao::FormatoLog::Json => registro.with(fmt::layer().json()).try_init().ok(),
        configuracao::FormatoLog::Texto => registro.with(fmt::layer()).try_init().ok(),
    };
}
