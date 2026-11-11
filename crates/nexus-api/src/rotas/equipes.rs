//! Rotas `/api/equipes/*`. Mesmo contrato JSON do sistema legado.
//!
//! O cálculo (dezenas a milhares de duelos) roda em `spawn_blocking`, numa thread separada,
//! para não segurar as threads do tokio que atendem as outras requisições.

use std::collections::HashMap;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use contrato::Erro;
use nucleo::equipes;
use serde_json::Value;

use crate::auth::Cliente;
use crate::bolsa::agora_ms;
use crate::equipes::ErroServico;
use crate::estado::Estado;

type Params = Query<HashMap<String, String>>;

fn indisponivel(e: &ErroServico) -> Response {
    tracing::warn!(erro = %e, "equipes indisponíveis");
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(Erro::novo(e.to_string())),
    )
        .into_response()
}

fn erro(status: u16, msg: &str) -> Response {
    let s = StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (s, Json(Erro::novo(msg))).into_response()
}

/// O nick permitido para o token (o da conta dele, se estiver preso a uma).
fn nick_de(
    cliente: Option<&Extension<Cliente>>,
    pedido: Option<&str>,
) -> Result<Option<String>, ()> {
    match cliente {
        Some(Extension(c)) => c.nick_permitido(pedido),
        None => Ok(pedido.filter(|n| !n.is_empty()).map(str::to_owned)),
    }
}

fn proibido() -> Response {
    erro(403, "este token só acessa a própria conta")
}

/// Busca motor e estado, ou devolve 503.
macro_rules! dados {
    ($estado:expr) => {{
        let motor = match $estado.equipes.motor().await {
            Ok(m) => m,
            Err(e) => return indisponivel(&e),
        };
        match $estado.equipes.estado().await {
            Ok(e) => (motor, e),
            Err(e) => return indisponivel(&e),
        }
    }};
}

pub async fn bosses(
    State(estado): State<Estado>,
    cliente: Option<Extension<Cliente>>,
    Query(p): Params,
) -> Response {
    let nick = match nick_de(cliente.as_ref(), p.get("nick").map(String::as_str)) {
        Ok(n) => n,
        Err(()) => return proibido(),
    };
    let (motor, est) = dados!(estado);
    let r = tokio::task::spawn_blocking(move || {
        equipes::resposta_bosses(&motor, &est, nick.as_deref())
    })
    .await;
    match r {
        Ok(v) => Json(v).into_response(),
        Err(_) => erro(500, "falha no cálculo"),
    }
}

pub async fn plano(
    State(estado): State<Estado>,
    cliente: Option<Extension<Cliente>>,
    Query(p): Params,
) -> Response {
    let nick = match nick_de(cliente.as_ref(), p.get("nick").map(String::as_str)) {
        Ok(n) => n,
        Err(()) => return proibido(),
    };
    let (motor, est) = dados!(estado);
    let r = tokio::task::spawn_blocking(move || {
        let g = |k: &str| p.get(k).map(String::as_str);
        equipes::resposta_plano(&motor, &est, nick.as_deref(), g("boss"), g("perfil"))
    })
    .await;
    match r {
        Ok(Ok(v)) => Json(v).into_response(),
        Ok(Err(e)) => erro(e.status, e.erro),
        Err(_) => erro(500, "falha no cálculo"),
    }
}

/// `POST /api/equipes/ingest`: só com `NEXUS_ESCRITA_EQUIPES=true`. Até o cutover, quem
/// recebe o relay é o legado, e o estado chega aqui pela importação.
pub async fn ingest(
    State(estado): State<Estado>,
    cliente: Option<Extension<Cliente>>,
    Json(corpo): Json<Value>,
) -> Response {
    if nick_de(cliente.as_ref(), corpo.get("nick").and_then(Value::as_str)).is_err() {
        return proibido();
    }
    if !estado.escrita_equipes {
        return erro(
            503,
            "escrita de equipes desligada neste servidor (NEXUS_ESCRITA_EQUIPES)",
        );
    }
    let _vez = estado.equipes.gravacao.lock().await;
    let atual = match estado.equipes.estado().await {
        Ok(e) => e,
        Err(e) => return indisponivel(&e),
    };
    let mut novo = (*atual).clone();
    match novo.ingerir(&corpo, agora_ms()) {
        Ok((nick, resumo)) => match estado.equipes.gravar(novo, &nick).await {
            Ok(()) => Json(resumo).into_response(),
            Err(e) => indisponivel(&e),
        },
        Err(msg) => erro(400, msg),
    }
}

// ---------- equipe da bolsa ----------

use crate::equipes::bolsa::{Pedido, Situacao};

