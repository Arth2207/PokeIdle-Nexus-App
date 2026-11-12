//! Aviso opcional no Discord (`NEXUS_DISCORD_WEBHOOK`, só com `NEXUS_MELHORIAS`).
//!
//! As rotas só enfileiram o texto (`try_send`, nunca espera). Uma task própria envia ao webhook
//! com timeout de 5 s. Fila cheia: a mensagem é descartada e contada. A URL é segredo: nunca
//! entra em log (erros do `reqwest` são logados sem URL).

use std::time::Duration;

use serde_json::{json, Value};
use tokio::sync::mpsc;

/// Mensagens pendentes. Passou disso, descarta.
const CAPACIDADE: usize = 64;
const TIMEOUT: Duration = Duration::from_secs(5);
/// Tentativas extras depois de um `429`.
const REPETICOES_429: u32 = 2;
/// Espera máxima honrada de um `retry_after` (o resto é cortado).
const ESPERA_MAXIMA: Duration = Duration::from_secs(10);
/// O Discord aceita até 2000 caracteres; ficamos bem abaixo.
const MAX_CHARS: usize = 300;

#[derive(Clone)]
pub struct Webhook {
    tx: mpsc::Sender<String>,
}

impl Webhook {
    /// Sobe a task de envio. `None` se a URL for vazia, o cliente HTTP não montar ou não houver
    /// runtime do tokio.
    pub fn iniciar(url: &str) -> Option<Self> {
        Self::iniciar_com(url, CAPACIDADE)
    }

    fn iniciar_com(url: &str, capacidade: usize) -> Option<Self> {
        let url = url.trim();
        if url.is_empty() {
            return None;
        }
        let rt = tokio::runtime::Handle::try_current().ok()?;
        // Idempotente: se já houver provedor instalado, o erro é ignorado.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let http = reqwest::Client::builder().timeout(TIMEOUT).build().ok()?;
        let (tx, rx) = mpsc::channel(capacidade);
        rt.spawn(enviar_fila(http, url.to_owned(), rx));
        Some(Self { tx })
    }

    /// Enfileira sem bloquear. Cheia ou fechada: descarta e conta.
    pub fn enviar(&self, texto: &str) {
        let msg: String = texto.chars().take(MAX_CHARS).collect();
        if self.tx.try_send(msg).is_err() {
            metrics::counter!("nexus_discord_webhook_descartadas_total").increment(1);
        }
    }
}

async fn enviar_fila(http: reqwest::Client, url: String, mut rx: mpsc::Receiver<String>) {
    while let Some(texto) = rx.recv().await {
        let corpo = json!({ "content": texto, "allowed_mentions": { "parse": [] } });
        let ok = entregar(&http, &url, &corpo).await;
        let resultado = if ok { "ok" } else { "falha" };
        metrics::counter!("nexus_discord_webhook_envios_total", "resultado" => resultado)
            .increment(1);
    }
}

/// Uma mensagem, com no máximo `REPETICOES_429` esperas pelo `retry_after` do Discord.
async fn entregar(http: &reqwest::Client, url: &str, corpo: &Value) -> bool {
    for tentativa in 0..=REPETICOES_429 {
        let resp = match http.post(url).json(corpo).send().await {
            Ok(r) => r,
            Err(e) => {
                // `without_url`: a URL do webhook é segredo.
                tracing::warn!(erro = %e.without_url(), "webhook do Discord falhou");
                return false;
            }
        };
        let status = resp.status();
        if status.is_success() {
            return true;
        }
        if status.as_u16() != 429 || tentativa == REPETICOES_429 {
            tracing::warn!(status = status.as_u16(), "webhook do Discord recusou");
            return false;
        }
        let cabecalho = resp
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<f64>().ok());
        let retry = match resp.json::<Value>().await {
            Ok(v) => v.get("retry_after").and_then(Value::as_f64),
            Err(_) => None,
        };
        let s = retry.or(cabecalho).unwrap_or(1.0);
        let espera = Duration::try_from_secs_f64(s.max(0.0)).unwrap_or(ESPERA_MAXIMA);
        tokio::time::sleep(espera.min(ESPERA_MAXIMA)).await;
    }
    false
}

#[cfg(test)]
mod testes {
    use std::sync::{Arc, Mutex};

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use super::*;

    /// Servidor HTTP falso: responde `respostas` em ordem (a última se repete) e guarda os corpos.
    async fn falso(respostas: Vec<&'static str>) -> (String, Arc<Mutex<Vec<String>>>) {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/hook", l.local_addr().unwrap());
        let corpos = Arc::new(Mutex::new(Vec::new()));
        let guarda = Arc::clone(&corpos);
        tokio::spawn(async move {
            let mut n = 0;
            loop {
                let Ok((mut s, _)) = l.accept().await else {
                    return;
                };
                let mut buf = Vec::new();
                let mut tmp = [0u8; 4096];
                loop {
                    let k = s.read(&mut tmp).await.unwrap_or(0);
                    if k == 0 {
                        break;
                    }
                    buf.extend_from_slice(&tmp[..k]);
                    let t = String::from_utf8_lossy(&buf).to_string();
                    if let Some((cab, corpo)) = t.split_once("\r\n\r\n") {
                        let len = cab
                            .lines()
                            .find_map(|l| {
                                l.to_lowercase()
                                    .strip_prefix("content-length:")
                                    .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                            })
                            .unwrap_or(0);
                        if corpo.len() >= len {
                            guarda.lock().unwrap().push(corpo.to_string());
                            break;
                        }
                    }
                }
                let r = respostas[n.min(respostas.len() - 1)];
                n += 1;
                let _ = s.write_all(r.as_bytes()).await;
                let _ = s.shutdown().await;
            }
        });
        (url, corpos)
    }

    const NO_CONTENT: &str = "HTTP/1.1 204 No Content\r\nconnection: close\r\n\r\n";
    const LIMITADO: &str = "HTTP/1.1 429 Too Many Requests\r\ncontent-type: application/json\r\ncontent-length: 20\r\nconnection: close\r\n\r\n{\"retry_after\":0.05}";

    async fn esperar(corpos: &Arc<Mutex<Vec<String>>>, n: usize) {
        for _ in 0..100 {
            if corpos.lock().unwrap().len() >= n {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    #[tokio::test]
    async fn envia_e_respeita_429() {
        let (url, corpos) = falso(vec![LIMITADO, NO_CONTENT]).await;
        let w = Webhook::iniciar(&url).unwrap();
        w.enviar("Raro novo: teste");
        esperar(&corpos, 2).await;
        let c = corpos.lock().unwrap().clone();
        assert_eq!(c.len(), 2, "{c:?}");
        assert!(c[1].contains("Raro novo: teste"));
        assert!(c[1].contains("allowed_mentions"));
    }

    #[tokio::test]
    async fn quatro_zero_nove_nao_faz_laco_infinito() {
        let (url, corpos) = falso(vec![LIMITADO]).await;
        let w = Webhook::iniciar(&url).unwrap();
        w.enviar("a");
        tokio::time::sleep(Duration::from_millis(800)).await;
        assert_eq!(corpos.lock().unwrap().len(), 1 + REPETICOES_429 as usize);
    }

    #[test]
    fn url_vazia_desliga() {
        assert!(Webhook::iniciar("  ").is_none());
    }

    #[test]
    fn fila_cheia_descarta_sem_bloquear() {
        let (tx, _rx) = mpsc::channel(1);
        let w = Webhook { tx };
        w.enviar("1");
        w.enviar("2");
        w.enviar("3");
    }
}
