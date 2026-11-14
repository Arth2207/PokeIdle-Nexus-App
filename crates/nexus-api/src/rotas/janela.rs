//! `POST /api/motor/janela` `{conta, janela}`: a janela da conta montada pelo app (Fase 4).
//! O motor do Nexus lê daqui quando roda sem o motor do legado (`NEXUS_MOTOR_SOMBRA_URL=nexus`).

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use contrato::Erro;
use serde_json::{json, Value};

use crate::auth::Cliente;
use crate::estado::Estado;

fn erro(status: StatusCode, msg: &str) -> Response {
    (status, Json(Erro::novo(msg))).into_response()
}

pub async fn gravar(
    State(estado): State<Estado>,
    cliente: Option<Extension<Cliente>>,
    Json(corpo): Json<Value>,
) -> Response {
    if !estado.escrita_ingestao {
        return erro(
            StatusCode::SERVICE_UNAVAILABLE,
            "ingestão desligada neste servidor (NEXUS_ESCRITA_INGESTAO)",
        );
    }
    let (Some(conta), Some(janela)) = (
        corpo
            .get("conta")
            .and_then(Value::as_str)
            .map(str::to_lowercase),
        corpo.get("janela").filter(|j| j.get("nick").is_some()),
    ) else {
        return erro(StatusCode::BAD_REQUEST, "falta conta ou janela.nick");
    };
    let presa = cliente.as_ref().and_then(|Extension(c)| c.conta.as_deref());
    if presa.is_some_and(|p| !p.eq_ignore_ascii_case(&conta)) {
        return erro(StatusCode::FORBIDDEN, "este token só grava a própria conta");
    }
    match persistencia::motor::gravar_janela(&estado.db, &conta, janela).await {
        Ok(()) => Json(json!({"ok": true})).into_response(),
        Err(e) => {
            tracing::warn!(erro = %e, "janela do motor");
            erro(StatusCode::SERVICE_UNAVAILABLE, "banco indisponível")
        }
    }
}
