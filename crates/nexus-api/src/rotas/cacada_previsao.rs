use crate::{auth::Cliente, estado::Estado};
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Extension, Json,
};
use serde_json::json;
use std::collections::HashMap;

pub async fn prever(
    State(e): State<Estado>,
    Extension(c): Extension<Cliente>,
    Query(p): Query<HashMap<String, String>>,
) -> Response {
    let conta = p
        .get("conta")
        .map(String::as_str)
        .or(c.conta.as_deref())
        .unwrap_or("");
    if conta.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"erro":"informe a conta"})),
        )
            .into_response();
    }
    if c.nick_permitido(Some(conta)).is_err() {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({"erro":"este token só acessa a própria conta"})),
        )
            .into_response();
    }
    let entradas =
        match persistencia::cacada_contexto::entradas(&e.db, conta, crate::bolsa::agora_ms()).await
        {
            Ok(Some(v)) => v,
            Ok(None) => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(json!({"erro":"aguardando dados recentes da conta"})),
                )
                    .into_response()
            }
            Err(er) => {
                tracing::warn!(erro=%er,"previsão de caçada indisponível");
                return (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(json!({"erro":"previsão indisponível"})),
                )
                    .into_response();
            }
        };
    match tokio::task::spawn_blocking(move || entradas.calcular(None)).await {
        Ok(Ok(v)) => Json(v).into_response(),
        Ok(Err(e)) => (StatusCode::BAD_REQUEST, Json(json!({"erro":e}))).into_response(),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"erro":"erro no cálculo"})),
        )
            .into_response(),
    }
}
