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
use sha2::Digest;

use crate::auth::Cliente;
use crate::bolsa::agora_ms;
use crate::equipes::ErroServico;
use crate::estado::Estado;

type Params = Query<HashMap<String, String>>;

/// Tamanho máximo dos eventos `batalha` de uma tentativa de boss.
const LIMITE_EVENTOS: usize = 2 * 1024 * 1024;

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

/// `GET /api/equipes/resumo?nick=`: o plano de cada boss num pedido só (grade do Combate):
/// os dados do boss, se a conta alcança e já venceu, e a cobertura da melhor equipe da conta
/// e da ideal (dano total ÷ HP do boss).
pub async fn resumo(
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
        let b = equipes::resposta_bosses(&motor, &est, nick.as_deref());
        let lista: Vec<Value> = b
            .get("bosses")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|boss| {
                let key = boss.get("key").and_then(Value::as_str).unwrap_or_default();
                let plano =
                    equipes::resposta_plano(&motor, &est, nick.as_deref(), Some(key), None).ok();
                let c = |k: &str| {
                    plano
                        .as_ref()
                        .and_then(|p| p.pointer(&format!("/{k}/cobertura")).cloned())
                        .unwrap_or(Value::Null)
                };
                let mut o = boss.clone();
                if let Value::Object(m) = &mut o {
                    m.insert("minha".into(), c("minha"));
                    m.insert("ideal".into(), c("ideal"));
                }
                o
            })
            .collect();
        serde_json::json!({"bosses": lista, "conta": b.get("conta")})
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

