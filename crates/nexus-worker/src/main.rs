//! nexus-worker: scheduler de jobs do Nexus (ETL, eficiência, dossiê).
//!
//! Uso:
//!   nexus-worker              sobe o worker
//!   nexus-worker healthcheck  checa o /health/ready local (usado pelo Docker)
//!   nexus-worker etl-relatorio [--desde <horas>]
//!   nexus-worker dossie-paridade            comparador de rotas do dossiê (laço)
//!   nexus-worker dossie-servir              só o servidor do dossiê (testes locais)
//!   nexus-worker dossie-relatorio [--desde <horas>]

mod configuracao;
mod dossie;
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

const USO: &str = "uso: nexus-worker [healthcheck | etl-relatorio [--desde <horas>] | dossie-paridade | dossie-relatorio [--desde <horas>]]";

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
        ["etl-relatorio"] => relatorio(&cfg, 24).await,
        ["etl-relatorio", "--desde", h] => {
            let horas: u32 = h.parse().context("--desde requer número inteiro")?;
            relatorio(&cfg, horas).await
        }
        ["dossie-paridade"] => dossie_paridade(&cfg).await,
        ["dossie-servir"] => dossie_servir(&cfg).await,
        ["dossie-relatorio"] => dossie_relatorio(&cfg, 24).await,
        ["dossie-relatorio", "--desde", h] => {
            let horas: u32 = h.parse().context("--desde requer número inteiro")?;
            dossie_relatorio(&cfg, horas).await
        }
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
    let paridade = loop_etl_paridade(
        db.clone(),
        cfg.etl_efic_intervalo,
        cfg.escrita_etl,
        parar.clone(),
    );

    if cfg.dossie_porta > 0 {
        let d = montar_dossie(&cfg, db.clone())?;
        tokio::spawn(dossie::servir(cfg.dossie_porta, d, parar.clone(), false));
    }

    tokio::join!(etl, efic, paridade);

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

async fn loop_etl_paridade(
    db: sqlx::PgPool,
    intervalo: Duration,
    escrita: bool,
    parar: CancellationToken,
) {
    // Depois do cutover o Nexus é a fonte do `public`: comparar com a sombra não diz nada.
    if escrita {
        info!("etl_paridade: desligado (NEXUS_ESCRITA_ETL=true, o Nexus é a fonte)");
        return;
    }
    loop {
        tokio::select! {
            _ = tokio::time::sleep(intervalo) => {}
            _ = parar.cancelled() => break,
        }
        match jobs::etl_paridade::ciclo(&db).await {
            Ok(n) => {
                metrics::gauge!("nexus_worker_paridade_ultima_ok").set(1.0);
                info!(comparacoes = n, "etl_paridade: ciclo ok");
            }
            Err(e) => {
                metrics::gauge!("nexus_worker_paridade_ultima_ok").set(0.0);
                warn!(erro = %e, "etl_paridade: ciclo falhou");
            }
        }
    }
}

async fn relatorio(cfg: &Configuracao, horas: u32) -> anyhow::Result<()> {
    iniciar_logs(cfg.log_formato);
    let db = PgPoolOptions::new()
        .max_connections(1)
        .connect(&cfg.database_url)
        .await
        .context("não consegui conectar ao banco")?;
    let (linhas, total, divergentes) = jobs::etl_paridade::relatorio(&db, horas).await?;
    if linhas.is_empty() {
        println!("sem dados nas últimas {} horas", horas);
        return Ok(());
    }
    println!(
        "{:<20} {:<9} {:<4} {:<17} {:>16} igual / diferenças",
        "tabela", "conta", "jan", "fim (UTC)", "linhas node/nexus"
    );
    for l in &linhas {
        let linhas_n = l.node.first().copied().unwrap_or(0);
        let linhas_x = l.nexus.first().copied().unwrap_or(0);
        println!(
            "{:<20} {:<9} {:<4} {:<17} {:>16} {}",
            l.tabela,
            l.conta_id,
            l.janela,
            l.fim.format("%Y-%m-%d %H:%M"),
            format!("{linhas_n}/{linhas_x}"),
            if l.igual {
                "✓".to_owned()
            } else {
                format!("✗ {}", l.diferencas())
            }
        );
    }
    println!(
        "janelas de 1h nas últimas {}h: {} divergentes de {}",
        horas, divergentes, total
    );
    Ok(())
}

