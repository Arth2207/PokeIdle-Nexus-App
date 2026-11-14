//! Liga o relay do `a2-nucleo` ao app: os pedidos ao jogo (mercado, vendas, médias) saem pela
//! janela da conta dona (a primeira da lista); o estado de todas as contas alimenta equipe,
//! hunt, inventário e raros. O que fica pronto vai ao servidor por HTTP, com o token do app,
//! por uma fila que junta lotes e tenta de novo o que falhar.

use a2_nucleo::jogo::rotulo_da_conta;
use a2_nucleo::relay::mercado::pokemon::alvos_da_resposta;
use a2_nucleo::relay::saida::Saida;
use a2_nucleo::relay::{base_http, Relay};
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

    /// Estado do jogo ou linhas do HUD de qualquer conta.
    pub fn receber_conta(&self, conta: &str, msg: &Value, agora: i64) {
        if let Some(r) = travar(&self.relay).as_mut() {
            r.receber_conta(conta, msg, agora);
        }
    }
}

/// Arquivo onde um fluxo guarda o estado dele (ex.: o diário de contagens).
fn arquivo_estado(app: &AppHandle, nome: &str) -> Option<std::path::PathBuf> {
    let dir = app.path().app_data_dir().ok()?;
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join(format!("relay-{nome}.json")))
}

fn restaurar(app: &AppHandle, r: &mut Relay) {
    for nome in ["contas", "treino"] {
        let Some(c) = arquivo_estado(app, nome) else {
            continue;
        };
        if let Some(v) = std::fs::read(&c)
            .ok()
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        {
            r.restaurar(nome, &v);
        }
    }
}

fn salvar(app: &AppHandle, estados: Vec<(&'static str, Value)>) {
    for (nome, v) in estados {
        if let Some(c) = arquivo_estado(app, nome) {
            let tmp = c.with_extension("tmp");
            if std::fs::write(&tmp, v.to_string()).is_ok() {
                let _ = std::fs::rename(&tmp, &c);
            }
        }
    }
}

/// (Re)inicia o relay: a cada segundo pede ao jogo o que for a hora e entrega o que estiver pronto.
pub fn iniciar(
    app: &AppHandle,
    url: &str,
    dono: Option<String>,
    oficial: bool,
    aprender: bool,
    mut parar: watch::Receiver<bool>,
    avisos: mpsc::UnboundedSender<String>,
) {
    let est = app.state::<crate::Estado>();
    let mut novo = Relay::padrao(oficial, dono.as_deref().unwrap_or(""), aprender);
    restaurar(app, &mut novo);
    *travar(&est.relay.relay) = Some(novo);
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
        // Alvos de coleta: a cada 30 min (10 min enquanto o servidor indicar falta de espécies).
        let mut alvos_proximo = 0i64;
        let mut saida = Saida::nova();
        let mut salvo_em = 0i64;
        loop {
            tokio::select! {
                _ = tique.tick() => {}
                _ = parar.changed() => {}
            }
            if *parar.borrow() {
                return;
            }
            let agora = crate::agora_ms();
            if agora >= alvos_proximo {
                alvos_proximo = agora + 5 * 60_000;
                let rota = format!("/api/bolsa/alvos-coleta?nick={}", codificar(&dono));
                match buscar(&cliente, &base, &rota).await {
                    Ok(v) => {
                        let (alvos, faltas) = alvos_da_resposta(&v);
                        alvos_proximo = agora + if faltas { 10 } else { 30 } * 60_000;
                        let est = app.state::<crate::Estado>();
                        if let Some(r) = travar(&est.relay.relay).as_mut() {
                            r.definir_alvos(&alvos, agora);
                        }
                        let _ = avisos.send(format!(
                            "Mercado: {} espécies alvo do servidor",
                            alvos.len()
                        ));
                    }
                    Err(er) => {
                        let _ = avisos.send(format!("relay: alvos de coleta falharam: {er}"));
                    }
                }
            }
            // Leituras no servidor que os fluxos pediram (ex.: meta do PvP para o treino).
            let consultas = {
                let est = app.state::<crate::Estado>();
                let mut g = travar(&est.relay.relay);
                g.as_mut().map(|r| r.consultas(agora)).unwrap_or_default()
            };
            for (nome, rota) in consultas {
                match buscar(&cliente, &base, &rota).await {
                    Ok(v) => {
                        let est = app.state::<crate::Estado>();
                        if let Some(r) = travar(&est.relay.relay).as_mut() {
                            r.responder(nome, &rota, &v, agora);
                        };
                    }
                    Err(er) => {
                        let _ = avisos.send(format!("relay: {rota} falhou: {er}"));
                    }
                }
            }
            let (pedido, entregas, estados) = {
                let est = app.state::<crate::Estado>();
                let mut g = travar(&est.relay.relay);
                match g.as_mut() {
                    Some(r) => {
                        let estados = if agora - salvo_em >= 60_000 {
                            salvo_em = agora;
                            r.salvar()
                        } else {
                            Vec::new()
                        };
                        (r.pedir(agora), r.entregar(agora), estados)
                    }
                    None => return,
                }
            };
            salvar(&app, estados);
            if let Some(mut p) = pedido {
                // Pedido de uma conta (ex.: treino de PvP) vai para a janela dela; o resto, à dona.
                let alvo = p
                    .as_object_mut()
                    .and_then(|o| o.remove("_conta"))
                    .and_then(|c| c.as_str().map(str::to_owned))
                    .unwrap_or_else(|| dono.clone());
                if let Some(w) = app.get_webview(&rotulo_da_conta(&alvo)) {
                    let _ = w.eval(format!("window.__hdxCmd&&window.__hdxCmd({p})"));
                }
            }
            for e in entregas {
                saida.guardar(e, agora);
            }
            // Um envio por segundo; a fila guarda o resto (e o que falhar) para depois.
            let Some(e) = saida.proxima(agora).cloned() else {
                continue;
            };
            let (rota, corpo) = e.requisicao();
            match enviar(&cliente, &base, rota, &corpo).await {
                Ok(()) => {
                    saida.ok();
                    let _ = avisos.send(e.resumo());
                }
                Err(er) => {
                    saida.falhou(agora, er.descartar);
                    let _ = avisos.send(format!(
                        "relay: {rota} falhou: {} ({} na fila)",
                        er.msg,
                        saida.len()
                    ));
                }
            }
        }
    });
}

