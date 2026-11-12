//! Rotas da fila de comandos: `GET /api/comandos`, `POST /api/confirmar-comando` e
//! `POST /api/inserir-comando`. Mesmo contrato do legado. Só com `NEXUS_ESCRITA_COMANDOS`
//! (a primeira reserva comandos, então também é escrita).

use std::collections::HashMap;

use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use contrato::Erro;
use persistencia::comandos::{self as fila, NovoComando, LIMITE_PADRAO};
use serde_json::{json, Value};

use crate::auth::Cliente;
use crate::bolsa::agora_ms;
use crate::comandos::{self, MAX_RESULTADO};
use crate::estado::Estado;
use crate::rotas::corpo_json;

fn proibido() -> Response {
    erro(403, "este token só acessa a própria conta")
}

fn erro(status: u16, msg: &str) -> Response {
    let s = StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (s, Json(Erro::novo(msg))).into_response()
}

fn desligada(estado: &Estado) -> Option<Response> {
    (!estado.escrita_comandos).then(|| {
        erro(
            503,
            "fila de comandos desligada neste servidor (NEXUS_ESCRITA_COMANDOS)",
        )
    })
}

fn indisponivel(e: &sqlx::Error) -> Response {
    tracing::warn!(erro = %e, "fila de comandos indisponível");
    erro(503, "banco indisponível")
}

/// O nick (normalizado) que o token pode usar; `Err` se pedir outra conta.
fn nick_permitido(cliente: Option<&Extension<Cliente>>, pedido: &str) -> Result<String, ()> {
    let pedido = persistencia::eventos::normalizar(pedido);
    match cliente {
        Some(Extension(c)) => c
            .nick_permitido(Some(&pedido))
            .map(|n| n.unwrap_or(pedido))
            .map_err(|_| ()),
        None => Ok(pedido),
    }
}

/// `GET /api/comandos?nick=`: reserva até 10 comandos pendentes da conta.
pub async fn listar(
    State(estado): State<Estado>,
    cliente: Option<Extension<Cliente>>,
    Query(p): Query<HashMap<String, String>>,
) -> Response {
    if let Some(r) = desligada(&estado) {
        return r;
    }
    let nick = p.get("nick").map(String::as_str).unwrap_or_default();
    if nick.is_empty() {
        return erro(400, "preciso de ?nick=");
    }
    let nick = match nick_permitido(cliente.as_ref(), nick) {
        Ok(n) => n,
        Err(()) => return proibido(),
    };
    let por = cliente
        .as_ref()
        .map_or("http", |Extension(c)| c.nome.as_str());
    match comandos::entregar(&estado, std::slice::from_ref(&nick), por, LIMITE_PADRAO).await {
        Ok(v) => Json(json!({
            "nick": nick,
            "comandos": v.into_iter().map(|c| json!({
                "id": c.id, "cmd": c.tipo, "params": c.params, "origem": c.origem,
            })).collect::<Vec<_>>(),
        }))
        .into_response(),
        Err(e) => indisponivel(&e),
    }
}

/// `POST /api/confirmar-comando` `{id, resultado?}`. Confirmar de novo não muda nada.
pub async fn confirmar(
    State(estado): State<Estado>,
    cliente: Option<Extension<Cliente>>,
    corpo: Bytes,
) -> Response {
    if let Some(r) = desligada(&estado) {
        return r;
    }
    let corpo = corpo_json(&corpo);
    // `parseInt(id, 10)`: aceita número ou texto que comece com dígitos.
    let id = match corpo.get("id") {
        Some(Value::Number(n)) => n.as_f64().map(|f| f.trunc() as i64),
        Some(Value::String(s)) => {
            let t = s.trim_start();
            let fim = t
                .char_indices()
                .find(|(i, c)| !(c.is_ascii_digit() || (*i == 0 && (*c == '-' || *c == '+'))))
                .map_or(t.len(), |(i, _)| i);
            t.get(..fim).and_then(|d| d.parse::<i64>().ok())
        }
        _ => None,
    };
    let Some(id) = id.filter(|i| *i >= 1) else {
        return erro(400, "id inválido");
    };
    // `String(resultado || 'ok').slice(0, 200)`.
    let resultado = match corpo
        .get("resultado")
        .filter(|v| nucleo::js::verdadeiro(Some(v)))
    {
        Some(v) => nucleo::js::texto(Some(v)),
        None => "ok".to_owned(),
    };
    let resultado: String = resultado.chars().take(MAX_RESULTADO).collect();
    let contas: Vec<String> = cliente
        .as_ref()
        .and_then(|Extension(c)| c.conta.clone())
        .into_iter()
        .collect();
    match comandos::confirmar_no_escopo(&estado, &contas, id, &resultado).await {
        Ok(comandos::Confirmada::Negada) => proibido(),
        Ok(_) => Json(json!({ "ok": true, "id": id })).into_response(),
        Err(e) => indisponivel(&e),
    }
}

/// `POST /api/inserir-comando` `{nick, cmd, params?, origem?, ts_expira_min?}`.
pub async fn inserir(
    State(estado): State<Estado>,
    cliente: Option<Extension<Cliente>>,
    corpo: Bytes,
) -> Response {
    if let Some(r) = desligada(&estado) {
        return r;
    }
    let corpo = corpo_json(&corpo);
    let texto = |c: &str| {
        corpo
            .get(c)
            .filter(|v| nucleo::js::verdadeiro(Some(v)))
            .map(|v| nucleo::js::texto(Some(v)))
    };
    let (Some(nick), Some(cmd)) = (texto("nick"), texto("cmd")) else {
        return erro(400, "preciso de {nick, cmd}");
    };
    // `params` ausente vira `{}`; `null` explícito é preservado, como no legado.
    let params = corpo.get("params").cloned().unwrap_or_else(|| json!({}));
    // O legado conta caracteres do JSON, não bytes.
    if params.to_string().chars().count() > 10_000 {
        return erro(400, "params muito grande (max 10KB)");
    }
    let origem = texto("origem").unwrap_or_else(|| "manual".to_owned());
    let nick = match nick_permitido(cliente.as_ref(), &nick) {
        Ok(n) => n,
        Err(()) => return proibido(),
    };
    // `Math.min(Number(x) || 5, 1440)`: 0 e NaN viram 5; negativo passa (nasce expirado).
    let pedido = corpo.get("ts_expira_min");
    let bruto = nucleo::js::mais(pedido);
    // Abaixo de -1440 estouraria o cálculo do instante (e viraria 503): é 400.
    if bruto < -1440.0 {
        return erro(400, "ts_expira_min abaixo de -1440");
    }
    // Acima de 1440 o legado limita a 1440 (`Math.min`), sem erro.
    let mut minutos = if bruto.is_nan() || bruto == 0.0 {
        5.0
    } else {
        bruto.min(1440.0)
    };
    if estado.melhorias {
        // Melhoria: o prazo fica sempre entre 1 minuto e 24 horas.
        minutos = minutos.clamp(1.0, 1440.0);
    }
    let expira_ms = agora_ms().saturating_add((minutos * 60_000.0) as i64);
    let novo = NovoComando {
        conta: &nick,
        tipo: &cmd,
        params: &params,
        prioridade: 5,
        origem: &origem,
        expira_ms,
    };
    match fila::inserir(&estado.db, &novo).await {
        Ok(id) => Json(json!({ "ok": true, "id": id })).into_response(),
        Err(e) => indisponivel(&e),
    }
}