/// `GET /api/bolsa/equipe`: responde direto quando o cálculo termina em até 1,5 s; senão
/// `202` com o resultado anterior e o progresso, como no legado.
pub async fn equipe_bolsa(
    State(estado): State<Estado>,
    cliente: Option<Extension<Cliente>>,
    Query(p): Params,
) -> Response {
    let nick = match nick_de(cliente.as_ref(), p.get("nick").map(String::as_str)) {
        Ok(n) => n,
        Err(()) => return proibido(),
    };
    let g = |k: &str| p.get(k).filter(|v| !v.is_empty()).cloned();
    let idade = nucleo::js::mais(p.get("idade").map(|v| Value::from(v.as_str())).as_ref());
    let idade = if idade == 0.0 || idade.is_nan() {
        90.0
    } else {
        idade
    };
    let pedido = Pedido {
        boss: g("boss"),
        moeda: if p.get("moeda").map(String::as_str) == Some("gold") {
            "gold".into()
        } else {
            "orb".into()
        },
        nick: nick.clone(),
        idade_min: idade.clamp(5.0, 1440.0 * 7.0),
        n: 5,
        orcamento: p
            .get("orcamento")
            .and_then(|v| v.parse::<f64>().ok())
            .filter(|v| v.is_finite() && *v > 0.0),
    };
    let job = estado.equipe_bolsa.trabalho(&estado, pedido);
    match job.esperar(std::time::Duration::from_millis(1500)).await {
        Situacao::Pronto(d) => {
            let status = if d.get("erro").is_some() {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::OK
            };
            (status, Json((*d).clone())).into_response()
        }
        Situacao::Erro(e) => erro(500, &e),
        Situacao::Carregando {
            anterior,
            progresso,
        } => (
            StatusCode::ACCEPTED,
            Json(serde_json::json!({
                "carregando": true,
                "mensagem": "Avaliando o valor justo das ofertas e o custo-benefício da equipe.",
                "anterior": anterior.map(|a| (*a).clone()),
                "progresso": progresso,
            })),
        )
            .into_response(),
    }
}

/// `GET /api/bolsa/equipe-propria`.
pub async fn equipe_propria(
    State(estado): State<Estado>,
    cliente: Option<Extension<Cliente>>,
    Query(p): Params,
) -> Response {
    let nick = match nick_de(cliente.as_ref(), p.get("nick").map(String::as_str)) {
        Ok(n) => n,
        Err(()) => return proibido(),
    };
    let g = |k: &str| p.get(k).cloned();
    match estado.equipe_bolsa.propria(&estado, g("boss"), nick).await {
        Ok(d) => {
            let status = if d.get("erro").is_some() {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::OK
            };
            (status, Json((*d).clone())).into_response()
        }
        Err(e) => {
            tracing::warn!(erro = %e, "equipe própria");
            erro(500, &e.to_string())
        }
    }
}

/// `GET /api/bolsa/alvos-coleta`.
pub async fn alvos_coleta(
    State(estado): State<Estado>,
    cliente: Option<Extension<Cliente>>,
    Query(p): Params,
) -> Response {
    let nick = match nick_de(cliente.as_ref(), p.get("nick").map(String::as_str)) {
        Ok(n) => n,
        Err(()) => return proibido(),
    };
    match estado
        .equipe_bolsa
        .alvos(&estado, nick, p.get("max").cloned())
        .await
    {
        Ok(v) => Json(v).into_response(),
        Err(e) => {
            tracing::warn!(erro = %e, "alvos de coleta");
            erro(500, &e.to_string())
        }
    }
}

/// `GET /api/bolsa/equipe-hibrida` (rota nova): a equipe com os pokémon da conta, comprando
/// só as vagas que faltam.
pub async fn equipe_hibrida(
    State(estado): State<Estado>,
    cliente: Option<Extension<Cliente>>,
    Query(p): Params,
) -> Response {
    let nick = match nick_de(cliente.as_ref(), p.get("nick").map(String::as_str)) {
        Ok(n) => n,
        Err(()) => return proibido(),
    };
    let g = |k: &str| p.get(k).filter(|v| !v.is_empty()).cloned();
    let moeda = if p.get("moeda").map(String::as_str) == Some("gold") {
        "gold"
    } else {
        "orb"
    };
    match estado
        .equipe_bolsa
        .hibrida(&estado, g("boss"), nick, moeda.into())
        .await
    {
        Ok(v) => {
            let status = if v.get("erro").is_some() {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::OK
            };
            (status, Json(v)).into_response()
        }
        Err(e) => erro(500, &e.to_string()),
    }
}
