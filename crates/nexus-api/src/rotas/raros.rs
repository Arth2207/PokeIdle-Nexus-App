//! Rotas de raros e shiny: `GET /api/raros2`, `GET /api/shiny`, `POST /api/raros/ingest` e
//! `POST /api/shiny/registrar`. Mesmo contrato do `raros.js` do legado.
//!
//! As duas de escrita só gravam com `NEXUS_ESCRITA_RAROS=true`; desligada, o estado vem da
//! importação (`nexus-api importar raros`). O `fato_cofre` continua sendo do legado.

use std::collections::{HashMap, HashSet};

use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use contrato::ws::{MensagemServidor, RaroNovo, ShinyNovo};
use contrato::Erro;
use nucleo::js;
use nucleo::raros as nr;
use serde_json::{json, Value};

use crate::auth::Cliente;
use crate::bolsa::agora_ms;
use crate::estado::Estado;
use crate::raros::{self, ErroRaros};
use crate::rotas::corpo_json;
use crate::ws::Destino;

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

/// Identificadores (`id` em JSON) dos itens do estado de raros.
pub(crate) fn ids_dos_itens(estado: &Value) -> HashSet<String> {
    itens(estado)
        .filter_map(|i| i.get("id"))
        .map(Value::to_string)
        .collect()
}

fn itens(estado: &Value) -> impl Iterator<Item = &Value> {
    estado
        .pointer("/raros/itens")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
}

/// Itens cujo `id` não estava no estado anterior. Estado anterior vazio (primeira carga) não
/// gera novidade: seria uma enxurrada de avisos sem sentido.
pub(crate) fn itens_novos(antes: &HashSet<String>, depois: &Value) -> Vec<Value> {
    if antes.is_empty() {
        return Vec::new();
    }
    itens(depois)
        .filter(|i| {
            i.get("id")
                .is_some_and(|id| !antes.contains(&id.to_string()))
        })
        .cloned()
        .collect()
}

/// No máximo de mensagens `raro.novo` por ingestão.
const MAX_AVISOS_RAROS: usize = 20;

/// `raro.novo` por conta dona do item (item sem conta vai só para tokens globais).
fn publicar_raros(estado: &Estado, novos: &[Value]) {
    let mut enviados = 0;
    for item in novos {
        let id = item.get("id").cloned().unwrap_or(Value::Null);
        let nome = item.get("nome").cloned().unwrap_or(Value::Null);
        let donos: Vec<&String> = item
            .get("contas")
            .and_then(Value::as_object)
            .map(|c| c.keys().collect())
            .unwrap_or_default();
        let destinos: Vec<Option<String>> = if donos.is_empty() {
            vec![None]
        } else {
            donos.into_iter().map(|c| Some(c.to_lowercase())).collect()
        };
        for conta in destinos {
            if enviados >= MAX_AVISOS_RAROS {
                return;
            }
            enviados += 1;
            if let Some(w) = &estado.webhook {
                w.enviar(&format!("Raro novo: {}", nome.as_str().unwrap_or("?")));
            }
            let destino = conta.clone().map_or(Destino::Global, Destino::Conta);
            estado.ws.publicar_para(
                destino,
                MensagemServidor::RaroNovo(RaroNovo {
                    id: id.clone(),
                    nome: nome.clone(),
                    conta,
                }),
            );
        }
    }
}

/// `shiny.novo` quando o registro soma shiny.
fn publicar_shiny(estado: &Estado, corpo: &Value, resposta: &Value) {
    let quantidade = js::mais(corpo.get("shiny"));
    if quantidade.is_nan() || quantidade <= 0.0 {
        return;
    }
    let (Some(nick), Some(dia)) = (
        resposta.get("nick").and_then(Value::as_str),
        resposta.get("dia").and_then(Value::as_str),
    ) else {
        return;
    };
    let total = resposta
        .get("shiny")
        .and_then(Value::as_f64)
        .unwrap_or(quantidade);
    if let Some(w) = &estado.webhook {
        w.enviar(&format!(
            "Shiny registrado: +{quantidade} (total do dia: {total})"
        ));
    }
    estado.ws.publicar_para(
        Destino::Conta(nick.to_owned()),
        MensagemServidor::ShinyNovo(ShinyNovo {
            nick: nick.to_owned(),
            dia: dia.to_owned(),
            quantidade,
            total_dia: total,
        }),
    );
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
    let r = persistencia::raros::modificar(&estado.db, |r| {
        let antes = ids_dos_itens(r);
        let n = nr::ingerir(r, &corpo, agora)?;
        Ok((n, itens_novos(&antes, r)))
    })
    .await;
    match r {
        Ok(Ok((n, novos))) => {
            if estado.melhorias {
                publicar_raros(&estado, &novos);
            }
            Json(json!({ "ok": true, "itens": n })).into_response()
        }
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
        Ok(Ok(v)) => {
            if estado.melhorias {
                publicar_shiny(&estado, &corpo, &v);
            }
            Json(v).into_response()
        }
        Ok(Err(msg)) => erro(400, msg),
        Err(e) => falha(&e.into()),
    }
}
