//! Bolsa no `nexus-api`: snapshot do mercado, consultas que viram respostas JSON e o
//! comparador com o sistema legado.

pub mod consultas;
pub mod paridade;
pub mod servico;

pub use servico::ServicoBolsa;

use std::time::{SystemTime, UNIX_EPOCH};

/// Agora em milissegundos desde a época Unix (o formato do legado).
pub fn agora_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}
