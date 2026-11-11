//! Rate limit por token (balde de fichas). O Nginx já limita por IP; aqui o limite é por
//! cliente, que só a aplicação conhece.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

use axum::extract::{Request, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;

use crate::auth::Cliente;
use crate::estado::Estado;

pub struct Limitador {
    por_segundo: f64,
    rajada: f64,
    baldes: Mutex<HashMap<String, (f64, Instant)>>,
}

impl Limitador {
    pub fn novo(por_segundo: f64, rajada: f64) -> Self {
        Self {
            por_segundo,
            rajada: rajada.max(1.0),
            baldes: Mutex::new(HashMap::new()),
        }
    }

    /// Gasta uma ficha do cliente. `false` se o balde estiver vazio.
    pub fn permitir(&self, cliente: &str) -> bool {
        let agora = Instant::now();
        // Um Mutex "envenenado" (uma thread entrou em pânico segurando o lock) não deve
        // derrubar a API: os dados do balde continuam válidos, então seguimos com eles.
        let mut baldes = self
            .baldes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (fichas, ultimo) = baldes
            .entry(cliente.to_owned())
            .or_insert((self.rajada, agora));
        let repostas = agora.duration_since(*ultimo).as_secs_f64() * self.por_segundo;
        *fichas = (*fichas + repostas).min(self.rajada);
        *ultimo = agora;
        if *fichas >= 1.0 {
            *fichas -= 1.0;
            true
        } else {
            false
        }
    }
}

/// Middleware. Roda depois do de token (precisa do `Cliente`).
pub async fn limitar(State(estado): State<Estado>, req: Request, next: Next) -> Response {
    let permitido = req
        .extensions()
        .get::<Cliente>()
        .is_none_or(|c| estado.limitador.permitir(&c.nome));
    if permitido {
        return next.run(req).await;
    }
    metrics::counter!("nexus_http_limitadas_total").increment(1);
    let mut r = (
        StatusCode::TOO_MANY_REQUESTS,
        Json(contrato::Erro::novo(
            "muitas requisições; tente de novo em instantes",
        )),
    )
        .into_response();
    r.headers_mut()
        .insert(header::RETRY_AFTER, HeaderValue::from_static("1"));
    r
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn balde_esvazia_e_separa_clientes() {
        let l = Limitador::novo(0.0, 2.0);
        assert!(l.permitir("a"));
        assert!(l.permitir("a"));
        assert!(!l.permitir("a"));
        assert!(l.permitir("b"));
    }
}
