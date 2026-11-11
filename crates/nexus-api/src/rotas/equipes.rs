//! Rotas `/api/equipes/*`. Mesmo contrato JSON do sistema legado.
//!
//! O cálculo (dezenas a milhares de duelos) roda em `spawn_blocking`, numa thread separada,
//! para não segurar as threads do tokio que atendem as outras requisições.

use std::collections::HashMap;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use contrato::Erro;
use nucleo::equipes;
use serde_json::Value;

use crate::bolsa::agora_ms;
use crate::equipes::ErroServico;
use crate::estado::Estado;

type Params = Query<HashMap<String, String>>;

fn indisponivel(e: &ErroServico) -> Response {
    tracing::warn!(erro = %e, "equipes indisponíveis");
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(Erro::novo(e.to_string())),
    )
        .into_response()
}

fn erro(status: u16, msg: &str) -> Response {
    let s = StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (s, Json(Erro::novo(msg))).into_response()
}

/// Busca motor e estado, ou devolve 503.
macro_rules! dados {
    ($estado:expr) => {{
        let motor = match $estado.equipes.motor().await {
            Ok(m) => m,
            Err(e) => return indisponivel(&e),
        };
        match $estado.equipes.estado().await {
            Ok(e) => (motor, e),
            Err(e) => return indisponivel(&e),
        }
    }};
}

pub async fn bosses(State(estado): State<Estado>, Query(p): Params) -> Response {
    let (motor, est) = dados!(estado);
    let r = tokio::task::spawn_blocking(move || {
        equipes::resposta_bosses(&motor, &est, p.get("nick").map(String::as_str))
    })
    .await;
    match r {
        Ok(v) => Json(v).into_response(),
        Err(_) => erro(500, "falha no cálculo"),
    }
}

pub async fn plano(State(estado): State<Estado>, Query(p): Params) -> Response {
    let (motor, est) = dados!(estado);
    let r = tokio::task::spawn_blocking(move || {
        let g = |k: &str| p.get(k).map(String::as_str);
        equipes::resposta_plano(&motor, &est, g("nick"), g("boss"), g("perfil"))
    })
    .await;
    match r {
        Ok(Ok(v)) => Json(v).into_response(),
        Ok(Err(e)) => erro(e.status, e.erro),
        Err(_) => erro(500, "falha no cálculo"),
    }
}

/// `POST /api/equipes/ingest`: só com `NEXUS_ESCRITA_EQUIPES=true`. Até o cutover, quem
/// recebe o relay é o legado, e o estado chega aqui pela importação.
pub async fn ingest(State(estado): State<Estado>, Json(corpo): Json<Value>) -> Response {
    if !estado.escrita_equipes {
        return erro(
            503,
            "escrita de equipes desligada neste servidor (NEXUS_ESCRITA_EQUIPES)",
        );
    }
    let _vez = estado.equipes.gravacao.lock().await;
    let atual = match estado.equipes.estado().await {
        Ok(e) => e,
        Err(e) => return indisponivel(&e),
    };
    let mut novo = (*atual).clone();
    match novo.ingerir(&corpo, agora_ms()) {
        Ok((nick, resumo)) => match estado.equipes.gravar(novo, &nick).await {
            Ok(()) => Json(resumo).into_response(),
            Err(e) => indisponivel(&e),
        },
        Err(msg) => erro(400, msg),
    }
}
