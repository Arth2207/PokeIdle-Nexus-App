//! A tabela pública de presença do Tracker, da última página guardada no Nexus.

use std::collections::HashMap;

use axum::http::{header, HeaderValue};
use axum::response::Response;
use chrono::{DateTime, Utc};
use serde_json::{json, Value};

use super::{calc, enviar, Dossie, Erro, Pedido};

pub async fn presenca(d: &Dossie, p: &Pedido) -> Result<Response, Erro> {
    if !d.limites.ok(&p.ip, 60) {
        return Ok(enviar(&json!({"erro":"muitas requisições"}), 429));
    }
    let captura: Option<(DateTime<Utc>, Value)> = sqlx::query_as(
        "SELECT em, dados #> '{estrutura,tabelas,0}' FROM nexus.evento
           WHERE tipo = 'tracker.captura'
             AND rtrim(split_part(dados->>'url', '?', 1), '/') = 'https://pokeidle.io/tracker/meta'
             AND dados #>> '{estrutura,tabelas,0,0,1}' = 'PRESENÇA'
           ORDER BY em DESC LIMIT 1",
    )
    .fetch_optional(&d.db)
    .await
    .map_err(|e| {
        tracing::warn!(erro = %e, "Tracker PvP indisponível no Dossiê");
        Erro::novo(503, "Tracker PvP indisponível")
    })?;
    let Some((em, tabela)) = captura else {
        return Ok(enviar(&json!({"especies":[]}), 200));
    };
    let especies = projetar(&tabela, &calc::dados()?.catalogo);
    let mut r = enviar(&json!({"em":em.to_rfc3339(), "especies":especies}), 200);
    r.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=120"),
    );
    Ok(r)
}

fn projetar(tabela: &Value, catalogo: &Value) -> Vec<Value> {
    let ids: HashMap<_, _> = catalogo
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|sp| {
            Some((
                sp.get("n")?.as_str()?.to_lowercase(),
                sp.get("d")?.as_i64()?,
            ))
        })
        .collect();
    tabela
        .as_array()
        .into_iter()
        .flatten()
        .skip(1)
        .filter_map(|linha| {
            let nome = linha
                .get(0)?
                .as_str()?
                .lines()
                .next()?
                .trim()
                .to_lowercase();
            let id = ids.get(&nome)?;
            let pct: f64 = linha
                .get(1)?
                .as_str()?
                .trim()
                .strip_suffix('%')?
                .trim()
                .replace(',', ".")
                .parse()
                .ok()?;
            (0.0..=100.0)
                .contains(&pct)
                .then(|| json!({"especie":id, "presenca":pct}))
        })
        .collect()
}
