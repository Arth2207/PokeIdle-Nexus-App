//! Encerramento gracioso.

/// Termina quando chega Ctrl+C ou SIGTERM (é o que o Docker manda para parar o container).
pub async fn sinal_de_parada() {
    let ctrl_c = async {
        // Se não der para registrar o handler, só não reagimos a Ctrl+C; o SIGTERM continua valendo.
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let sigterm = async {
        use tokio::signal::unix::{signal, SignalKind};
        match signal(SignalKind::terminate()) {
            Ok(mut sinal) => {
                sinal.recv().await;
            }
            Err(erro) => {
                tracing::error!(%erro, "não consegui escutar SIGTERM");
                std::future::pending::<()>().await;
            }
        }
    };

    #[cfg(not(unix))]
    let sigterm = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = sigterm => {},
    }
    tracing::info!("sinal de parada recebido, terminando requisições em andamento");
}
