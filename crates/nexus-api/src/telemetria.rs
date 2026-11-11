//! Logs estruturados com `tracing`.
//!
//! O nível vem de `RUST_LOG` (ex.: `RUST_LOG=debug`). Sem ela, usa um padrão razoável.

use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use crate::configuracao::FormatoLog;

const FILTRO_PADRAO: &str = "info,tower_http=info,sqlx=warn";

pub fn iniciar(formato: FormatoLog) -> Result<(), tracing_subscriber::util::TryInitError> {
    let filtro =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(FILTRO_PADRAO));
    let registro = tracing_subscriber::registry().with(filtro);

    match formato {
        FormatoLog::Json => registro.with(fmt::layer().json()).try_init(),
        FormatoLog::Texto => registro.with(fmt::layer()).try_init(),
    }
}
