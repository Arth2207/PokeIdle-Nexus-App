//! Liga o relay do `a2-nucleo` (varredura do mercado etc.) ao app: os pedidos saem pela janela
//! do jogo da conta dona (a primeira da lista) e o que fica pronto vai ao servidor por HTTP, com
//! o mesmo token do app.

use a2_nucleo::jogo::rotulo_da_conta;
use a2_nucleo::relay::{base_http, Entrega, Relay};
use a2_nucleo::{ArmazemToken, ArquivoToken};
use serde_json::Value;
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Manager};
use tokio::sync::{mpsc, watch};

#[derive(Default)]
pub struct EstadoRelay {
    relay: Mutex<Option<Relay>>,
    dono: Mutex<Option<String>>,
}

fn travar<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl EstadoRelay {
    /// Resposta do jogo repassada pelo coletor (`{t:'mkt', m}`); só a conta dona alimenta.
    pub fn receber(&self, conta: &str, msg: &Value, agora: i64) {
        if travar(&self.dono).as_deref() != Some(conta) {
            return;
        }
        if let (Some(r), Some(m)) = (travar(&self.relay).as_mut(), msg.get("m")) {
            r.receber(m, agora);
        }
    }
}

/// (Re)inicia o relay: a cada segundo pede ao jogo o que for a hora e entrega o que estiver pronto.
pub fn iniciar(
    app: &AppHandle,
    url: &str,
    dono: Option<String>,
    mut parar: watch::Receiver<bool>,
    avisos: mpsc::UnboundedSender<String>,
) {
    let est = app.state::<crate::Estado>();
    *travar(&est.relay.relay) = Some(Relay::padrao());
    *travar(&est.relay.dono) = dono.clone();
    let (Some(dono), Some(base)) = (dono, base_http(url)) else {
        return;
    };
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let cliente = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build();
        let Ok(cliente) = cliente else {
            let _ = avisos.send("relay: cliente HTTP indisponível".into());
            return;
        };
        let mut tique = tokio::time::interval(Duration::from_secs(1));
        loop {
            tokio::select! {
                _ = tique.tick() => {}
                _ = parar.changed() => {}
            }
            if *parar.borrow() {
                return;
            }
            let agora = crate::agora_ms();
            let (pedido, entregas) = {
                let est = app.state::<crate::Estado>();
                let mut g = travar(&est.relay.relay);
                match g.as_mut() {
                    Some(r) => (r.pedir(agora), r.entregar()),
                    None => return,
                }
            };
            if let Some(p) = pedido {
                if let Some(w) = app.get_webview(&rotulo_da_conta(&dono)) {
                    let _ = w.eval(format!("window.__hdxCmd&&window.__hdxCmd({p})"));
                }
            }
            for e in entregas {
                let (rota, corpo) = e.requisicao();
                let msg = match (enviar(&cliente, &base, rota, &corpo).await, &e) {
                    (Ok(()), Entrega::Ofertas(o)) => {
                        format!("Bolsa: {} ofertas do mercado enviadas", o.len())
                    }
                    (Err(er), _) => format!("relay: {rota} falhou: {er}"),
                };
                let _ = avisos.send(msg);
            }
        }
    });
}

async fn enviar(c: &reqwest::Client, base: &str, rota: &str, corpo: &Value) -> Result<(), String> {
    let token = ArquivoToken::padrao("nexus")
        .and_then(|a| a.ler())
        .map_err(|e| e.to_string())?
        .ok_or("token ausente")?;
    let r = c
        .post(format!("{base}{rota}"))
        .bearer_auth(token)
        .json(corpo)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if r.status().is_success() {
        Ok(())
    } else {
        Err(format!("HTTP {}", r.status().as_u16()))
    }
}
