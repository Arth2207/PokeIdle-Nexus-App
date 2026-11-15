//! `POST /api/motor/janela` `{conta, janela}`: a janela da conta montada pelo app (Fase 4).
//! O motor do Nexus lê daqui quando roda sem o motor do legado (`NEXUS_MOTOR_SOMBRA_URL=nexus`).
//! `GET /api/contas`: as contas do app (rótulo, nick no jogo e a última janela), para o painel.
//! `GET /api/cacada?conta=`: sessão da conta por hora, hunts e drops (tela Caçada).
//! `GET /api/motor/decisoes`: decisões do motor (6 h) e o resultado dos comandos por tipo (24 h).

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use contrato::Erro;
use serde_json::{json, Value};

use crate::auth::Cliente;
use crate::estado::Estado;

fn erro(status: StatusCode, msg: &str) -> Response {
    (status, Json(Erro::novo(msg))).into_response()
}

pub async fn gravar(
    State(estado): State<Estado>,
    cliente: Option<Extension<Cliente>>,
    Json(corpo): Json<Value>,
) -> Response {
    if !estado.escrita_ingestao {
        return erro(
            StatusCode::SERVICE_UNAVAILABLE,
            "ingestão desligada neste servidor (NEXUS_ESCRITA_INGESTAO)",
        );
    }
    let (Some(conta), Some(janela)) = (
        corpo
            .get("conta")
            .and_then(Value::as_str)
            .map(str::to_lowercase),
        corpo.get("janela").filter(|j| j.get("nick").is_some()),
    ) else {
        return erro(StatusCode::BAD_REQUEST, "falta conta ou janela.nick");
    };
    let presa = cliente.as_ref().and_then(|Extension(c)| c.conta.as_deref());
    if presa.is_some_and(|p| !p.eq_ignore_ascii_case(&conta)) {
        return erro(StatusCode::FORBIDDEN, "este token só grava a própria conta");
    }
    match persistencia::motor::gravar_janela(&estado.db, &conta, janela).await {
        Ok(()) => Json(json!({"ok": true})).into_response(),
        Err(e) => {
            tracing::warn!(erro = %e, "janela do motor");
            erro(StatusCode::SERVICE_UNAVAILABLE, "banco indisponível")
        }
    }
}

pub async fn contas(State(estado): State<Estado>) -> Response {
    let motor = estado.equipes.motor().await.ok();
    match persistencia::motor::contas_do_app(&estado.db).await {
        Ok(mut c) => {
            // Bioma do fundo do painel, pela hunt da conta.
            if let Some(m) = motor.as_ref() {
                for x in &mut c {
                    let b = m.bioma_da_hunt(x.get("hunt").and_then(Value::as_str).unwrap_or(""));
                    if let Value::Object(o) = x {
                        o.insert("bioma".into(), json!(b));
                    }
                }
            }
            Json(json!({ "contas": c })).into_response()
        }
        Err(e) => {
            tracing::warn!(erro = %e, "contas do app");
            erro(StatusCode::SERVICE_UNAVAILABLE, "banco indisponível")
        }
    }
}

/// `GET /api/financas/mercado?dias=30`: vendas e compras do Mercado de cada conta, com a
/// moeda de cada uma (o diário do jogo só tem o gold da caçada e da loja).
pub async fn extrato_mercado(
    State(estado): State<Estado>,
    Query(p): Query<std::collections::HashMap<String, String>>,
) -> Response {
    let dias: i64 = p
        .get("dias")
        .and_then(|d| d.parse().ok())
        .unwrap_or(30)
        .clamp(1, 90);
    let desde = crate::bolsa::agora_ms() - dias * 86_400_000;
    match persistencia::pvp::extrato_mercado(&estado.db, desde).await {
        Ok(l) => {
            let linhas: Vec<Value> = l
                .into_iter()
                .map(|(conta, lado, mut x)| {
                    if let Value::Object(o) = &mut x {
                        o.insert("conta".into(), Value::from(conta));
                        o.insert("lado".into(), Value::from(lado));
                    }
                    x
                })
                .collect();
            Json(json!({ "linhas": linhas })).into_response()
        }
        Err(e) => {
            tracing::warn!(erro = %e, "extrato do mercado");
            erro(StatusCode::SERVICE_UNAVAILABLE, "banco indisponível")
        }
    }
}

