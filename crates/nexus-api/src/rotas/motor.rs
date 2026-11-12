//! Rotas `GET` do motor de automação (fase 4, etapa 3), no caminho e com o contrato do
//! `motor.js` do legado: `{ok, ...}` ou o corpo da rota, 404 `{erro: "conta"}`, 500
//! `{erro: "erro interno"}`.
//!
//! Token preso a uma conta: as rotas com `nick` só respondem sobre a própria conta, e as que
//! juntam todas as contas (financeiro, histórico, estado, backup...) respondem 403.

use std::collections::HashMap;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Extension, Json, Router};
use serde_json::json;

use crate::auth::Cliente;
use crate::bolsa::agora_ms;
use crate::estado::Estado;
use crate::motor::{self, Rota};

fn resposta(status: u16, corpo: serde_json::Value) -> Response {
    let s = StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (s, Json(corpo)).into_response()
}

async fn atender(
    rota: Rota,
    estado: Estado,
    cliente: Option<Extension<Cliente>>,
    p: HashMap<String, String>,
) -> Response {
    let pedido = p.get("nick").map(String::as_str);
    let restrito = cliente.as_ref().and_then(|Extension(c)| c.conta.as_deref());
    let nick = match (restrito, rota.por_conta()) {
        (None, _) => pedido.map(str::to_owned),
        (Some(_), false) => {
            return resposta(
                403,
                json!({ "erro": "este token só acessa a própria conta" }),
            )
        }
        (Some(c), true) => match pedido.filter(|n| !n.is_empty()) {
            None => Some(c.to_owned()),
            // O motor compara o nick exato: vale o que veio no pedido.
            Some(n) if n.eq_ignore_ascii_case(c) => Some(n.to_owned()),
            Some(_) => {
                return resposta(
                    403,
                    json!({ "erro": "este token só acessa a própria conta" }),
                )
            }
        },
    };
    match motor::responder(
        &estado.db,
        estado.motor_fuso_ms,
        agora_ms(),
        rota,
        nick.as_deref(),
    )
    .await
    {
        Ok(Ok(r)) => resposta(r.status, r.corpo),
        Ok(Err(e)) => {
            tracing::warn!(rota = rota.caminho(), erro = %e, "erro na rota do motor");
            resposta(500, json!({ "erro": "erro interno" }))
        }
        Err(e) => {
            tracing::warn!(rota = rota.caminho(), erro = %e, "estado do motor indisponível");
            resposta(503, json!({ "erro": "estado do motor indisponível" }))
        }
    }
}

/// As rotas `GET` do motor, para o grupo de leitura.
pub fn rotas() -> Router<Estado> {
    Rota::TODAS.into_iter().fold(Router::new(), |r, rota| {
        r.route(
            rota.caminho(),
            get(
                move |State(e): State<Estado>,
                      cliente: Option<Extension<Cliente>>,
                      Query(p): Query<HashMap<String, String>>| {
                    atender(rota, e, cliente, p)
                },
            ),
        )
    })
}
