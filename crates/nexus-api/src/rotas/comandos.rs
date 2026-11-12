//! Rotas da fila de comandos: `GET /api/comandos`, `POST /api/confirmar-comando` e
//! `POST /api/inserir-comando`. Mesmo contrato do legado. Só com `NEXUS_ESCRITA_COMANDOS`
//! (a primeira reserva comandos, então também é escrita).

use std::collections::HashMap;

use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
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
    cabecalhos: HeaderMap,
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
    // Chave de idempotência: header `Idempotency-Key` ou campo `chave`. Sem chave, igual ao legado.
    let chave = cabecalhos
        .get("idempotency-key")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty())
        .or_else(|| texto("chave"));
    if chave.as_ref().is_some_and(|k| k.chars().count() > 200) {
        return erro(400, "chave de idempotência muito longa (max 200)");
    }
    // Agendamento (só com NEXUS_MELHORIAS): `executar_em` em ms desde 1970, até 30 dias à frente.
    let agora = agora_ms();
    let executar_ms = match corpo.get("executar_em").filter(|_| estado.melhorias) {
        None | Some(Value::Null) => None,
        Some(v) => {
            let ms = nucleo::js::mais(Some(v));
            if ms.is_nan() || ms < 0.0 || ms > (agora + JANELA_AGENDAMENTO_MS) as f64 {
                return erro(
                    400,
                    "executar_em inválido (ms desde 1970, até 30 dias à frente)",
                );
            }
            // Instante já passado: executa já, sem agendar.
            Some(ms as i64).filter(|ms| *ms > agora)
        }
    };
    // O prazo conta a partir da hora agendada.
    let base = executar_ms.unwrap_or(agora);
    let expira_ms = base.saturating_add((minutos * 60_000.0) as i64);
    let novo = NovoComando {
        conta: &nick,
        tipo: &cmd,
        params: &params,
        prioridade: 5,
        origem: &origem,
        expira_ms,
    };
    match fila::inserir_ext(&estado.db, &novo, chave.as_deref(), executar_ms).await {
        Ok((id, false)) => Json(json!({ "ok": true, "id": id })).into_response(),
        Ok((id, true)) => Json(json!({ "ok": true, "id": id, "repetido": true })).into_response(),
        Err(e) => indisponivel(&e),
    }
}

/// Máximo de agendamento à frente (30 dias).
const JANELA_AGENDAMENTO_MS: i64 = 30 * 86_400_000;

fn sem_melhorias(estado: &Estado) -> Option<Response> {
    (!estado.melhorias).then(|| erro(404, "rota disponível só com NEXUS_MELHORIAS"))
}

/// `GET /api/comandos/falhos?nick=&limite=` (`NEXUS_MELHORIAS`): comandos que esgotaram as
/// tentativas. Token de conta vê só a própria conta; token global vê todas (ou filtra por `nick`).
pub async fn falhos(
    State(estado): State<Estado>,
    cliente: Option<Extension<Cliente>>,
    Query(p): Query<HashMap<String, String>>,
) -> Response {
    if let Some(r) = sem_melhorias(&estado) {
        return r;
    }
    let contas: Vec<String> = match p.get("nick").filter(|n| !n.is_empty()) {
        Some(n) => match nick_permitido(cliente.as_ref(), n) {
            Ok(n) => vec![n],
            Err(()) => return proibido(),
        },
        None => cliente
            .as_ref()
            .and_then(|Extension(c)| c.conta.clone())
            .into_iter()
            .collect(),
    };
    let limite = p
        .get("limite")
        .and_then(|l| l.parse::<i64>().ok())
        .unwrap_or(100)
        .clamp(1, 500);
    match fila::falhos(&estado.db, &contas, limite).await {
        Ok(v) => Json(json!({
            "falhos": v.into_iter().map(|c| json!({
                "id": c.id, "conta": c.conta, "cmd": c.tipo, "params": c.params,
                "origem": c.origem, "tentativas": c.tentativas, "criadoEm": c.criado_ms,
            })).collect::<Vec<_>>(),
        }))
        .into_response(),
        Err(e) => indisponivel(&e),
    }
}

/// `POST /api/comandos/{id}/reenviar` (`NEXUS_MELHORIAS`): volta um falho para pendente.
pub async fn reenviar(
    State(estado): State<Estado>,
    cliente: Option<Extension<Cliente>>,
    Path(id): Path<i64>,
) -> Response {
    if let Some(r) = sem_melhorias(&estado) {
        return r;
    }
    if let Some(r) = desligada(&estado) {
        return r;
    }
    let dono = match fila::conta_de(&estado.db, id).await {
        Ok(d) => d,
        Err(e) => return indisponivel(&e),
    };
    let Some(dono) = dono else {
        return erro(404, "comando não encontrado");
    };
    if cliente
        .as_ref()
        .and_then(|Extension(c)| c.conta.as_deref())
        .is_some_and(|propria| !propria.eq_ignore_ascii_case(&dono))
    {
        return proibido();
    }
    match fila::reenviar(&estado.db, id).await {
        Ok(true) => Json(json!({ "ok": true, "id": id })).into_response(),
        Ok(false) => erro(409, "o comando não está na lista de falhos"),
        Err(e) => indisponivel(&e),
    }
}
