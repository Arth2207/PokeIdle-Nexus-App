//! Cliente WebSocket `nexus.v1`: conecta, retoma por `seq`, recebe comandos, confirma
//! resultados, envia lotes de eventos e reconecta com backoff.
//!
//! [`Estado`] concentra a lógica (testável sem rede); [`rodar`] só cuida do socket.

use crate::backoff::Backoff;
use crate::comandos::{Despachante, Executor, Modo};
use crate::eventos::FilaEventos;
use crate::sessao::Sessao;
use crate::{ArmazemToken, Erro};
use contrato::ws::{Ack, Envelope, MensagemCliente, MensagemServidor, SUBPROTOCOLO};
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::{mpsc, watch};
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message};

const ESPERA_ACK: Duration = Duration::from_secs(10);

#[derive(Debug, Clone)]
pub struct Config {
    /// `wss://host/ws`.
    pub url: String,
    pub modo: Modo,
    /// Contas deste app (vazio = as do token).
    pub contas: Vec<String>,
    /// Conta dos lotes de eventos (obrigatória com token global).
    pub conta_lote: Option<String>,
    pub lote_max: usize,
    /// Avisos de conexão para a tela ("conectado", "sem conexão: …"); `None` = sem aviso.
    pub avisos: Option<mpsc::UnboundedSender<String>>,
}

pub fn agora_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Estado que sobrevive às reconexões (o lote em voo e o `seq` não se perdem).
pub struct Estado<E> {
    pub sessao: Sessao,
    pub fila: FilaEventos,
    pub despachante: Despachante<E>,
    resultados: mpsc::UnboundedSender<MensagemCliente>,
}

impl<E: Executor> Estado<E> {
    pub fn novo(cfg: &Config, exec: E) -> (Self, mpsc::UnboundedReceiver<MensagemCliente>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let estado = Self {
            sessao: Sessao::default(),
            fila: FilaEventos::nova(cfg.conta_lote.clone(), agora_ms().max(0) as u64),
            despachante: Despachante::novo(exec, &cfg.contas, cfg.modo),
            resultados: tx,
        };
        (estado, rx)
    }

    /// Processa uma mensagem do servidor; devolve o que responder já. Resultados de comandos
    /// saem depois, pelo canal devolvido em [`Estado::novo`] (a execução não trava o socket,
    /// senão o `Pong` atrasaria e o servidor fecharia a conexão).
    pub fn ao_receber(&mut self, env: Envelope<MensagemServidor>) -> Vec<MensagemCliente> {
        let mut saida = Vec::new();
        let seq = env.seq;
        // Comando reentregue pode vir com `seq` antigo: quem deduplica é o despachante, por id.
        let novo = self.sessao.registrar(seq);
        match env.msg {
            MensagemServidor::SessaoPronta(p) => self.sessao.adotar_pronta(p.seq),
            MensagemServidor::ComandoNovo(cmd) => {
                let d = self.despachante.clone();
                let tx = self.resultados.clone();
                tokio::spawn(async move {
                    if let Some(r) = d.tratar(&cmd, agora_ms()).await {
                        let _ = tx.send(MensagemCliente::ComandoResultado(r));
                    }
                });
            }
            MensagemServidor::ComandoCancelado(c) => self.despachante.cancelar(c.id),
            MensagemServidor::SessaoRessincronizar(_) => {
                self.sessao.precisa_ressincronizar = true;
            }
            MensagemServidor::EventoAck(a) => {
                self.fila.confirmar(&a);
            }
            // Informativos e erros: a casca decide o que fazer (TODO: expor por callback).
            _ => {}
        }
        if let (Some(s), true) = (seq, novo) {
            saida.push(MensagemCliente::Ack(Ack { seq: s }));
        }
        saida
    }
}

fn texto(msg: &MensagemCliente) -> Option<Message> {
    let env = Envelope::novo(None, agora_ms(), msg);
    serde_json::to_string(&env).ok().map(Message::text)
}

fn avisar(cfg: &Config, msg: String) {
    if let Some(t) = &cfg.avisos {
        let _ = t.send(msg);
    }
}

enum Fim {
    Parar,
    Caiu { ficou_de_pe: bool },
}

