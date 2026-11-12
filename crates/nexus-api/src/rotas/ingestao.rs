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
pub(crate) fn rotulo(tipo: &str) -> String {
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

/// Resultado de gravar um lote de eventos.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResultadoLote {
    /// Eventos válidos (gravados agora ou já existentes).
    pub ok: usize,
    /// Eventos rejeitados pela validação.
    pub erros: usize,
    /// Eventos que já estavam gravados.
    pub duplicados: usize,
    pub total: usize,
}

/// Por que um lote não foi gravado.
#[derive(Debug)]
pub enum ErroLote {
    Desligada,
    /// Token preso a outra conta.
    ContaProibida,
    Banco(sqlx::Error),
}

/// Grava um lote: o caminho único do `POST /ingest` e do `evento.lote` do WebSocket (mesma
/// validação, mesmo escopo de conta, mesma flag `NEXUS_ESCRITA_INGESTAO`, mesmas métricas).
/// `conta_token`: conta a que o token está preso (`None` = token global).
pub async fn gravar_lote(
    estado: &Estado,
    conta_token: Option<&str>,
    conta_id: &str,
    nick: Option<&str>,
    eventos_brutos: &[Value],
) -> Result<ResultadoLote, ErroLote> {
    if !estado.escrita_ingestao {
        return Err(ErroLote::Desligada);
    }
    let conta_id = normalizar(conta_id);
    let nick = nick.map(normalizar).filter(|n| !n.is_empty());
    // `conta` é o nick quando vier; sem nick, é o próprio `conta_id`. Token de conta só aceita
    // lote cujo nick é a conta dele; lote sem nick (conta global) exige token global.
    let conta = nick.clone().unwrap_or_else(|| conta_id.clone());
    if let Some(propria) = conta_token {
        if nick
            .as_deref()
            .is_none_or(|n| !propria.eq_ignore_ascii_case(n))
        {
            return Err(ErroLote::ContaProibida);
        }
    }
    let lote = eventos::validar(&conta, eventos_brutos, agora_ms(), estado.melhorias);
    let sem_ts = lote.eventos.iter().filter(|e| e.sem_ts).count();
    if sem_ts > 0 {
        metrics::counter!("nexus_ingestao_sem_ts_total").increment(sem_ts as u64);
    }
    let inseridos = eventos::gravar(&estado.db, &conta, Some(&conta_id), &lote.eventos)
        .await
        .map_err(ErroLote::Banco)?;
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
    if !inseridos.is_empty() {
        metrics::gauge!("nexus_ingestao_ultima_gravacao_segundos").set(agora_ms() as f64 / 1000.0);
    }
    Ok(ResultadoLote {
        ok: lote.eventos.len(),
        erros: lote.erros.len(),
        duplicados: lote.eventos.len().saturating_sub(inseridos.len()),
        total: eventos_brutos.len(),
    })
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
    if let Some(Extension(c)) = cliente.as_ref() {
        if estado.melhorias && !estado.limite_ingestao.permitir(&c.nome) {
            return erro(429, "limite de ingestão excedido");
        }
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
    let nick = texto(corpo.get("nick"));
    let conta_token = cliente.as_ref().and_then(|Extension(c)| c.conta.as_deref());
    match gravar_lote(
        &estado,
        conta_token,
        &conta_id,
        nick.as_deref(),
        eventos_brutos,
    )
    .await
    {
        Ok(r) => Json(json!({ "ok": r.ok, "erros": r.erros, "total": r.total })).into_response(),
        Err(ErroLote::Desligada) => desligada(&estado).unwrap_or_else(|| erro(503, "desligada")),
        Err(ErroLote::ContaProibida) => erro(403, "este token só grava a própria conta"),
        Err(ErroLote::Banco(e)) => indisponivel(&e),
    }
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
    if let Some(Extension(c)) = cliente.as_ref() {
        if estado.melhorias && !estado.limite_ingestao.permitir(&c.nome) {
            return erro(429, "limite de ingestão excedido");
        }
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