/// `GET /api/financas/cacada?dias=30`: abates e ouro da caçada por conta e dia (`fato_kill`).
pub async fn ouro_cacada(
    State(estado): State<Estado>,
    Query(p): Query<std::collections::HashMap<String, String>>,
) -> Response {
    let dias: i64 = p
        .get("dias")
        .and_then(|d| d.parse().ok())
        .unwrap_or(30)
        .clamp(1, 90);
    match persistencia::cacada::ouro_por_dia(&estado.db, dias).await {
        Ok(l) => Json(json!({ "dias": l })).into_response(),
        Err(e) => {
            tracing::warn!(erro = %e, "ouro da caçada");
            erro(StatusCode::SERVICE_UNAVAILABLE, "banco indisponível")
        }
    }
}

/// `GET /api/motor/regras`: as regras do motor de cada conta (para ligar/desligar na tela).
pub async fn regras(State(estado): State<Estado>) -> Response {
    match persistencia::motor::config_contas(&estado.db).await {
        Ok(l) => Json(json!({ "regras": l })).into_response(),
        Err(e) => {
            tracing::warn!(erro = %e, "regras do motor");
            erro(StatusCode::SERVICE_UNAVAILABLE, "banco indisponível")
        }
    }
}

/// `POST /api/motor/config` `{nick, secao, campo, valor}`: liga/desliga ou ajusta uma regra
/// do motor de uma conta. Só troca campo que já existe, por valor do mesmo tipo.
pub async fn alterar_config(State(estado): State<Estado>, Json(c): Json<Value>) -> Response {
    let t = |k: &str| c.get(k).and_then(Value::as_str).unwrap_or_default();
    let (nick, secao, campo) = (t("nick"), t("secao"), t("campo"));
    let Some(valor) = c
        .get("valor")
        .filter(|v| v.is_boolean() || v.is_number() || v.is_string())
    else {
        return erro(
            StatusCode::BAD_REQUEST,
            "valor precisa ser booleano, número ou texto",
        );
    };
    if nick.is_empty() || secao.is_empty() || campo.is_empty() {
        return erro(StatusCode::BAD_REQUEST, "informe nick, secao e campo");
    }
    match persistencia::motor::alterar_config(&estado.db, nick, secao, campo, valor).await {
        Ok(true) => {
            tracing::info!(nick, secao, campo, %valor, "regra do motor alterada");
            Json(json!({"ok": true})).into_response()
        }
        Ok(false) => erro(
            StatusCode::NOT_FOUND,
            "regra ou campo não existe, ou o tipo não confere",
        ),
        Err(e) => {
            tracing::warn!(erro = %e, "alterar regra do motor");
            erro(StatusCode::SERVICE_UNAVAILABLE, "banco indisponível")
        }
    }
}

pub async fn cacada(
    State(estado): State<Estado>,
    Query(p): Query<std::collections::HashMap<String, String>>,
) -> Response {
    use persistencia::cacada as c;
    let rotulo = p.get("conta").map(String::as_str).unwrap_or_default();
    let sessao = match c::conta_id(rotulo) {
        Some(id) => c::sessao(&estado.db, id).await.map(Some),
        None => Ok(None),
    };
    let r = async {
        Ok::<_, sqlx::Error>(json!({
            "sessao": sessao?,
            "hunts": c::hunts(&estado.db).await?,
            "drops": c::drops(&estado.db, rotulo).await?,
        }))
    }
    .await;
    match r {
        Ok(v) => Json(v).into_response(),
        Err(e) => {
            tracing::warn!(erro = %e, "caçada");
            erro(StatusCode::SERVICE_UNAVAILABLE, "banco indisponível")
        }
    }
}

pub async fn decisoes(State(estado): State<Estado>) -> Response {
    use persistencia::motor as m;
    let r = async {
        Ok::<_, sqlx::Error>(json!({
            "decisoes": m::decisoes_recentes(&estado.db).await?,
            "regras": m::regras_resumo(&estado.db).await?,
        }))
    }
    .await;
    match r {
        Ok(v) => Json(v).into_response(),
        Err(e) => {
            tracing::warn!(erro = %e, "decisões do motor");
            erro(StatusCode::SERVICE_UNAVAILABLE, "banco indisponível")
        }
    }
}