/// Roda até `parar` virar `true`. Erro só para token ausente ou config inválida; queda de rede
/// reconecta sozinha.
pub async fn rodar<E: Executor, A: ArmazemToken>(
    cfg: Config,
    armazem: A,
    exec: E,
    mut eventos: mpsc::Receiver<Value>,
    mut parar: watch::Receiver<bool>,
) -> Result<(), Erro> {
    let (mut estado, mut resultados) = Estado::novo(&cfg, exec);
    let mut backoff = Backoff::novo(agora_ms().unsigned_abs());
    loop {
        // Relê a cada tentativa: o usuário pode ter trocado o token.
        let token = armazem.ler()?.ok_or(Erro::SemToken)?;
        let mut req = cfg
            .url
            .as_str()
            .into_client_request()
            .map_err(|e| Erro::Config(e.to_string()))?;
        let h = req.headers_mut();
        h.insert(
            "Authorization",
            format!("Bearer {token}")
                .parse()
                .map_err(|_| Erro::Config("token inválido para cabeçalho".into()))?,
        );
        h.insert(
            "Sec-WebSocket-Protocol",
            SUBPROTOCOLO
                .parse()
                .map_err(|_| Erro::Config("subprotocolo".into()))?,
        );
        let fim = match tokio_tungstenite::connect_async(req).await {
            Ok((ws, _)) => {
                avisar(&cfg, "conectado ao Nexus".into());
                sessao(
                    ws,
                    &cfg,
                    &mut estado,
                    &mut resultados,
                    &mut eventos,
                    &mut parar,
                )
                .await
            }
            Err(e) => {
                avisar(&cfg, format!("sem conexão com o Nexus: {e}"));
                Fim::Caiu { ficou_de_pe: false }
            }
        };
        match fim {
            Fim::Parar => return Ok(()),
            Fim::Caiu { ficou_de_pe } => {
                if ficou_de_pe {
                    avisar(&cfg, "conexão com o Nexus caiu; reconectando".into());
                    backoff.zerar();
                }
                let espera = backoff.proximo();
                tokio::select! {
                    () = tokio::time::sleep(espera) => {}
                    _ = parar.changed() => {}
                }
                if *parar.borrow() {
                    return Ok(());
                }
            }
        }
    }
}

async fn sessao<E: Executor, S>(
    ws: tokio_tungstenite::WebSocketStream<S>,
    cfg: &Config,
    estado: &mut Estado<E>,
    resultados: &mut mpsc::UnboundedReceiver<MensagemCliente>,
    eventos: &mut mpsc::Receiver<Value>,
    parar: &mut watch::Receiver<bool>,
) -> Fim
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let (mut tx, mut rx) = ws.split();
    let mut ficou_de_pe = false;
    let caiu = |p| Fim::Caiu { ficou_de_pe: p };
    let Some(retomar) = texto(&MensagemCliente::Retomar(estado.sessao.retomar())) else {
        return caiu(false);
    };
    if tx.send(retomar).await.is_err() {
        return caiu(false);
    }
    let mut enviado_em: Option<Instant> = None;
    let mut tique = tokio::time::interval(Duration::from_secs(1));
    loop {
        let mut enviar: Vec<MensagemCliente> = Vec::new();
        tokio::select! {
            _ = parar.changed() => {
                if *parar.borrow() {
                    let _ = tx.send(Message::Close(None)).await;
                    return Fim::Parar;
                }
            }
            m = rx.next() => match m {
                Some(Ok(Message::Text(t))) => {
                    if let Ok(env) = serde_json::from_str::<Envelope<MensagemServidor>>(t.as_str()) {
                        if matches!(env.msg, MensagemServidor::SessaoPronta(_)) {
                            ficou_de_pe = true;
                        }
                        let era_ack = matches!(env.msg, MensagemServidor::EventoAck(_));
                        enviar = estado.ao_receber(env);
                        if era_ack && !estado.fila.em_voo() {
                            enviado_em = None;
                            // Fila com trabalho: o próximo lote sai já, sem esperar o tique.
                            if ficou_de_pe {
                                if let Some(l) = estado.fila.proximo_lote(cfg.lote_max) {
                                    enviar.push(MensagemCliente::EventoLote(l));
                                    enviado_em = Some(Instant::now());
                                }
                            }
                        }
                    }
                }
                Some(Ok(Message::Ping(p))) => {
                    if tx.send(Message::Pong(p)).await.is_err() {
                        return caiu(ficou_de_pe);
                    }
                }
                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => return caiu(ficou_de_pe),
                Some(Ok(_)) => {}
            },
            Some(r) = resultados.recv() => enviar.push(r),
            Some(ev) = eventos.recv() => { estado.fila.adicionar(ev); }
            _ = tique.tick() => {
                let vencido = enviado_em.is_none_or(|t| t.elapsed() >= ESPERA_ACK);
                if ficou_de_pe && vencido {
                    if let Some(l) = estado.fila.proximo_lote(cfg.lote_max) {
                        enviar.push(MensagemCliente::EventoLote(l));
                        enviado_em = Some(Instant::now());
                    }
                }
            }
        }
        for m in &enviar {
            let Some(t) = texto(m) else { continue };
            if tx.send(t).await.is_err() {
                return caiu(ficou_de_pe);
            }
        }
    }
}

