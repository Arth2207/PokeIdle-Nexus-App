//! Rotas de raros e shiny: `GET /api/raros2`, `GET /api/shiny`, `POST /api/raros/ingest` e
//! `POST /api/shiny/registrar`. Mesmo contrato do `raros.js` do legado.
//!
//! As duas de escrita só gravam com `NEXUS_ESCRITA_RAROS=true`; desligada, o estado vem da
//! importação (`nexus-api importar raros`). O `fato_cofre` continua sendo do legado.

use std::collections::HashMap;

use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use contrato::Erro;
use nucleo::js;
use nucleo::raros as nr;
use serde_json::{json, Value};

use crate::auth::Cliente;
use crate::bolsa::agora_ms;
use crate::estado::Estado;
use crate::raros::{self, ErroRaros};
use crate::rotas::corpo_json;

type Params = Query<HashMap<String, String>>;

fn proibido() -> Response {
    erro(403, "este token só acessa a própria conta")
}

fn erro(status: u16, msg: &str) -> Response {
    let s = StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (s, Json(Erro::novo(msg))).into_response()
}

fn falha(e: &ErroRaros) -> Response {
    tracing::warn!(erro = %e, "raros indisponível");
    match e {
        ErroRaros::Calculo => erro(500, "falha no cálculo"),
        _ => erro(503, "dados de raros indisponíveis agora"),
    }
}

fn desligada(estado: &Estado) -> Option<Response> {
    (!estado.escrita_raros).then(|| {
        erro(
            503,
            "escrita de raros desligada neste servidor (NEXUS_ESCRITA_RAROS)",
        )
    })
}

/// O nick que o token pode consultar: o da conta dele, ou o pedido.
fn nick_de(cliente: Option<&Extension<Cliente>>, pedido: &str) -> Result<String, ()> {
    let pedido = pedido.to_lowercase();
    match cliente {
        Some(Extension(c)) => c
            .nick_permitido(Some(&pedido))
            .map(|n| n.unwrap_or_default().to_lowercase())
            .map_err(|_| ()),
        None => Ok(pedido),
    }
}

/// `GET /api/raros2?nick=&notaMin=&meta=`.
pub async fn raros2(
    State(estado): State<Estado>,
    cliente: Option<Extension<Cliente>>,
    Query(p): Params,
) -> Response {
    let nick = match nick_de(cliente.as_ref(), p.get("nick").map_or("", String::as_str)) {
        Ok(n) => n,
        Err(()) => return proibido(),
    };
    // `Math.max(0, Math.min(10, +x || 3.5))`.
    let nota_min = match p
        .get("notaMin")
        .map(|v| js::mais(Some(&Value::from(v.as_str()))))
    {
        Some(n) if !n.is_nan() && n != 0.0 => n,
        _ => nr::NOTA_MIN_PADRAO,
    }
    .clamp(0.0, 10.0);
    let meta = p
        .get("meta")
        .map(|v| js::mais(Some(&Value::from(v.as_str()))))
        .filter(|m| !m.is_nan())
        .unwrap_or(0.0);
    let restrito = cliente.as_ref().and_then(|Extension(c)| c.conta.as_deref());
    match raros::resposta_raros2(&estado, &nick, nota_min, meta, restrito).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => falha(&e),
    }
}

/// `GET /api/shiny?nick=`.
pub async fn shiny(
    State(estado): State<Estado>,
    cliente: Option<Extension<Cliente>>,
    Query(p): Params,
) -> Response {
    let nick = match nick_de(cliente.as_ref(), p.get("nick").map_or("", String::as_str)) {
        Ok(n) => n,
        Err(()) => return proibido(),
    };
    match raros::resposta_shiny(&estado, &nick).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => falha(&e),
    }
}

/// `POST /api/raros/ingest` `{raros: {itens: []...}, historico?}` → `{ok, itens}`.
/// Token preso a uma conta não grava aqui: o estado é de todas as contas.
pub async fn ingest(
    State(estado): State<Estado>,
    cliente: Option<Extension<Cliente>>,
    corpo: Bytes,
) -> Response {
    if let Some(r) = desligada(&estado) {
        return r;
    }
    if cliente
        .as_ref()
        .is_some_and(|Extension(c)| c.conta.is_some())
    {
        return erro(403, "token de conta não grava o estado global de raros");
    }
    let corpo = corpo_json(&corpo);
    let agora = agora_ms();
    match persistencia::raros::modificar(&estado.db, |r| nr::ingerir(r, &corpo, agora)).await {
        Ok(Ok(n)) => Json(json!({ "ok": true, "itens": n })).into_response(),
        Ok(Err(msg)) => erro(400, msg),
        Err(e) => falha(&e.into()),
    }
}

/// `POST /api/shiny/registrar` `{nick, dia?, shiny?, bolas?}`.
pub async fn registrar(
    State(estado): State<Estado>,
    cliente: Option<Extension<Cliente>>,
    corpo: Bytes,
) -> Response {
    if let Some(r) = desligada(&estado) {
        return r;
    }
    let corpo = corpo_json(&corpo);
    if let Some(Extension(c)) = cliente.as_ref() {
        let pedido = corpo
            .get("nick")
            .map(|n| js::texto(Some(n)))
            .unwrap_or_default();
        if c.nick_permitido(Some(&pedido.to_lowercase())).is_err() {
            return erro(403, "este token só acessa a própria conta");
        }
    }
    let hoje = js::data_do_dia(agora_ms().div_euclid(86_400_000));
    match persistencia::raros::modificar(&estado.db, |r| nr::registrar_shiny(r, &corpo, &hoje))
        .await
    {
        Ok(Ok(v)) => Json(v).into_response(),
        Ok(Err(msg)) => erro(400, msg),
        Err(e) => falha(&e.into()),
    }
}
