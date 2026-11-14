//! Núcleo do app desktop A2 (substitui o relay Node `holodex-relay.js`).
//!
//! Sem Tauri e sem webview: roda e é testado na VPS. A casca em `apps/desktop` só plugua o
//! [`Executor`] (que age no jogo) e chama [`cliente::rodar`]. Protocolo em `docs/websocket.md`.

pub mod backoff;
pub mod cliente;
pub mod comandos;
pub mod eventos;
pub mod jogo;
pub mod relay;
pub mod sessao;
pub mod token;

pub use comandos::{Despachante, Executor, Modo};
pub use contrato::ws::ComandoNovo;
pub use token::{ArmazemToken, ArquivoToken};

/// Erros do núcleo.
#[derive(Debug)]
pub enum Erro {
    /// Não há token salvo: o app precisa pedir ao usuário.
    SemToken,
    /// Falha ao ler ou gravar o token.
    Token(String),
    /// URL ou cabeçalho inválido.
    Config(String),
}

impl std::fmt::Display for Erro {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Erro::SemToken => write!(f, "token ausente"),
            Erro::Token(m) => write!(f, "token: {m}"),
            Erro::Config(m) => write!(f, "config: {m}"),
        }
    }
}

impl std::error::Error for Erro {}
