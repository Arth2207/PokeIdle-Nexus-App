//! nexus-api: servidor HTTP e WebSocket do Nexus.

mod configuracao;
mod desligamento;
mod estado;
mod rotas;
mod telemetria;

use std::net::SocketAddr;

use anyhow::Context;
use tokio::net::TcpListener;

use crate::configuracao::Configuracao;
use crate::estado::Estado;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Em desenvolvimento lê o arquivo .env. Em produção ele não existe, e tudo bem.
    dotenvy::dotenv().ok();

    let cfg = Configuracao::carregar().context("configuração inválida")?;
    telemetria::iniciar(cfg.log_formato).context("não consegui iniciar os logs")?;
    tracing::info!(?cfg, "configuração carregada");

    let estado = Estado::novo(&cfg).context("NEXUS_DATABASE_URL inválida")?;
    let app = rotas::router(estado);

    let endereco = SocketAddr::new(cfg.endereco, cfg.porta);
    let listener = TcpListener::bind(endereco)
        .await
        .with_context(|| format!("não consegui abrir {endereco}"))?;
    tracing::info!(%endereco, "nexus-api ouvindo");

    axum::serve(listener, app)
        .with_graceful_shutdown(desligamento::sinal_de_parada())
        .await
        .context("servidor HTTP falhou")?;

    tracing::info!("nexus-api encerrado");
    Ok(())
}