/// `GET /api/equipes/lab?nick=&boss=&ids=1,2,3`: avalia a equipe montada à mão.
pub async fn lab(
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
        let ids: Vec<&str> = p
            .get("ids")
            .map(|t| {
                t.split(',')
                    .map(str::trim)
                    .filter(|x| !x.is_empty())
                    .take(6)
                    .collect()
            })
            .unwrap_or_default();
        let boss = p.get("boss").map(String::as_str).unwrap_or_default();
        equipes::resposta_lab(&motor, &est, nick.as_deref(), boss, &ids)
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
    let atual = match estado.equipes.estado_fresco().await {
        Ok(e) => e,
        Err(e) => return indisponivel(&e),
    };
    let mut novo = (*atual).clone();
    // A lista de bosses é global: um token preso a uma conta não a substitui.
    let mut corpo = corpo;
    if cliente
        .as_ref()
        .is_some_and(|Extension(c)| c.conta.is_some())
    {
        if let Value::Object(o) = &mut corpo {
            o.remove("bosses");
        }
    }
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
        Situacao::Erro(e) if e.starts_with("servidor ocupado") => erro(503, &e),
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

/// O instante `em` do corpo: inteiro, ou número com fração (arredondado). Limitado ao agora,
/// para não passar na frente das tentativas reais.
fn em_do_corpo(c: &Value, agora: i64) -> Option<i64> {
    let n = c.get("em")?.as_number()?;
    let em = n.as_i64().or_else(|| {
        n.as_f64()
            .filter(|f| f.is_finite())
            .map(|f| f.round() as i64)
    })?;
    Some(em.min(agora))
}

/// A chave de idempotência da tentativa: sha256 de `conta|chave_do_cliente` quando o cliente
/// manda `chave` (sem truncar), ou de `conta|boss|em` se houver conta e `em`. `conta` é a do
/// token ou a do corpo. Sem nenhuma das duas, não há chave (sempre grava). O `em` é o mesmo
/// (já limitado) que vai na linha.
fn chave_da_tentativa(
    c: &Value,
    conta: Option<&str>,
    boss: &str,
    em: Option<i64>,
) -> Option<String> {
    let conta = conta
        .map(|n| n.trim().to_lowercase())
        .filter(|n| !n.is_empty());
    let texto = if let Some(k) = c
        .get("chave")
        .and_then(Value::as_str)
        .filter(|k| !k.is_empty())
    {
        format!("{}|{k}", conta.unwrap_or_default())
    } else {
        format!("{}|{boss}|{}", conta?, em?)
    };
    Some(hex::encode(sha2::Sha256::digest(texto.as_bytes())))
}

/// `POST /api/boss/tentativa` (rota nova): grava uma tentativa de boss para calibrar o
/// modelo do jogo. Só com `NEXUS_ESCRITA_EQUIPES=true`; quem envia é o relay (fase 3).
///
/// Corpo: `{ nick, boss, nivelBoss?, tiposBoss?, venceu, duracaoS, equipe: [fichas],
/// danoCausado?, danoRecebido?, eventos?, em? }`.
pub async fn tentativa(
    State(estado): State<Estado>,
    cliente: Option<Extension<Cliente>>,
    Json(c): Json<Value>,
) -> Response {
    if !estado.escrita_equipes {
        return erro(
            503,
            "escrita de equipes desligada neste servidor (NEXUS_ESCRITA_EQUIPES)",
        );
    }
    let Ok(nick) = nick_de(cliente.as_ref(), c.get("nick").and_then(Value::as_str)) else {
        return proibido();
    };
    let Some(boss) = c
        .get("boss")
        .and_then(Value::as_str)
        .map(equipes::norm)
        .filter(|b| !b.is_empty())
    else {
        return erro(400, "boss obrigatório");
    };
    let Some(fichas) = c
        .get("equipe")
        .and_then(Value::as_array)
        .filter(|e| (1..=5).contains(&e.len()) && e.iter().all(Value::is_object))
    else {
        return erro(400, "equipe precisa de 1 a 5 fichas");
    };
    let Some(venceu) = c.get("venceu").and_then(Value::as_bool) else {
        return erro(400, "venceu obrigatório");
    };
    let duracao = c
        .get("duracaoS")
        .and_then(Value::as_f64)
        .filter(|d| d.is_finite() && *d > 0.0);
    let Some(duracao) = duracao else {
        return erro(400, "duracaoS precisa ser positiva");
    };
    // Nível e tipos do boss: os enviados, ou os da lista viva.
    let (motor, est) = dados!(estado);
    let da_lista = motor
        .lista_bosses(&est.bosses)
        .into_iter()
        .find(|b| b.key == boss);
    // Boss conhecido: vale o nível da lista do jogo (o enviado só conta fora da lista).
    let nivel = da_lista
        .as_ref()
        .map(|b| b.team_level)
        .or(c.get("nivelBoss").and_then(Value::as_f64));
    let Some(nivel) = nivel.filter(|n| *n > 0.0) else {
        return erro(400, "nivelBoss desconhecido para este boss");
    };
    let tipos: Vec<String> = match c.get("tiposBoss").and_then(Value::as_array) {
        Some(t) => t
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_uppercase)
            .collect(),
        None => da_lista.map(|b| b.tipos).unwrap_or_default(),
    };
    let agora = agora_ms();
    let em_pedido = em_do_corpo(&c, agora);
    let chave = chave_da_tentativa(&c, nick.as_deref(), &boss, em_pedido);
    let linha = persistencia::equipes::LinhaTentativa {
        em: em_pedido.unwrap_or(agora),
        conta: nick,
        boss,
        nivel_boss: nivel,
        tipos_boss: tipos,
        venceu,
        duracao_s: duracao,
        equipe: Value::Array(fichas.clone()),
        dano_causado: c.get("danoCausado").and_then(Value::as_f64),
        dano_recebido: c.get("danoRecebido").and_then(Value::as_f64),
        eventos: c.get("eventos").cloned().filter(|v| !v.is_null()),
        chave,
    };
    if linha
        .eventos
        .as_ref()
        .is_some_and(|e| e.to_string().len() > LIMITE_EVENTOS)
    {
        return erro(413, "eventos acima de 2 MB");
    }
    match persistencia::equipes::registrar_tentativa(&estado.db, &linha).await {
        Ok(gravada) => {
            if !gravada {
                metrics::counter!("nexus_boss_tentativas_duplicadas_total").increment(1);
            }
            Json(serde_json::json!({ "ok": true, "duplicada": !gravada })).into_response()
        }
        Err(e) => {
            tracing::warn!(erro = %e, "tentativa de boss");
            erro(503, "banco indisponível")
        }
    }
}

#[cfg(test)]
mod testes_chave {
    use serde_json::json;

    use super::*;

    #[test]
    fn chave_inclui_a_conta_e_nao_trunca() {
        let longa = "k".repeat(300);
        let c = json!({"chave": longa});
        let a = chave_da_tentativa(&c, Some("Ana"), "b", None);
        assert_ne!(a, chave_da_tentativa(&c, Some("bia"), "b", None));
        assert_eq!(a, chave_da_tentativa(&c, Some(" ana "), "b", None));
        let c2 = json!({"chave": format!("{longa}x")});
        assert_ne!(a, chave_da_tentativa(&c2, Some("ana"), "b", None));
        // Sem chave e sem `em`: sem chave derivada.
        assert_eq!(chave_da_tentativa(&json!({}), Some("ana"), "b", None), None);
        assert!(chave_da_tentativa(&json!({}), Some("ana"), "b", Some(5)).is_some());
    }

    #[test]
    fn em_float_e_arredondado_e_limitado() {
        assert_eq!(em_do_corpo(&json!({"em": 10.6}), 100), Some(11));
        assert_eq!(em_do_corpo(&json!({"em": 1000}), 100), Some(100));
        assert_eq!(em_do_corpo(&json!({}), 100), None);
    }
}
