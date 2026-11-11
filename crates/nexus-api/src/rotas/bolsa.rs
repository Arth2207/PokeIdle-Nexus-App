//! Rotas `/api/bolsa/*`. Mesmo contrato JSON do sistema legado.

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use contrato::bolsa::{SerieParams, SerieResposta};
use contrato::Erro;
use nucleo::bolsa::mercado;
use nucleo::bolsa::ticker::TickerId;

use crate::estado::Estado;

/// `GET /api/bolsa/serie?id=<ticker>`: os últimos 5000 pontos (momento, preço) do ticker.
pub async fn serie(State(estado): State<Estado>, Query(params): Query<SerieParams>) -> Response {
    let mercado = match estado.bolsa.mercado().await {
        Ok(m) => m,
        Err(erro) => return indisponivel(&erro),
    };

    // Id ausente, mal formado ou de um ticker sem vendas suficientes: todos dão 404, como
    // no legado.
    let Some(texto) = params.id else {
        return nao_encontrado();
    };
    let Ok(id) = texto.parse::<TickerId>() else {
        return nao_encontrado();
    };
    let Some(pontos) = mercado::serie(&mercado, &id) else {
        return nao_encontrado();
    };

    Json(SerieResposta {
        vendas: pontos.collect(),
        id: texto,
    })
    .into_response()
}

fn nao_encontrado() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(Erro::novo("ticker não encontrado")),
    )
        .into_response()
}

fn indisponivel(erro: &dyn std::error::Error) -> Response {
    tracing::warn!(%erro, "não consegui montar o mercado");
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(Erro::novo("dados do mercado indisponíveis agora")),
    )
        .into_response()
}
