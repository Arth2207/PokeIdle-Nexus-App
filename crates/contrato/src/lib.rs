//! Tipos que o servidor e o app desktop trocam.
//!
//! Regra do crate: só `serde`. Nada de axum, sqlx ou tokio aqui, porque este crate também
//! compila para WebAssembly (interface em Leptos). Se um campo mudar de forma incompatível,
//! o app deixa de compilar, em vez de quebrar em produção.

pub mod bolsa;
pub mod rotas;
pub mod ws;

use serde::{Deserialize, Serialize};

/// Corpo de erro das rotas da bolsa. Mesmo formato do sistema legado: `{ "erro": "..." }`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Erro {
    pub erro: String,
}

impl Erro {
    /// `impl Into<String>` aceita `&str` ou `String`, como uma função JS que aceita qualquer
    /// coisa "convertível em string". A conversão acontece uma vez, aqui dentro.
    pub fn novo(mensagem: impl Into<String>) -> Self {
        Self {
            erro: mensagem.into(),
        }
    }
}
