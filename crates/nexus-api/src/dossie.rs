//! Ponte para o Dossiê: o app manda as vendas do mercado com o token dele e o servidor
//! repassa ao Dossiê (`POST /api/dossie/vendas`) com a chave que só o servidor conhece.

use std::time::Duration;

use serde_json::Value;

use crate::configuracao::{Configuracao, Segredo};

/// Vendas por requisição (o Dossiê aceita até 3000).
pub const MAX_VENDAS: usize = 3000;

pub struct PonteDossie {
    url: String,
    chave: Segredo,
    http: reqwest::Client,
}

/// Resposta do Dossiê: status HTTP e corpo JSON (ou nulo).
pub struct Resposta {
    pub status: u16,
    pub corpo: Value,
}

impl PonteDossie {
    /// Só existe com URL e chave configuradas.
    pub fn nova(cfg: &Configuracao) -> Option<Self> {
        let url = cfg.dossie_url.as_deref()?.trim_end_matches('/').to_owned();
        let chave = cfg.dossie_chave.clone()?;
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .build()
            .ok()?;
        Some(Self { url, chave, http })
    }

    pub async fn vendas(&self, corpo: &Value) -> Result<Resposta, reqwest::Error> {
        let r = self
            .http
            .post(format!("{}/api/dossie/vendas", self.url))
            .header("x-chave", self.chave.expor())
            .json(corpo)
            .send()
            .await?;
        let status = r.status().as_u16();
        let corpo = r.json().await.unwrap_or(Value::Null);
        Ok(Resposta { status, corpo })
    }
}