#[cfg(test)]
mod testes {
    use super::*;
    use contrato::ws::{ComandoNovo, EventoAck, SessaoPronta};
    use std::future::Future;
    use std::pin::Pin;

    struct Eco;
    impl Executor for Eco {
        fn executar<'a>(
            &'a self,
            c: &'a ComandoNovo,
        ) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send + 'a>> {
            Box::pin(async move { Ok(format!("feito {}", c.id)) })
        }
    }

    fn cfg() -> Config {
        Config {
            url: "ws://x/ws".into(),
            modo: Modo::Ativo,
            contas: vec![],
            conta_lote: None,
            lote_max: 50,
            avisos: None,
        }
    }

    fn env(seq: Option<u64>, msg: MensagemServidor) -> Envelope<MensagemServidor> {
        Envelope::novo(seq, 0, msg)
    }

    fn comando(id: i64) -> MensagemServidor {
        MensagemServidor::ComandoNovo(ComandoNovo {
            id,
            conta: "a".into(),
            tipo: "t".into(),
            params: serde_json::json!({}),
            prioridade: 0,
            expira_em: None,
        })
    }

    #[tokio::test]
    async fn seq_avanca_ack_sai_e_retomar_usa_ultimo() {
        let (mut e, _rx) = Estado::novo(&cfg(), Eco);
        let _ = e.ao_receber(env(
            None,
            MensagemServidor::SessaoPronta(SessaoPronta { seq: 5 }),
        ));
        assert_eq!(e.sessao.retomar().desde, 5);
        let out = e.ao_receber(env(Some(6), comando(1)));
        assert_eq!(out, vec![MensagemCliente::Ack(Ack { seq: 6 })]);
        let out = e.ao_receber(env(
            Some(6),
            MensagemServidor::SessaoRessincronizar(contrato::ws::Ressincronizar {
                motivo: "lag".into(),
            }),
        ));
        assert!(out.is_empty(), "seq repetido não é confirmado de novo");
        assert_eq!(e.sessao.retomar().desde, 6);
    }

    #[tokio::test]
    async fn comando_reentregue_confirma_o_mesmo_resultado() {
        let (mut e, mut rx) = Estado::novo(&cfg(), Eco);
        e.ao_receber(env(Some(1), comando(7)));
        e.ao_receber(env(Some(1), comando(7)));
        let r1 = rx.recv().await;
        let r2 = rx.recv().await;
        assert!(r1.is_some());
        assert_eq!(r1, r2);
    }

    #[tokio::test]
    async fn ack_de_lote_libera_proximo() {
        let (mut e, _rx) = Estado::novo(&cfg(), Eco);
        e.fila.adicionar(serde_json::json!(1));
        let l = e.fila.proximo_lote(10);
        assert!(e.fila.em_voo());
        let seq = l.map(|l| l.seq_cliente).unwrap_or(0);
        e.ao_receber(env(
            None,
            MensagemServidor::EventoAck(EventoAck {
                seq_cliente: seq,
                ok: true,
                duplicados: 0,
                erros: 0,
            }),
        ));
        assert!(!e.fila.em_voo());
    }
}
