//! `GET /api/eventos` (`NEXUS_MELHORIAS`): histórico de eventos gravados pela ingestão.
//!
//! Sem `agrupar`, lê `nexus.evento` (mais recentes primeiro, com limite de linhas). Com
//! `agrupar=hora`, lê o agregado contínuo `nexus.evento_hora` (contagem por hora, conta e
//! tipo). Token preso a uma conta só enxerga a própria.

use std::collections::HashMap;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use contrato::Erro;
use persistencia::eventos::{self, normalizar, FiltroHistorico};
use serde_json::json;

use crate::auth::Cliente;
use crate::estado::Estado;

/// Linhas devolvidas por padrão e no máximo.
const LIMITE_PADRAO: i64 = 100;
const LIMITE_MAXIMO: i64 = 1000;

fn erro(status: u16, msg: &str) -> Response {
    let s = StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (s, Json(Erro::novo(msg))).into_response()
}

/// Instante em ms; `Err` se o texto não é número.
fn instante(p: &HashMap<String, String>, campo: &str) -> Result<Option<i64>, ()> {
    match p.get(campo).filter(|v| !v.is_empty()) {
        None => Ok(None),
        Some(v) => v.parse::<i64>().map(Some).map_err(|_| ()),
    }
}

pub async fn historico(
    State(estado): State<Estado>,
    cliente: Option<Extension<Cliente>>,
    Query(p): Query<HashMap<String, String>>,
) -> Response {
    if !estado.melhorias {
        return erro(404, "rota disponível só com NEXUS_MELHORIAS");
    }
    let pedida = p
        .get("conta")
        .filter(|c| !c.is_empty())
        .map(|c| normalizar(c));
    let propria = cliente
        .as_ref()
        .and_then(|Extension(c)| c.conta.as_deref())
        .map(normalizar);
    // Token de conta: só a própria; pedir outra é 403.
    let conta = match (propria, pedida) {
        (Some(prop), Some(ped)) if prop != ped => {
            return erro(403, "este token só acessa a própria conta");
        }
        (Some(prop), _) => Some(prop),
        (None, ped) => ped,
    };
    let (Ok(de_ms), Ok(ate_ms)) = (instante(&p, "de"), instante(&p, "ate")) else {
        return erro(400, "de/ate devem ser ms desde 1970");
    };
    let agrupar = match p.get("agrupar").map(String::as_str) {
        None | Some("") => false,
        Some("hora") => true,
        Some(_) => return erro(400, "agrupar aceita só 'hora'"),
    };
    let limite = p
        .get("limite")
        .and_then(|l| l.parse::<i64>().ok())
        .unwrap_or(LIMITE_PADRAO)
        .clamp(1, LIMITE_MAXIMO);
    let f = FiltroHistorico {
        conta: conta.as_deref(),
        tipo: p.get("tipo").map(String::as_str).filter(|t| !t.is_empty()),
        de_ms,
        ate_ms,
    };
    let r = if agrupar {
        eventos::por_hora(&estado.db, &f, limite).await.map(|v| {
            json!({ "agrupar": "hora", "horas": v.into_iter().map(|(h, conta, tipo, total)| {
                json!({ "hora": h, "conta": conta, "tipo": tipo, "total": total })
            }).collect::<Vec<_>>() })
        })
    } else {
        eventos::listar(&estado.db, &f, limite).await.map(|v| {
            json!({ "eventos": v.into_iter().map(|e| {
                json!({ "conta": e.conta, "tipo": e.tipo, "em": e.em_ms, "dados": e.dados })
            }).collect::<Vec<_>>() })
        })
    };
    match r {
        Ok(v) => Json(v).into_response(),
        Err(e) => {
            tracing::warn!(erro = %e, "histórico de eventos indisponível");
            erro(503, "banco indisponível")
        }
    }
}
