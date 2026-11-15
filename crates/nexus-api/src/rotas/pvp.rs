//! PvP, shiny do servidor e fichas de raid (dados que o app grava em `nexus.evento`):
//!
//! - `GET /api/pvp/meta?faixa=&dias=`: espécies, casas e times que vencem na faixa de pontos.
//! - `GET /api/pvp/sugestao?nick=&faixa=`: time de 5 e casas para a conta, contra o meta.
//! - `GET /api/pvp/treinos?nick=`: resultados da Área de Treinamento.
//! - `GET /api/shiny/servidor?dias=`: shiny capturados no servidor inteiro.
//! - `GET /api/boss/fichas`: fichas dos bosses de raid (golpes, tipos, multiplicadores).

use std::collections::HashMap;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use contrato::Erro;
use nucleo::combate::especie::Pokedex;
use nucleo::pvp::{meta, shiny, sugestao};
use serde_json::{json, Value};

use crate::auth::Cliente;
use crate::bolsa::agora_ms;
use crate::estado::Estado;

type Params = Query<HashMap<String, String>>;

const DIA_MS: i64 = 86_400_000;
/// Abaixo disto a faixa tem pouca amostra e a sugestão usa o meta de todas as faixas.
const MIN_PARTIDAS_FAIXA: u32 = 20;

fn erro(status: StatusCode, msg: &str) -> Response {
    (status, Json(Erro::novo(msg))).into_response()
}

fn banco(e: &sqlx::Error) -> Response {
    tracing::warn!(erro = %e, "pvp: banco indisponível");
    erro(StatusCode::SERVICE_UNAVAILABLE, "banco indisponível")
}

fn dias(p: &HashMap<String, String>, padrao: i64) -> i64 {
    p.get("dias")
        .and_then(|d| d.parse::<i64>().ok())
        .map_or(padrao, |d| d.clamp(1, 90))
}

/// O nick pedido, respeitando o token preso a uma conta.
fn nick(
    cliente: Option<&Extension<Cliente>>,
    p: &HashMap<String, String>,
) -> Result<String, (StatusCode, &'static str)> {
    let pedido = p.get("nick").map(String::as_str);
    let n = match cliente {
        Some(Extension(c)) => c.nick_permitido(pedido),
        None => Ok(pedido.filter(|n| !n.is_empty()).map(str::to_owned)),
    };
    match n {
        Ok(Some(n)) => Ok(n.to_lowercase()),
        Ok(None) => Err((StatusCode::BAD_REQUEST, "falta nick")),
        Err(()) => Err((
            StatusCode::FORBIDDEN,
            "este token só acessa a própria conta",
        )),
    }
}

async fn carregar_partidas(estado: &Estado, dias: i64) -> Result<Vec<meta::Partida>, sqlx::Error> {
    let linhas = persistencia::pvp::partidas(&estado.db, agora_ms() - dias * DIA_MS).await?;
    Ok(linhas
        .iter()
        .filter_map(|(em, d)| meta::Partida::de_evento(*em, d))
        .collect())
}

pub async fn meta_pvp(State(estado): State<Estado>, Query(p): Params) -> Response {
    let partidas = match carregar_partidas(&estado, dias(&p, 14)).await {
        Ok(v) => v,
        Err(e) => return banco(&e),
    };
    let faixa = p
        .get("faixa")
        .and_then(|f| f.parse::<i64>().ok())
        .map(meta::faixa);
    Json(meta::agregar(&partidas, faixa)).into_response()
}

