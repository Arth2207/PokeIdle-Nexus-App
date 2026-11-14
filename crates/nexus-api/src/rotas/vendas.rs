//! `POST /api/bolsa/vendas` `{lidoEm, vendas: [...]}`: vendas do mercado coletadas pelo app.
//! O servidor tira vendedor, comprador e apelido (se vierem) e repassa ao Dossiê com a chave
//! dele; o app não precisa conhecer a chave do Dossiê.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use contrato::Erro;
use serde_json::{json, Value};

use crate::bolsa::agora_ms;
use crate::dossie::MAX_VENDAS;
use crate::estado::Estado;

fn erro(status: StatusCode, msg: &str) -> Response {
    (status, Json(Erro::novo(msg))).into_response()
}

/// Copia a venda sem dados de pessoas.
fn anonima(v: &Value) -> Option<Value> {
    let mut o = v.as_object()?.clone();
    o.remove("vendedor");
    o.remove("comprador");
    if let Some(Value::Object(f)) = o.get_mut("ficha") {
        f.remove("apelido");
    }
    Some(Value::Object(o))
}

/// Corpo que vai ao Dossiê: no máximo [`MAX_VENDAS`], sem dados de pessoas.
pub fn corpo_dossie(corpo: &Value, agora: i64) -> Option<Value> {
    let vendas: Vec<Value> = corpo
        .get("vendas")?
        .as_array()?
        .iter()
        .take(MAX_VENDAS)
        .filter_map(anonima)
        .collect();
    let lido = corpo
        .get("lidoEm")
        .and_then(Value::as_i64)
        .filter(|x| *x > 0)
        .unwrap_or(agora);
    Some(json!({ "lidoEm": lido, "vendas": vendas }))
}

pub async fn gravar(State(estado): State<Estado>, Json(corpo): Json<Value>) -> Response {
    let Some(ponte) = estado.dossie.as_ref() else {
        return erro(
            StatusCode::SERVICE_UNAVAILABLE,
            "Dossiê não configurado neste servidor (NEXUS_DOSSIE_URL)",
        );
    };
    let Some(corpo) = corpo_dossie(&corpo, agora_ms()) else {
        return erro(StatusCode::BAD_REQUEST, "falta vendas[]");
    };
    match ponte.vendas(&corpo).await {
        Ok(r) => {
            let status = StatusCode::from_u16(r.status).unwrap_or(StatusCode::BAD_GATEWAY);
            (status, Json(r.corpo)).into_response()
        }
        Err(e) => {
            tracing::warn!(erro = %e, "dossiê indisponível");
            erro(StatusCode::BAD_GATEWAY, "Dossiê indisponível")
        }
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn tira_pessoas_e_limita() {
        let c = json!({"lidoEm": 5, "vendas": [
            {"id": 1, "vendedor": "a", "comprador": "b", "ficha": {"apelido": "x", "level": 3}},
            "lixo"
        ]});
        let r = corpo_dossie(&c, 9).unwrap_or_default();
        assert_eq!(
            r,
            json!({"lidoEm": 5, "vendas": [{"id": 1, "ficha": {"level": 3}}]})
        );
        assert_eq!(corpo_dossie(&json!({}), 9), None);
        let sem_lido = corpo_dossie(&json!({"vendas": []}), 9).unwrap_or_default();
        assert_eq!(sem_lido["lidoEm"], 9);
    }
}
