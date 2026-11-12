//! `POST /ingest` e `POST /api/hunt-sync`: o relay manda lotes de eventos do jogo e a hunt
//! atual de cada conta. Mesmo contrato do legado; só grava com `NEXUS_ESCRITA_INGESTAO`.
//!
//! O token de relay precisa do escopo `relay` (ou `escrita`). Um token preso a uma conta só
//! grava a própria. O legado confiava na rede interna; o Nexus não.

use axum::body::Bytes;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use contrato::Erro;
use persistencia::eventos::{self, normalizar, TIPOS};
use serde_json::{json, Value};

use crate::auth::Cliente;
use crate::bolsa::agora_ms;
use crate::estado::Estado;
use crate::rotas::corpo_json;

fn erro(status: u16, msg: &str) -> Response {
    let s = StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (s, Json(Erro::novo(msg))).into_response()
}

fn desligada(estado: &Estado) -> Option<Response> {
    (!estado.escrita_ingestao).then(|| {
        erro(
            503,
            "ingestão desligada neste servidor (NEXUS_ESCRITA_INGESTAO)",
        )
    })
}

fn indisponivel(e: &sqlx::Error) -> Response {
    tracing::warn!(erro = %e, "ingestão indisponível");
    erro(503, "banco indisponível")
}

/// O token preso a uma conta só escreve nela.
fn conta_liberada(cliente: Option<&Extension<Cliente>>, conta: &str) -> bool {
    cliente.is_none_or(|Extension(c)| {
        c.conta
            .as_deref()
            .is_none_or(|propria| propria.eq_ignore_ascii_case(conta))
    })
}

/// Rótulo de métrica: tipos fora da lista conhecida viram `desconhecido` (o rótulo vem do
/// cliente, e não pode criar séries sem limite).
fn rotulo(tipo: &str) -> String {
    if TIPOS.contains(&tipo) {
        tipo
    } else {
        "desconhecido"
    }
    .to_owned()
}

/// Texto "verdadeiro" do JS: ausente, vazio, 0 e `null` não contam.
fn texto(v: Option<&Value>) -> Option<String> {
    v.filter(|v| nucleo::js::verdadeiro(Some(v)))
        .map(|v| nucleo::js::texto(Some(v)))
}

/// `POST /ingest` `{conta_id, eventos[], nick?}` → `{ok, erros, total}`.
pub async fn ingest(
    State(estado): State<Estado>,
    cliente: Option<Extension<Cliente>>,
    corpo: Bytes,
) -> Response {
    if let Some(r) = desligada(&estado) {
        return r;
    }
    let corpo = corpo_json(&corpo);
    let eventos_brutos = corpo
        .get("eventos")
        .and_then(Value::as_array)
        .filter(|e| !e.is_empty());
    let (Some(conta_id), Some(eventos_brutos)) = (texto(corpo.get("conta_id")), eventos_brutos)
    else {
        return erro(400, "falta conta_id ou eventos[]");
    };
    let nick = texto(corpo.get("nick"))
        .map(|n| normalizar(&n))
        .filter(|n| !n.is_empty());
    let conta_id = normalizar(&conta_id);
    // `conta` é o nick quando vier; sem nick, é o próprio `conta_id`. Grava-se também o
    // `conta_id`. Token de conta só aceita lote cujo nick é a conta dele; lote sem nick (conta
    // global) exige token global.
    let conta = nick.clone().unwrap_or_else(|| conta_id.clone());
    if let Some(Extension(c)) = cliente.as_ref() {
        if let Some(propria) = &c.conta {
            if nick
                .as_deref()
                .is_none_or(|n| !propria.eq_ignore_ascii_case(n))
            {
                return erro(403, "este token só grava a própria conta");
            }
        }
    }
    let lote = eventos::validar(&conta, eventos_brutos, agora_ms(), estado.melhorias);
    let sem_ts = lote.eventos.iter().filter(|e| e.sem_ts).count();
    if sem_ts > 0 {
        metrics::counter!("nexus_ingestao_sem_ts_total").increment(sem_ts as u64);
    }
    let inseridos = match eventos::gravar(&estado.db, &conta, Some(&conta_id), &lote.eventos).await
    {
        Ok(t) => t,
        Err(e) => return indisponivel(&e),
    };
    for tipo in TIPOS.iter().copied().chain(["desconhecido"]) {
        let aceitos = lote
            .eventos
            .iter()
            .filter(|e| rotulo(&e.tipo) == tipo)
            .count();
        let novos = inseridos.iter().filter(|t| rotulo(t) == tipo).count();
        let erros = lote.erros.iter().filter(|t| rotulo(t) == tipo).count();
        let conta_metrica = |resultado: &'static str, n: usize| {
            if n > 0 {
                metrics::counter!("nexus_ingest_eventos_total", "tipo" => tipo, "resultado" => resultado)
                    .increment(n as u64);
            }
        };
        conta_metrica("aceito", novos);
        conta_metrica("duplicado", aceitos.saturating_sub(novos));
        conta_metrica("erro", erros);
    }
    Json(json!({
        "ok": lote.eventos.len(),
        "erros": lote.erros.len(),
        "total": eventos_brutos.len(),
    }))
    .into_response()
}

/// `POST /api/hunt-sync` `{eventos: [{nick, hunt_slug}]}` → `{atualizados}`.
pub async fn hunt_sync(
    State(estado): State<Estado>,
    cliente: Option<Extension<Cliente>>,
    corpo: Bytes,
) -> Response {
    if let Some(r) = desligada(&estado) {
        return r;
    }
    let corpo = corpo_json(&corpo);
    // Sem nick ou sem hunt, o evento é pulado em silêncio, como no legado. Se o mesmo nick
    // aparece duas vezes, vale a última.
    let mut contas: Vec<(String, String)> = Vec::new();
    for ev in corpo
        .get("eventos")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice)
    {
        let (Some(nick), Some(slug)) = (texto(ev.get("nick")), texto(ev.get("hunt_slug"))) else {
            continue;
        };
        let nick = normalizar(&nick);
        if nick.is_empty() {
            continue;
        }
        // Nick fora do escopo do token: pulado (não conta como atualizado), sem derrubar o lote.
        if !conta_liberada(cliente.as_ref(), &nick) {
            continue;
        }
        contas.retain(|(n, _)| *n != nick);
        contas.push((nick, slug));
    }
    match eventos::gravar_hunt(&estado.db, &contas, agora_ms()).await {
        Ok(n) => Json(json!({ "atualizados": n })).into_response(),
        Err(e) => indisponivel(&e),
    }
}