/// Codifica o nick para a query (letras, números e `-_.~` passam; o resto vira `%XX`).
fn codificar(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn token() -> Result<String, String> {
    ArquivoToken::padrao("nexus")
        .and_then(|a| a.ler())
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "token ausente".to_owned())
}

async fn buscar(c: &reqwest::Client, base: &str, rota: &str) -> Result<Value, String> {
    let r = c
        .get(format!("{base}{rota}"))
        .bearer_auth(token()?)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !r.status().is_success() {
        return Err(format!("HTTP {}", r.status().as_u16()));
    }
    r.json().await.map_err(|e| e.to_string())
}

/// Falha de envio. `descartar`: o servidor recusou o pedido (4xx): repetir não adianta.
struct FalhaEnvio {
    msg: String,
    descartar: bool,
}

async fn enviar(
    c: &reqwest::Client,
    base: &str,
    rota: &str,
    corpo: &Value,
) -> Result<(), FalhaEnvio> {
    let falha = |msg: String| FalhaEnvio {
        msg,
        descartar: false,
    };
    let r = c
        .post(format!("{base}{rota}"))
        .bearer_auth(token().map_err(falha)?)
        .json(corpo)
        .send()
        .await
        .map_err(|e| falha(e.to_string()))?;
    let st = r.status();
    if st.is_success() {
        return Ok(());
    }
    // 408 e 429 são "tente mais tarde"; os outros 4xx não mudam tentando de novo.
    let descartar = st.is_client_error() && st.as_u16() != 408 && st.as_u16() != 429;
    Err(FalhaEnvio {
        msg: format!("HTTP {}", st.as_u16()),
        descartar,
    })
}
