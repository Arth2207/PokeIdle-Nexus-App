//! Dados das telas: lê a API do Nexus pelo comando `api_get` do app (o token fica no Rust) e
//! ajuda a tirar campos do JSON sem pânico.

use leptos::prelude::*;
use serde_json::Value;
use wasm_bindgen::JsValue;

use crate::{args, erro_texto, invoke, Estado};

/// Dentro do app (ponte do Tauri) ou como site (navegador)?
pub fn no_app() -> bool {
    web_sys::window()
        .and_then(|w| js_sys::Reflect::has(&w, &JsValue::from_str("__TAURI_INTERNALS__")).ok())
        .unwrap_or(false)
}

/// `GET` numa rota `/api/...` do Nexus. No app, pelo comando `api_get` (o token fica no Rust);
/// como site, `fetch` em `api/...` relativo à página (o Nginx põe o token e só deixa GET).
pub async fn api(url: &str, rota: &str) -> Result<Value, String> {
    if !no_app() {
        return api_site(rota).await;
    }
    let a = args(&[
        ("url", JsValue::from_str(url)),
        ("rota", JsValue::from_str(rota)),
    ]);
    let r = invoke("api_get", a).await.map_err(|e| erro_texto(&e))?;
    let texto = js_sys::JSON::stringify(&r)
        .ok()
        .and_then(|s| s.as_string())
        .unwrap_or_default();
    serde_json::from_str(&texto).map_err(|e| e.to_string())
}

async fn api_site(rota: &str) -> Result<Value, String> {
    let rel = format!("api{}", rota.strip_prefix("/api").unwrap_or(rota));
    let w = web_sys::window().ok_or("sem janela")?;
    let resp = wasm_bindgen_futures::JsFuture::from(w.fetch_with_str(&rel))
        .await
        .map_err(|e| erro_texto(&e))?;
    let resp: web_sys::Response =
        wasm_bindgen::JsCast::dyn_into(resp).map_err(|_| "resposta inválida")?;
    if !resp.ok() {
        return Err(format!("HTTP {}", resp.status()));
    }
    let texto = wasm_bindgen_futures::JsFuture::from(resp.text().map_err(|e| erro_texto(&e))?)
        .await
        .map_err(|e| erro_texto(&e))?
        .as_string()
        .unwrap_or_default();
    serde_json::from_str(&texto).map_err(|e| e.to_string())
}

/// Recurso que relê a rota quando `rota()` muda (e a cada `versao` nova).
pub fn recurso(
    e: Estado,
    versao: RwSignal<u32>,
    rota: impl Fn() -> Option<String> + 'static,
) -> LocalResource<Option<Result<Value, String>>> {
    LocalResource::new(move || {
        let url = e.url.get();
        let r = rota();
        versao.track();
        async move {
            match r {
                Some(r) => Some(api(&url, &r).await),
                None => None,
            }
        }
    })
}

pub fn txt(v: &Value, k: &str) -> String {
    match v.get(k) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Bool(b)) => if *b { "sim" } else { "não" }.to_owned(),
        _ => String::new(),
    }
}

pub fn num(v: &Value, k: &str) -> Option<f64> {
    v.get(k).and_then(Value::as_f64).filter(|x| x.is_finite())
}

pub fn lista<'a>(v: &'a Value, k: &str) -> &'a [Value] {
    v.get(k)
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

/// Número curto: 1.234, 12,3 mil, 4,5 mi, 1,2 bi.
pub fn curto(x: f64) -> String {
    let a = x.abs();
    let (v, suf) = if a >= 1e9 {
        (x / 1e9, " bi")
    } else if a >= 1e6 {
        (x / 1e6, " mi")
    } else if a >= 1e4 {
        (x / 1e3, " mil")
    } else {
        return inteiro(x);
    };
    format!("{}{suf}", format!("{v:.1}").replace('.', ","))
}

/// Inteiro com ponto de milhar.
pub fn inteiro(x: f64) -> String {
    let n = x.round() as i64;
    let s = n.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push('.');
        }
        out.push(c);
    }
    if n < 0 {
        format!("-{out}")
    } else {
        out
    }
}

/// Porcentagem com sinal: `+12%`.
pub fn pct(x: f64) -> String {
    format!("{:+.0}%", x * 100.0)
}

/// Sprite do PokeAPI (render 3D `home`, com shiny quando pedido).
pub fn sprite(especie: i64, shiny: bool) -> String {
    let base =
        "https://raw.githubusercontent.com/PokeAPI/sprites/master/sprites/pokemon/other/home";
    if shiny {
        format!("{base}/shiny/{especie}.png")
    } else {
        format!("{base}/{especie}.png")
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn numeros_em_portugues() {
        assert_eq!(inteiro(1234567.0), "1.234.567");
        assert_eq!(inteiro(-999.0), "-999");
        assert_eq!(curto(12_300.0), "12,3 mil");
        assert_eq!(curto(4_500_000.0), "4,5 mi");
        assert_eq!(pct(0.123), "+12%");
    }
}
