//! `POST /api/motor/janela` `{conta, janela}`: a janela da conta montada pelo app (Fase 4).
//! O motor do Nexus lê daqui quando roda sem o motor do legado (`NEXUS_MOTOR_SOMBRA_URL=nexus`).
//! `GET /api/contas`: as contas do app (rótulo, nick no jogo e a última janela), para o painel.
//! `GET /api/cacada?conta=`: sessão da conta por hora, hunts e drops (tela Caçada).
//! `GET /api/motor/decisoes`: decisões do motor (6 h) e o resultado dos comandos por tipo (24 h).

use axum::extract::{Query, State};
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

pub async fn contas(State(estado): State<Estado>) -> Response {
    match persistencia::motor::contas_do_app(&estado.db).await {
        Ok(c) => Json(json!({ "contas": c })).into_response(),
        Err(e) => {
            tracing::warn!(erro = %e, "contas do app");
            erro(StatusCode::SERVICE_UNAVAILABLE, "banco indisponível")
        }
    }
}

pub async fn cacada(
    State(estado): State<Estado>,
    Query(p): Query<std::collections::HashMap<String, String>>,
) -> Response {
    use persistencia::cacada as c;
    let rotulo = p.get("conta").map(String::as_str).unwrap_or_default();
    let sessao = match c::conta_id(rotulo) {
        Some(id) => c::sessao(&estado.db, id).await.map(Some),
        None => Ok(None),
    };
    let r = async {
        Ok::<_, sqlx::Error>(json!({
            "sessao": sessao?,
            "hunts": c::hunts(&estado.db).await?,
            "drops": c::drops(&estado.db, rotulo).await?,
        }))
    }
    .await;
    match r {
        Ok(v) => Json(v).into_response(),
        Err(e) => {
            tracing::warn!(erro = %e, "caçada");
            erro(StatusCode::SERVICE_UNAVAILABLE, "banco indisponível")
        }
    }
}

pub async fn decisoes(State(estado): State<Estado>) -> Response {
    use persistencia::motor as m;
    let r = async {
        Ok::<_, sqlx::Error>(json!({
            "decisoes": m::decisoes_recentes(&estado.db).await?,
            "regras": m::regras_resumo(&estado.db).await?,
        }))
    }
    .await;
    match r {
        Ok(v) => Json(v).into_response(),
        Err(e) => {
            tracing::warn!(erro = %e, "decisões do motor");
            erro(StatusCode::SERVICE_UNAVAILABLE, "banco indisponível")
        }
    }
}