async fn conectar(cfg: &Configuracao) -> anyhow::Result<sqlx::PgPool> {
    PgPoolOptions::new()
        .max_connections(2)
        .connect(&cfg.database_url)
        .await
        .context("não consegui conectar ao banco")
}

fn montar_dossie(
    cfg: &Configuracao,
    db: sqlx::PgPool,
) -> anyhow::Result<std::sync::Arc<dossie::Dossie>> {
    // Chave ilegível não derruba o worker (o ETL roda no mesmo processo): segue sem chave,
    // e as rotas protegidas respondem 401.
    let chave = cfg.dossie_chave_arquivo.as_ref().and_then(|arq| {
        std::fs::read_to_string(arq)
            .map(|c| c.trim().to_owned())
            .map_err(|e| warn!(erro = %e, arquivo = %arq, "dossiê: não consegui ler a chave"))
            .ok()
    });
    Ok(std::sync::Arc::new(dossie::Dossie {
        db,
        static_dir: cfg.dossie_static.clone().into(),
        dados_dir: cfg.dossie_dados.clone().into(),
        chave: chave.filter(|c| !c.is_empty()),
        limites: dossie::Limites::default(),
        precos: Default::default(),
        escrita: cfg.escrita_dossie,
    }))
}

async fn dossie_servir(cfg: &Configuracao) -> anyhow::Result<()> {
    iniciar_logs(cfg.log_formato);
    let db = conectar(cfg).await?;
    let d = montar_dossie(cfg, db)?;
    info!(escrita = d.escrita, "dossiê (serviço próprio) iniciando");
    let parar = CancellationToken::new();
    {
        // SIGTERM (docker stop) ou Ctrl-C: termina as requisições em andamento e sai.
        let parar = parar.clone();
        tokio::spawn(async move {
            let mut term =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).ok();
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                _ = async { match term.as_mut() { Some(t) => { t.recv().await; } None => std::future::pending().await } } => {}
            }
            parar.cancel();
        });
    }
    if cfg.dossie_porta_privada > 0 {
        tokio::spawn(dossie::servir(
            cfg.dossie_porta_privada,
            std::sync::Arc::clone(&d),
            parar.clone(),
            true,
        ));
    }
    dossie::servir(cfg.dossie_porta.max(1), d, parar, false).await;
    Ok(())
}

async fn dossie_paridade(cfg: &Configuracao) -> anyhow::Result<()> {
    iniciar_logs(cfg.log_formato);
    let db = conectar(cfg).await?;
    info!(
        legado = %cfg.dossie_legado_url,
        nexus = %cfg.dossie_nexus_url,
        "comparador do dossiê iniciado"
    );
    let chave = match &cfg.dossie_chave_arquivo {
        Some(arq) => Some(
            std::fs::read_to_string(arq)
                .with_context(|| format!("não consegui ler NEXUS_DOSSIE_CHAVE_FILE={arq}"))?
                .trim()
                .to_owned(),
        )
        .filter(|c| !c.is_empty()),
        None => None,
    };
    let alvos = dossie::paridade::Alvos {
        legado: cfg.dossie_legado_url.clone(),
        legado_privado: cfg.dossie_legado_privado_url.clone(),
        nexus: cfg.dossie_nexus_url.clone(),
        chave,
    };
    dossie::paridade::rodar(db, alvos, cfg.dossie_paridade_ciclo).await
}

async fn dossie_relatorio(cfg: &Configuracao, horas: u32) -> anyhow::Result<()> {
    iniciar_logs(cfg.log_formato);
    let db = conectar(cfg).await?;
    dossie::paridade::relatorio(&db, horas).await
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
    // Imagem distroless não tem wget: HTTP/1.0 cru pelo socket, como no nexus-api.
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let checagem = async {
        let mut conexao =
            tokio::net::TcpStream::connect((Ipv4Addr::LOCALHOST, cfg.porta_metricas)).await?;
        conexao
            .write_all(b"GET /health/ready HTTP/1.0\r\nHost: localhost\r\n\r\n")
            .await?;
        let mut inicio = [0u8; 12];
        conexao.read_exact(&mut inicio).await?;
        anyhow::Ok(inicio)
    };
    let inicio = tokio::time::timeout(Duration::from_secs(5), checagem)
        .await
        .context("healthcheck: sem resposta em 5s")??;
    if !inicio.ends_with(b" 200") {
        bail!(
            "healthcheck: resposta {:?}",
            String::from_utf8_lossy(&inicio)
        );
    }
    Ok(())
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
