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

/// Capturas da conta e Top Catch local, com reset domingo 00h (Brasília).
pub async fn capturas(
    State(estado): State<Estado>,
    cliente: Option<Extension<Cliente>>,
    Query(p): Query<std::collections::HashMap<String, String>>,
) -> Response {
    let restrito = cliente.as_ref().and_then(|Extension(c)| c.conta.as_deref());
    let conta = p
        .get("conta")
        .map(String::as_str)
        .filter(|c| !c.trim().is_empty())
        .or(restrito);
    let Some(conta) = conta else {
        return erro(StatusCode::BAD_REQUEST, "informe conta");
    };
    if restrito.is_some_and(|r| !r.eq_ignore_ascii_case(conta)) {
        return erro(
            StatusCode::FORBIDDEN,
            "este token só acessa a própria conta",
        );
    }
    match persistencia::cacada::capturas(&estado.db, conta, crate::bolsa::agora_ms()).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => {
            tracing::warn!(erro = %e, "capturas da semana");
            erro(StatusCode::SERVICE_UNAVAILABLE, "banco indisponível")
        }
    }
}

/// Relatório de Outlands: dados atuais, comparação por contexto e valor de inventário.
pub async fn outlands(
    State(estado): State<Estado>,
    cliente: Option<Extension<Cliente>>,
    Query(p): Query<std::collections::HashMap<String, String>>,
) -> Response {
    let restrito = cliente.as_ref().and_then(|Extension(c)| c.conta.as_deref());
    let Some(conta) = p
        .get("conta")
        .map(String::as_str)
        .filter(|c| !c.trim().is_empty())
        .or(restrito)
    else {
        return erro(StatusCode::BAD_REQUEST, "informe conta");
    };
    if restrito.is_some_and(|r| !r.eq_ignore_ascii_case(conta)) {
        return erro(
            StatusCode::FORBIDDEN,
            "este token só acessa a própria conta",
        );
    }
    let taxa = match p.get("taxa") {
        None => None,
        Some(t) => match t.parse::<f64>() {
            Ok(n) if n.is_finite() && (0.0..=1.0).contains(&n) => Some(n),
            _ => return erro(StatusCode::BAD_REQUEST, "taxa deve estar entre 0 e 1"),
        },
    };
    let ctx = match persistencia::outlands::contexto(&estado.db, conta).await {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(%e,"contexto de Outlands");
            return erro(StatusCode::SERVICE_UNAVAILABLE, "banco indisponível");
        }
    };
    let w = &ctx["regras"];
    let u = &ctx["janela"]["ultimo"];
    let tiers = nucleo::outlands::tiers(w);
    let atual = nucleo::outlands::atual(w, u);
    let nivel = nucleo::motor::jogo::num(u.get("trLv")).unwrap_or(0.0);
    let proxima = tiers
        .iter()
        .filter(|t| t.nivel > nivel)
        .min_by(|a, b| a.nivel.total_cmp(&b.nivel));
    let regra = w["outlandRegrasId"].as_str();
    let medidas = match persistencia::outlands::medidas(&estado.db, conta, regra).await {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(%e,"medições de Outlands");
            return erro(StatusCode::SERVICE_UNAVAILABLE, "medições indisponíveis");
        }
    };
    let agora = crate::bolsa::agora_ms();
    let mut cotacoes = Vec::new();
    if let Ok(s) = estado.bolsa.snapshot().await {
        for t in s.mercado.tickers.iter().filter(|t| {
            !t.pokemon
                && t.moeda.como_str() == "orb"
                && t.a.n >= 3
                && t.a.ultima_venda >= agora - 7 * 86_400_000
        }) {
            if let Some(preco) = t.a.base.filter(|p| p.is_finite() && *p > 0.0) {
                cotacoes.push(json!({"id":t.item_id,"nome":t.nome,"preco":preco,"liquidez":t.a.liquidez.como_str(),"ultima_venda":t.a.ultima_venda}));
            }
        }
    }
    let ls = medidas["linhas"].as_array().cloned().unwrap_or_default();
    let ps = medidas["periodos"].as_array().cloned().unwrap_or_default();
    let out_slugs: std::collections::HashSet<&str> = ctx["hunts"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|h| h["area"] == "outland")
        .filter_map(|h| h["slug"].as_str())
        .collect();
    let mesmo = |a: &Value, b: &Value| {
        ["slug", "tier", "regra", "ativo", "ativo_nivel", "loot"]
            .iter()
            .all(|k| a[*k] == b[*k])
    };
    let mut linhas = Vec::new();
    let mut sem_preco = 0.0;
    for k in ls.iter().filter(|l| {
        l["tipo"] == "kill"
            && l["tier"].is_string()
            && l["slug"].as_str().is_some_and(|s| out_slugs.contains(s))
    }) {
        let horas: f64 = ps
            .iter()
            .filter(|p| mesmo(p, k))
            .filter_map(|p| p["horas"].as_f64())
            .sum();
        let kills = k["qtd"].as_f64().unwrap_or(0.0);
        let kph = (horas >= 0.25).then_some(kills / horas.max(1e-9));
        let mut valor = 0.0;
        let mut itens = Vec::new();
        let mut cotados = 0;
        for d in ls
            .iter()
            .filter(|d| d["tipo"] == "drop" && mesmo(d, k) && d["especie"] == k["especie"])
        {
            let q = d["qtd"].as_f64().unwrap_or(0.0);
            let cotacao = cotacoes.iter().find(|c| {
                (!d["item_id"].is_null() && c["id"] == d["item_id"])
                    || c["nome"]
                        .as_str()
                        .zip(d["item"].as_str())
                        .is_some_and(|(a, b)| a.eq_ignore_ascii_case(b))
            });
            if let Some(c) = cotacao {
                valor += q * c["preco"].as_f64().unwrap_or(0.0);
                cotados += 1;
            } else {
                sem_preco += q;
            }
            itens.push(json!({"nome":d["item"],"qtd":q,"cotacao":cotacao}));
        }
        let tier = k["tier"]
            .as_str()
            .and_then(|s| s.parse::<i64>().ok())
            .and_then(|id| tiers.iter().find(|t| t.tier == id));
        let atualizada = k["regra"].as_str() == regra && regra.is_some();
        let tokens_h = kph
            .zip(tier)
            .zip(nucleo::outlands::chance_token(w))
            .filter(|_| atualizada)
            .map(|((kh, t), c)| kh * t.mult * c);
        linhas.push(json!({"especie":k["especie"],"tier":tier,"regra":k["regra"],"regra_atual":atualizada,
            "ativo":k["ativo"],"ativo_nivel":k["ativo_nivel"],"loot":k["loot"],"horas":horas,"kills":kills,"kph":kph,
            "tokens_h_sem_bonus":tokens_h,"valor_bruto":if cotados>0{Some(valor)}else{None},
            "valor_pos_comissao":taxa.filter(|_|cotados>0).map(|t|valor*(1.0-t)),
            "gemas_h_parcial":kph.filter(|_|cotados>0).map(|_|valor/horas),"itens":itens}));
    }
    // Vendas realizadas da conta: não presume que vieram dos drops desta sessão.
    let nick = ctx["janela"]["nick"].as_str().unwrap_or(conta);
    let recebido = match persistencia::pvp::extrato_mercado(&estado.db, agora - 86_400_000).await {
        Ok(v) => {
            let mut n = 0;
            let mut soma = 0.0;
            for (c, lado, l) in v {
                if (c.eq_ignore_ascii_case(conta) || c.eq_ignore_ascii_case(nick))
                    && lado == "venda"
                    && l["moeda"] == "orb"
                {
                    if let Some(valor) = l["valor"].as_f64() {
                        n += 1;
                        soma += valor;
                    }
                }
            }
            if n > 0 {
                Some(soma)
            } else {
                None
            }
        }
        Err(_) => None,
    };
    Json(json!({"conta":conta,"agora":agora,"regras_em":ctx["em"],"regra_id":regra,"tiers":tiers,"atual":atual,
        "nivel_treinador":nivel,"proxima":proxima,"chance_token":nucleo::outlands::chance_token(w),
        "linhas":linhas,"taxa":taxa,"abates_sem_contexto":medidas["abates_sem_contexto"],"itens_sem_cotacao":sem_preco,
        "vendas_recebidas_24h":recebido,"cotacoes":cotacoes,"custos_coins":Value::Null,
        "nota":"Valor de drops é inventário estimado, não venda realizada. Previsão de tokens sem bônus adicionais; custos em Coins não convertidos em gemas."})).into_response()
}