pub async fn sugestao_pvp(
    State(estado): State<Estado>,
    cliente: Option<Extension<Cliente>>,
    Query(p): Params,
) -> Response {
    let nick = match nick(cliente.as_ref(), &p) {
        Ok(n) => n,
        Err((st, msg)) => return erro(st, msg),
    };
    let atual = match persistencia::pvp::estado_pvp(&estado.db, &nick).await {
        Ok(v) => v.map(|x| x.1),
        Err(e) => return banco(&e),
    };
    let partidas = match carregar_partidas(&estado, dias(&p, 14)).await {
        Ok(v) => v,
        Err(e) => return banco(&e),
    };
    let pontos = p
        .get("faixa")
        .and_then(|f| f.parse::<i64>().ok())
        .or_else(|| atual.as_ref().and_then(|a| a.get("pontos")?.as_i64()));
    let mut m = meta::agregar(&partidas, pontos.map(meta::faixa));
    if m.partidas < MIN_PARTIDAS_FAIXA {
        m = meta::agregar(&partidas, None);
    }
    let fichas: Vec<Value> = match estado.equipes.estado_fresco().await {
        Ok(e) => e
            .conta(Some(&nick))
            .and_then(|c| c.get("pokemons"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default(),
        Err(e) => {
            tracing::warn!(erro = %e, "pvp: estado de equipes");
            return erro(StatusCode::SERVICE_UNAVAILABLE, "coleção indisponível");
        }
    };
    if fichas.is_empty() {
        return erro(
            StatusCode::NOT_FOUND,
            "coleção da conta ainda não chegou (abra a conta no app)",
        );
    }
    let dex = persistencia::equipes::carregar_pokedex(&estado.db)
        .await
        .ok()
        .and_then(|v| Pokedex::de_valor(v).ok());
    let s = sugestao::sugerir(&fichas, &m, dex.as_ref());
    Json(json!({
        "nick": nick,
        "atual": atual,
        "sugestao": s,
        "comando": {"tipo": "pvp.formacao", "params": {
            "pokemonIds": s.time.iter().map(|e| e.id).collect::<Vec<_>>(),
            "casas": s.casas,
        }},
    }))
    .into_response()
}

pub async fn treinos(
    State(estado): State<Estado>,
    cliente: Option<Extension<Cliente>>,
    Query(p): Params,
) -> Response {
    let nick = match nick(cliente.as_ref(), &p) {
        Ok(n) => n,
        Err((st, msg)) => return erro(st, msg),
    };
    match persistencia::pvp::treinos(&estado.db, &nick, 50).await {
        Ok(v) => Json(json!({
            "nick": nick,
            "treinos": v.into_iter().map(|(em, d)| json!({"em": em, "resultado": d})).collect::<Vec<_>>(),
        }))
        .into_response(),
        Err(e) => banco(&e),
    }
}

pub async fn shiny_servidor(State(estado): State<Estado>, Query(p): Params) -> Response {
    match persistencia::pvp::shiny_servidor(&estado.db, agora_ms() - dias(&p, 7) * DIA_MS).await {
        Ok(v) => Json(shiny::resumir(&v)).into_response(),
        Err(e) => banco(&e),
    }
}

pub async fn fichas_raid(State(estado): State<Estado>) -> Response {
    match persistencia::pvp::fichas_raid(&estado.db).await {
        Ok(Some((em, d))) => Json(json!({"em": em, "fichas": d.get("fichas")})).into_response(),
        Ok(None) => erro(
            StatusCode::NOT_FOUND,
            "nenhuma ficha de raid ainda (abra a tela de Raid em uma conta)",
        ),
        Err(e) => banco(&e),
    }
}

/// `GET /api/comandos/fila?nick=`: os 100 comandos mais recentes (só leitura; não reserva).
pub async fn fila_comandos(
    State(estado): State<Estado>,
    cliente: Option<Extension<Cliente>>,
    Query(p): Params,
) -> Response {
    let conta = if p.get("nick").is_some_and(|n| !n.is_empty())
        || cliente
            .as_ref()
            .is_some_and(|Extension(c)| c.conta.is_some())
    {
        match nick(cliente.as_ref(), &p) {
            Ok(n) => Some(n),
            Err((st, msg)) => return erro(st, msg),
        }
    } else {
        None
    };
    match persistencia::comandos::fila(&estado.db, conta.as_deref(), 100).await {
        Ok(v) => Json(json!({ "comandos": v })).into_response(),
        Err(e) => banco(&e),
    }
}

/// `GET /api/equipes/colecao?nick=`: a coleção da conta como o app mandou (fichas).
pub async fn colecao(
    State(estado): State<Estado>,
    cliente: Option<Extension<Cliente>>,
    Query(p): Params,
) -> Response {
    let nick = match nick(cliente.as_ref(), &p) {
        Ok(n) => n,
        Err((st, msg)) => return erro(st, msg),
    };
    let motor = estado.equipes.motor().await.ok();
    match estado.equipes.estado_fresco().await {
        Ok(e) => {
            let c = e.conta(Some(&nick));
            let mut pokemons = c
                .and_then(|c| c.get("pokemons"))
                .cloned()
                .unwrap_or_else(|| json!([]));
            // `dex`: a espécie base para a imagem (formas especiais do jogo pelo nome).
            if let (Some(m), Some(ps)) = (motor.as_ref(), pokemons.as_array_mut()) {
                for p in ps {
                    let dex = p
                        .get("speciesId")
                        .and_then(Value::as_i64)
                        .and_then(|id| m.dex_de(id));
                    if let (Some(d), Some(o)) = (dex, p.as_object_mut()) {
                        o.insert("dex".into(), json!(d));
                    }
                }
            }
            Json(json!({
                "nick": nick,
                "trLv": c.and_then(|c| c.get("trLv")),
                "em": c.and_then(|c| c.get("em")),
                "pokemons": pokemons,
            }))
            .into_response()
        }
        Err(e) => {
            tracing::warn!(erro = %e, "colecao: estado de equipes");
            erro(StatusCode::SERVICE_UNAVAILABLE, "coleção indisponível")
        }
    }
}

/// `GET /api/pvp/painel?nick=`: elo atual, a semana (vitórias, derrotas, ganho e perda médios,
/// ritmo) e os cortes do ranking (posições 10, 100 e 500) desta semana e da anterior.
pub async fn painel(
    State(estado): State<Estado>,
    Query(p): Query<std::collections::HashMap<String, String>>,
) -> Response {
    let nick = p.get("nick").map(String::as_str).unwrap_or_default();
    let r = async {
        let atual = persistencia::pvp::estado_pvp(&estado.db, nick)
            .await?
            .map(|x| x.1);
        Ok::<_, sqlx::Error>(serde_json::json!({
            "atual": atual,
            "semana": persistencia::pvp::semana_pvp(&estado.db, nick).await?,
            "cortes": persistencia::pvp::cortes_pvp(&estado.db).await?,
        }))
    }
    .await;
    match r {
        Ok(v) => Json(v).into_response(),
        Err(e) => {
            tracing::warn!(erro = %e, "painel do pvp");
            (
                axum::http::StatusCode::SERVICE_UNAVAILABLE,
                Json(contrato::Erro::novo("banco indisponível")),
            )
                .into_response()
        }
    }
}
