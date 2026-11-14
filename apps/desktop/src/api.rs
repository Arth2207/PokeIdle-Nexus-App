//! Leitura da API do Nexus para as telas (Leptos): o token fica no Rust, a página só pede a
//! rota. Só `GET` em `/api/...`, sem `..`, com o endereço configurado do app.

use std::sync::OnceLock;
use std::time::Duration;

use a2_nucleo::relay::base_http;
use a2_nucleo::{ArmazemToken, ArquivoToken};
use serde_json::Value;

fn cliente() -> Option<&'static reqwest::Client> {
    static C: OnceLock<Option<reqwest::Client>> = OnceLock::new();
    C.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .build()
            .ok()
    })
    .as_ref()
}

/// Rota aceita: começa em `/api/`, sem `..` nem esquema.
pub fn rota_valida(rota: &str) -> bool {
    rota.starts_with("/api/") && !rota.contains("..") && !rota.contains("://") && rota.len() < 512
}

#[tauri::command]
pub async fn api_get(url: String, rota: String) -> Result<Value, String> {
    if !rota_valida(&rota) {
        return Err("rota inválida".into());
    }
    let base = base_http(url.trim()).ok_or("endereço do Nexus inválido (wss://…/ws)")?;
    let token = ArquivoToken::padrao("nexus")
        .and_then(|a| a.ler())
        .map_err(|e| e.to_string())?
        .ok_or("nenhum token salvo neste PC")?;
    let c = cliente().ok_or("cliente HTTP indisponível")?;
    let r = c
        .get(format!("{base}{rota}"))
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| format!("Nexus fora do ar: {e}"))?;
    let status = r.status();
    let corpo: Value = r.json().await.unwrap_or(Value::Null);
    if status.is_success() {
        Ok(corpo)
    } else {
        let msg = corpo.get("erro").and_then(Value::as_str).unwrap_or("erro");
        Err(format!("HTTP {}: {msg}", status.as_u16()))
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn so_rotas_da_api() {
        assert!(rota_valida("/api/pvp/meta?faixa=400"));
        assert!(!rota_valida("/ingest"));
        assert!(!rota_valida("/api/../health"));
        assert!(!rota_valida("/api/x?u=https://mal"));
    }
}
