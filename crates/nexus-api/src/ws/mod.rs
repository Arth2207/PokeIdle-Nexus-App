//! WebSocket `GET /ws` (protocolo `nexus.v1`, ver `docs/websocket.md`).
//!
//! Peças:
//! - [`Hub`]: o seq global, o buffer circular dos eventos informativos (1000) e o canal de
//!   `broadcast` que leva os avisos a todas as conexões;
//! - o listener (`LISTEN nexus_comando`), que transforma cada NOTIFY do Postgres em aviso. Funciona
//!   mesmo quando quem escreve no banco é o Node, e se a conexão cair ele reconecta e manda todo
//!   mundo conferir a fila de novo;
//! - uma task por conexão, com `select!` sobre o socket, os avisos, o ping e o shutdown. A saída
//!   passa por uma fila `mpsc(256)` e uma task escritora: se a fila enche, a conexão fecha com 1013.
//!
//! Entrega de comando = reserva com lease (as mesmas funções do `GET /api/comandos`). Entrega
//! "pelo menos uma vez": o que não for confirmado no prazo volta à fila e chega de novo.

use std::collections::{HashSet, VecDeque};
use std::sync::atomic::{AtomicU16, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use axum::body::Bytes;
use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use contrato::ws::{
    Atualizacao, ComandoCancelado, ComandoEstado, ComandoNovo, Envelope, ErroWs, EventoAck,
    EventoLote, MensagemCliente, MensagemServidor, Ressincronizar, SessaoPronta, SUBPROTOCOLO,
};
use contrato::Erro;
use futures_util::stream::SplitSink;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use sqlx::postgres::PgListener;
use tokio::sync::broadcast::error::{RecvError, TryRecvError};
use tokio::sync::{broadcast, mpsc, Notify};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

use crate::auth::Cliente;
use crate::bolsa::agora_ms;
use crate::comandos::{self, Confirmada, MAX_RESULTADO};
use crate::estado::Estado;
use crate::rotas::ingestao::{gravar_lote, ErroLote};

#[cfg(test)]
mod testes;

const CANAL: &str = "nexus_comando";
/// Capacidade da fila de saída de cada conexão.
const FILA: usize = 256;
/// Eventos informativos guardados para `retomar`.
const BUFFER: usize = 1000;
/// Comandos reservados por consulta ao banco.
const LOTE: i64 = 50;
const PING: Duration = Duration::from_secs(20);
const PONG_PRAZO: Duration = Duration::from_secs(10);
/// Limite para cada operação de banco dentro do laço da conexão: um banco lento não pode
/// deixar o laço parado a ponto de perder o Pong e derrubar uma conexão saudável.
const BANCO: Duration = Duration::from_secs(5);
/// Quanto esperar a fila de saída esvaziar um pouco antes de considerar o cliente lento.
const ESPERA_FILA: Duration = Duration::from_secs(2);
const TEMPO_FECHAR: Duration = Duration::from_secs(5);
const MAX_MENSAGEM: usize = 64 * 1024;

/// Códigos de fechamento.
const FECHA_SAINDO: u16 = 1001;
const FECHA_ERRO: u16 = 1011;
const FECHA_TENTE_DEPOIS: u16 = 1013;

/// Evento da fila, como o trigger do banco o envia.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Evento {
    Novo,
    Reservado,
    Devolvido,
    Executado,
    Expirado,
}

#[derive(Debug, Deserialize)]
struct Notificacao {
    id: i64,
    conta: String,
    evento: Evento,
}

/// Quem recebe um evento informativo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Destino {
    /// Todas as conexões.
    Todos,
    /// Só tokens globais (o evento não pertence a uma conta).
    Global,
    /// Tokens globais e o token preso a esta conta.
    Conta(String),
}

impl Destino {
    /// `contas`: as contas do token (vazio = token global).
    fn alcanca(&self, contas: &[String]) -> bool {
        match self {
            Destino::Todos => true,
            Destino::Global => contas.is_empty(),
            Destino::Conta(c) => comandos::no_escopo(contas, c),
        }
    }
}

/// O que circula no `broadcast`.
#[derive(Debug, Clone)]
pub enum Aviso {
    Comando {
        id: i64,
        conta: String,
        evento: Evento,
    },
    /// Evento informativo já serializado (com o `seq` dele).
    Info {
        seq: u64,
        texto: Arc<str>,
        destino: Destino,
    },
    /// O listener reconectou (ou o canal transbordou): conferir a fila inteira.
    Conferir,
}

#[derive(Default)]
struct Buffer {
    eventos: VecDeque<(u64, Arc<str>, Destino)>,
    /// Maior `seq` já descartado do buffer.
    descartado_ate: u64,
}

/// Resposta a um `retomar`.
#[derive(Debug)]
pub enum Retomada {
    Ressincronizar(&'static str),
    Eventos(Vec<(u64, Arc<str>)>),
}

pub struct Hub {
    seq: AtomicU64,
    buffer: Mutex<Buffer>,
    avisos: broadcast::Sender<Aviso>,
    /// Cancelado no desligamento do servidor: as conexões fecham com 1001.
    pub parar: CancellationToken,
    pub ping: Duration,
    pub pong_prazo: Duration,
}

impl Hub {
    pub fn novo(parar: CancellationToken) -> Self {
        Self::com_prazos(parar, PING, PONG_PRAZO)
    }

    pub fn com_prazos(parar: CancellationToken, ping: Duration, pong_prazo: Duration) -> Self {
        Self {
            seq: AtomicU64::new(0),
            buffer: Mutex::new(Buffer::default()),
            avisos: broadcast::channel(1024).0,
            parar,
            ping,
            pong_prazo,
        }
    }

    fn proximo_seq(&self) -> u64 {
        self.seq.fetch_add(1, Ordering::SeqCst) + 1
    }

    pub fn seq_atual(&self) -> u64 {
        self.seq.load(Ordering::SeqCst)
    }

    fn buffer(&self) -> std::sync::MutexGuard<'_, Buffer> {
        self.buffer.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Publica um evento informativo (`bolsa.atualizada`, `alvos.atualizados`): ganha `seq`,
    /// entra no buffer circular e vai a todas as conexões.
    pub fn publicar_info(&self, msg: MensagemServidor) {
        self.publicar_para(Destino::Todos, msg);
    }

    /// Como [`Hub::publicar_info`], restrito a quem pode ver o `destino`.
    pub fn publicar_para(&self, destino: Destino, msg: MensagemServidor) {
        let mut b = self.buffer();
        let seq = self.proximo_seq();
        let Ok(texto) = serde_json::to_string(&Envelope::novo(Some(seq), agora_ms(), msg)) else {
            return;
        };
        let texto: Arc<str> = texto.into();
        b.eventos.push_back((seq, texto.clone(), destino.clone()));
        while b.eventos.len() > BUFFER {
            if let Some((velho, _, _)) = b.eventos.pop_front() {
                b.descartado_ate = velho;
            }
        }
        // Sem nenhuma conexão ouvindo o envio falha, e tudo bem: o buffer já guardou.
        let _ = self.avisos.send(Aviso::Info {
            seq,
            texto,
            destino,
        });
    }

    /// O que reenviar a quem pede `retomar` com `desde`.
    pub fn retomar(&self, desde: u64, contas: &[String]) -> Retomada {
        let b = self.buffer();
        if desde > self.seq_atual() {
            // O cliente viu um seq maior que o nosso: o servidor reiniciou.
            return Retomada::Ressincronizar("seq_futuro");
        }
        if desde < b.descartado_ate {
            return Retomada::Ressincronizar("fora_do_buffer");
        }
        Retomada::Eventos(
            b.eventos
                .iter()
                .filter(|(s, _, d)| *s > desde && d.alcanca(contas))
                .map(|(s, t, _)| (*s, t.clone()))
                .collect(),
        )
    }

    fn avisar(&self, a: Aviso) {
        let _ = self.avisos.send(a);
    }
}

fn erro(status: StatusCode, msg: &str) -> Response {
    (status, Json(Erro::novo(msg))).into_response()
}

/// `GET /ws`. O token (escopo `relay`) já foi validado pelo middleware: sem ele a resposta é 401
/// antes do upgrade.
pub async fn handler(
    State(estado): State<Estado>,
    Extension(cliente): Extension<Cliente>,
    ws: WebSocketUpgrade,
) -> Response {
    if !(estado.escrita_comandos || estado.espelho_comandos) {
        return erro(
            StatusCode::SERVICE_UNAVAILABLE,
            "WebSocket desligado neste servidor (NEXUS_ESCRITA_COMANDOS / NEXUS_ESPELHO_COMANDOS)",
        );
    }
    let ws = ws.protocols([SUBPROTOCOLO]).max_message_size(MAX_MENSAGEM);
    if ws.selected_protocol().is_none() {
        return erro(StatusCode::BAD_REQUEST, "subprotocolo nexus.v1 obrigatório");
    }
    ws.on_upgrade(move |socket| conexao(estado, cliente, socket))
}

/// Sobe as tarefas do WebSocket: o listener do Postgres e o aviso de `bolsa.atualizada`.
pub fn iniciar(estado: Estado, parar: CancellationToken) {
    iniciar_listener(estado.clone(), parar.clone());
    iniciar_aviso_bolsa(estado, parar);
}

/// Conexão dedicada com `LISTEN nexus_comando`; reconecta com espera crescente.
pub fn iniciar_listener(estado: Estado, parar: CancellationToken) {
    tokio::spawn(async move {
        let mut espera = Duration::from_secs(1);
        loop {
            match PgListener::connect_with(&estado.db).await {
                Ok(mut l) => match l.listen(CANAL).await {
                    Ok(()) => {
                        espera = Duration::from_secs(1);
                        // Notificações perdidas enquanto estávamos fora: manda conferir a fila.
                        estado.ws.avisar(Aviso::Conferir);
                        loop {
                            tokio::select! {
                                () = parar.cancelled() => return,
                                r = l.try_recv() => match r {
                                    Ok(Some(n)) => {
                                        if let Ok(n) = serde_json::from_str::<Notificacao>(n.payload()) {
                                            if estado.melhorias {
                                                publicar_estado_comando(&estado, &n);
                                            }
                                            estado.ws.avisar(Aviso::Comando {
                                                id: n.id,
                                                conta: n.conta,
                                                evento: n.evento,
                                            });
                                        }
                                    }
                                    Ok(None) => {
                                        tracing::warn!("listener de comandos perdeu a conexão");
                                        break;
                                    }
                                    Err(e) => {
                                        tracing::warn!(erro = %e, "listener de comandos falhou");
                                        break;
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => tracing::warn!(erro = %e, "LISTEN nexus_comando falhou"),
                },
                Err(e) => tracing::warn!(erro = %e, "listener de comandos não conectou"),
            }
            tokio::select! {
                () = parar.cancelled() => return,
                () = tokio::time::sleep(espera) => {}
            }
            espera = (espera * 2).min(Duration::from_secs(30));
        }
    });
}

/// `comando.estado` a partir do NOTIFY da fila (só com `NEXUS_MELHORIAS`). `devolvido` não
/// muda o estado visível (volta a pendente) e não gera mensagem.
fn publicar_estado_comando(estado: &Estado, n: &Notificacao) {
    let nome = match n.evento {
        Evento::Novo => "criado",
        Evento::Reservado => "entregue",
        Evento::Executado => "confirmado",
        Evento::Expirado => "expirado",
        Evento::Devolvido => return,
    };
    estado.ws.publicar_para(
        Destino::Conta(n.conta.clone()),
        MensagemServidor::ComandoEstado(ComandoEstado {
            id: n.id,
            conta: n.conta.clone(),
            estado: nome.into(),
        }),
    );
}

/// Quando o mercado é remontado, avisa os clientes com `bolsa.atualizada`.
fn iniciar_aviso_bolsa(estado: Estado, parar: CancellationToken) {
    tokio::spawn(async move {
        let mut ultimo = None;
        loop {
            if let Ok(s) = estado.bolsa.snapshot().await {
                if ultimo.is_some_and(|u| u != s.montado) {
                    estado
                        .ws
                        .publicar_info(MensagemServidor::BolsaAtualizada(Atualizacao {
                            em: agora_ms(),
                        }));
                }
                ultimo = Some(s.montado);
            }
            tokio::select! {
                () = parar.cancelled() => return,
                () = tokio::time::sleep(Duration::from_secs(5)) => {}
            }
        }
    });
}

enum Saida {
    Texto(String),
    Ping,
}

/// Pede à escritora que feche o socket (o código fica em `codigo`).
struct Fechamento {
    aviso: Notify,
    codigo: AtomicU16,
}

impl Fechamento {
    fn pedir(&self, codigo: u16) {
        self.codigo.store(codigo, Ordering::SeqCst);
        self.aviso.notify_one();
    }
}

/// Por que a conexão acabou.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fim {
    Cliente,
    FilaCheia,
    SemPong,
    Shutdown,
    Erro,
}

impl Fim {
    fn motivo(self) -> &'static str {
        match self {
            Fim::Cliente => "cliente",
            Fim::FilaCheia => "fila_cheia",
            Fim::SemPong => "sem_pong",
            Fim::Shutdown => "shutdown",
            Fim::Erro => "erro",
        }
    }
    fn codigo(self) -> Option<u16> {
        match self {
            Fim::FilaCheia => Some(FECHA_TENTE_DEPOIS),
            Fim::SemPong | Fim::Erro => Some(FECHA_ERRO),
            Fim::Shutdown => Some(FECHA_SAINDO),
            Fim::Cliente => None,
        }
    }
}

/// A task que escreve no socket. Fica separada do loop para que um cliente lento trave só ela:
/// o loop continua enxergando a fila encher e fecha a conexão.
async fn escrever(
    mut sink: SplitSink<WebSocket, Message>,
    mut rx: mpsc::Receiver<Saida>,
    fechamento: Arc<Fechamento>,
) {
    let pedido = loop {
        let saida = tokio::select! {
            biased;
            () = fechamento.aviso.notified() => break true,
            s = rx.recv() => match s {
                Some(s) => s,
                None => break false,
            },
        };
        let msg = match saida {
            Saida::Texto(t) => Message::text(t),
            Saida::Ping => Message::Ping(Bytes::new()),
        };
        tokio::select! {
            r = sink.send(msg) => if r.is_err() { return },
            () = fechamento.aviso.notified() => break true,
        }
    };
    if pedido {
        let codigo = fechamento.codigo.load(Ordering::SeqCst);
        let quadro = Message::Close(Some(CloseFrame {
            code: codigo,
            reason: "".into(),
        }));
        let _ = tokio::time::timeout(TEMPO_FECHAR, sink.send(quadro)).await;
    }
    let _ = tokio::time::timeout(TEMPO_FECHAR, sink.close()).await;
}

struct Conexao {
    estado: Estado,
    tx: mpsc::Sender<Saida>,
    /// Contas do token; vazio = todas (token global).
    contas: Vec<String>,
    /// Quem reserva os comandos (nome do token).
    por: String,
    /// O `seq` atual quando a conexão abriu: o que veio antes só sai num `retomar`.
    seq_inicial: u64,
    /// Maior `seq` informativo já enviado nesta conexão.
    ultimo_info: u64,
}

fn conta_msg(m: &MensagemServidor) -> &'static str {
    match m {
        MensagemServidor::SessaoPronta(_) => "sessao.pronta",
        MensagemServidor::ComandoNovo(_) => "comando.novo",
        MensagemServidor::ComandoCancelado(_) => "comando.cancelado",
        MensagemServidor::BolsaAtualizada(_) => "bolsa.atualizada",
        MensagemServidor::AlvosAtualizados(_) => "alvos.atualizados",
        MensagemServidor::SessaoRessincronizar(_) => "sessao.ressincronizar",
        MensagemServidor::EventoAck(_) => "evento.ack",
        MensagemServidor::Erro(_) => "erro",
        MensagemServidor::RaroNovo(_) => "raro.novo",
        MensagemServidor::ShinyNovo(_) => "shiny.novo",
        MensagemServidor::ComandoEstado(_) => "comando.estado",
    }
}

impl Conexao {
    fn hub(&self) -> &Hub {
        &self.estado.ws
    }

    fn enfileirar(&self, s: Saida) -> Result<(), Fim> {
        self.tx.try_send(s).map_err(|e| match e {
            mpsc::error::TrySendError::Full(_) => Fim::FilaCheia,
            mpsc::error::TrySendError::Closed(_) => Fim::Cliente,
        })
    }

    /// Envia uma mensagem do servidor. `seq`: o dela, se tiver.
    fn enviar(&self, seq: Option<u64>, msg: MensagemServidor) -> Result<(), Fim> {
        let tipo = conta_msg(&msg);
        let texto =
            serde_json::to_string(&Envelope::novo(seq, agora_ms(), msg)).map_err(|_| Fim::Erro)?;
        self.enfileirar(Saida::Texto(texto))?;
        metrics::counter!("nexus_ws_mensagens_total", "sentido" => "enviada", "tipo" => tipo)
            .increment(1);
        Ok(())
    }

    fn enviar_info(&mut self, seq: u64, texto: &Arc<str>) -> Result<(), Fim> {
        if seq <= self.ultimo_info {
            return Ok(());
        }
        self.enfileirar(Saida::Texto(texto.to_string()))?;
        self.ultimo_info = seq;
        metrics::counter!("nexus_ws_mensagens_total", "sentido" => "enviada", "tipo" => "info")
            .increment(1);
        Ok(())
    }

    /// Reenvio de um `retomar`: pula só o que esta conexão já recebeu ao vivo.
    fn reenviar_info(&mut self, seq: u64, texto: &Arc<str>) -> Result<(), Fim> {
        if seq > self.seq_inicial && seq <= self.ultimo_info {
            return Ok(());
        }
        self.enfileirar(Saida::Texto(texto.to_string()))?;
        self.ultimo_info = self.ultimo_info.max(seq);
        metrics::counter!("nexus_ws_mensagens_total", "sentido" => "enviada", "tipo" => "info")
            .increment(1);
        Ok(())
    }

    fn no_escopo(&self, conta: &str) -> bool {
        comandos::no_escopo(&self.contas, conta)
    }

    /// Reserva e envia os comandos pendentes das contas (todas as do token se vazio). Nunca
    /// reserva mais do que cabe na fila de saída; o que não for enviado volta à fila sem contar
    /// tentativa.
    async fn entregar(&self, contas: &[String]) -> Result<(), Fim> {
        loop {
            let mut livre = self.tx.capacity();
            if livre == 0 {
                // Fila cheia: dá um tempo para a escritora esvaziar; se não esvazia, é cliente lento.
                match tokio::time::timeout(ESPERA_FILA, self.tx.reserve()).await {
                    Ok(Ok(permissao)) => drop(permissao),
                    _ => return Err(Fim::FilaCheia),
                }
                livre = self.tx.capacity().max(1);
            }
            let limite = LOTE.min(livre as i64);
            let v = match tokio::time::timeout(
                BANCO,
                comandos::entregar(&self.estado, contas, &self.por, limite),
            )
            .await
            {
                Ok(Ok(v)) => v,
                Ok(Err(e)) => {
                    tracing::warn!(erro = %e, "ws: não consegui reservar comandos");
                    return Ok(());
                }
                Err(_) => {
                    tracing::warn!("ws: reserva de comandos estourou o tempo");
                    return Ok(());
                }
            };
            let n = v.len() as i64;
            let ids: Vec<i64> = v.iter().map(|c| c.id).collect();
            for (i, c) in v.into_iter().enumerate() {
                let r = self.enviar(
                    Some(self.hub().proximo_seq()),
                    MensagemServidor::ComandoNovo(ComandoNovo {
                        id: c.id,
                        conta: c.conta,
                        tipo: c.tipo,
                        params: c.params,
                        prioridade: c.prioridade,
                        expira_em: c.expira_ms,
                    }),
                );
                if let Err(f) = r {
                    // Devolve o que não saiu (este e os seguintes).
                    let resto = ids.get(i..).unwrap_or_default();
                    if let Ok(Err(e)) = tokio::time::timeout(
                        BANCO,
                        persistencia::comandos::devolver(&self.estado.db, resto),
                    )
                    .await
                    {
                        tracing::warn!(erro = %e, "ws: não consegui devolver comandos");
                    }
                    return Err(f);
                }
            }
            if n < limite {
                return Ok(());
            }
        }
    }

    async fn do_cliente(&mut self, texto: &str) -> Result<(), Fim> {
        let Ok(env) = serde_json::from_str::<Envelope<MensagemCliente>>(texto) else {
            metrics::counter!("nexus_ws_mensagens_total", "sentido" => "recebida", "tipo" => "invalida")
                .increment(1);
            return Ok(());
        };
        match env.msg {
            // Os comandos têm lease e a retomada usa o `desde` do cliente: o ack só é contado.
            MensagemCliente::Ack(_) => self.recebida("ack"),
            MensagemCliente::Retomar(r) => {
                self.recebida("retomar");
                match self.hub().retomar(r.desde, &self.contas) {
                    Retomada::Ressincronizar(motivo) => {
                        self.enviar(
                            None,
                            MensagemServidor::SessaoRessincronizar(Ressincronizar {
                                motivo: motivo.into(),
                            }),
                        )?;
                        // O cliente recarrega o estado: parte do seq atual.
                        self.ultimo_info = self.hub().seq_atual();
                    }
                    Retomada::Eventos(v) => {
                        for (seq, t) in v {
                            self.reenviar_info(seq, &t)?;
                        }
                    }
                }
                // Os comandos vêm da própria fila, não do buffer.
                self.entregar(&self.contas.clone()).await?;
            }
            MensagemCliente::ComandoResultado(r) => {
                self.recebida("comando.resultado");
                self.confirmar(r.id, &r.resultado).await;
            }
            MensagemCliente::EventoLote(l) => {
                self.recebida("evento.lote");
                self.lote(l).await?;
            }
        }
        Ok(())
    }

    fn erro_ws(&self, motivo: &str, seq_cliente: Option<u64>) -> Result<(), Fim> {
        self.enviar(
            None,
            MensagemServidor::Erro(ErroWs {
                motivo: motivo.into(),
                seq_cliente,
            }),
        )
    }

    /// `evento.lote`: grava pela mesma função do `POST /ingest` e responde `evento.ack`.
    async fn lote(&self, l: EventoLote) -> Result<(), Fim> {
        let seq = Some(l.seq_cliente);
        if self.estado.melhorias && !self.estado.limite_ingestao.permitir(&self.por) {
            metrics::counter!("nexus_ws_lotes_limitados_total").increment(1);
            return self.erro_ws("limite_taxa", seq);
        }
        // Token de uma conta: o lote vai para ela se `conta` faltar. Token de várias contas ou
        // global exige `conta`, e ela precisa estar no escopo do token.
        let unica = (self.contas.len() == 1)
            .then(|| self.contas.first().map(String::as_str))
            .flatten();
        let Some(conta) = l.conta.as_deref().or(unica) else {
            return self.erro_ws("conta_obrigatoria", seq);
        };
        if !comandos::no_escopo(&self.contas, conta) {
            return self.erro_ws("conta_proibida", seq);
        }
        let conta_token = (!self.contas.is_empty()).then_some(conta);
        let r = tokio::time::timeout(
            BANCO,
            gravar_lote(&self.estado, conta_token, conta, Some(conta), &l.eventos),
        )
        .await;
        match r {
            Ok(Ok(r)) => self.enviar(
                None,
                MensagemServidor::EventoAck(EventoAck {
                    seq_cliente: l.seq_cliente,
                    ok: true,
                    duplicados: r.duplicados as u64,
                    erros: r.erros as u64,
                }),
            ),
            Ok(Err(ErroLote::Desligada)) => self.erro_ws("ingestao_desligada", seq),
            Ok(Err(ErroLote::ContaProibida)) => self.erro_ws("conta_proibida", seq),
            Ok(Err(ErroLote::Banco(e))) => {
                tracing::warn!(erro = %e, "ws: lote de eventos falhou");
                self.erro_ws("banco_indisponivel", seq)
            }
            Err(_) => self.erro_ws("banco_indisponivel", seq),
        }
    }

    fn recebida(&self, tipo: &'static str) {
        metrics::counter!("nexus_ws_mensagens_total", "sentido" => "recebida", "tipo" => tipo)
            .increment(1);
    }

    /// Confirmação idempotente. Token de conta só confirma comando da própria conta (mesma
    /// regra da rota HTTP).
    async fn confirmar(&self, id: i64, resultado: &str) {
        let resultado: String = resultado.chars().take(MAX_RESULTADO).collect();
        let r = tokio::time::timeout(
            BANCO,
            comandos::confirmar_no_escopo(&self.estado, &self.contas, id, &resultado),
        )
        .await;
        match r {
            Ok(Ok(Confirmada::Negada)) => {
                metrics::counter!("nexus_ws_confirmacoes_negadas_total").increment(1);
            }
            Ok(Ok(Confirmada::Resultado(_))) => {}
            Ok(Err(e)) => tracing::warn!(erro = %e, "ws: confirmação falhou"),
            Err(_) => tracing::warn!("ws: confirmação estourou o tempo"),
        }
    }

    /// Trata um aviso do `broadcast` e os que já estiverem na fila, entregando uma vez só.
    async fn avisos(
        &mut self,
        primeiro: Result<Aviso, RecvError>,
        rx: &mut broadcast::Receiver<Aviso>,
    ) -> Result<(), Fim> {
        let mut contas: HashSet<String> = HashSet::new();
        let mut tudo = false;
        let mut lag = false;
        let mut atual = Some(primeiro);
        while let Some(r) = atual.take() {
            match r {
                Ok(Aviso::Comando { id, conta, evento }) if self.no_escopo(&conta) => {
                    match evento {
                        Evento::Novo | Evento::Devolvido => {
                            contas.insert(conta);
                        }
                        Evento::Expirado => self.enviar(
                            Some(self.hub().proximo_seq()),
                            MensagemServidor::ComandoCancelado(ComandoCancelado {
                                id,
                                conta,
                                motivo: "expirado".into(),
                            }),
                        )?,
                        Evento::Reservado | Evento::Executado => {}
                    }
                }
                Ok(Aviso::Comando { .. }) => {}
                Ok(Aviso::Info {
                    seq,
                    texto,
                    destino,
                }) => {
                    if destino.alcanca(&self.contas) {
                        self.enviar_info(seq, &texto)?;
                    }
                }
                Ok(Aviso::Conferir) => tudo = true,
                Err(RecvError::Lagged(_)) => {
                    tudo = true;
                    lag = true;
                }
                Err(RecvError::Closed) => return Err(Fim::Shutdown),
            }
            atual = match rx.try_recv() {
                Ok(a) => Some(Ok(a)),
                Err(TryRecvError::Lagged(n)) => Some(Err(RecvError::Lagged(n))),
                Err(TryRecvError::Empty | TryRecvError::Closed) => None,
            };
        }
        if lag {
            // Perdemos avisos informativos: o cliente recarrega o estado.
            self.enviar(
                None,
                MensagemServidor::SessaoRessincronizar(Ressincronizar {
                    motivo: "lag".into(),
                }),
            )?;
            self.ultimo_info = self.hub().seq_atual();
        }
        if tudo {
            self.entregar(&self.contas.clone()).await
        } else if !contas.is_empty() {
            self.entregar(&contas.into_iter().collect::<Vec<_>>()).await
        } else {
            Ok(())
        }
    }
}

async fn conexao(estado: Estado, cliente: Cliente, socket: WebSocket) {
    metrics::gauge!("nexus_ws_conexoes_abertas").increment(1.0);
    let fim = rodar(estado, cliente, socket).await;
    metrics::gauge!("nexus_ws_conexoes_abertas").decrement(1.0);
    metrics::counter!("nexus_ws_fechamentos_total", "motivo" => fim.motivo()).increment(1);
}

async fn rodar(estado: Estado, cliente: Cliente, socket: WebSocket) -> Fim {
    let hub = Arc::clone(&estado.ws);
    let (sink, mut entrada) = socket.split();
    let (tx, rx) = mpsc::channel::<Saida>(FILA);
    let fechamento = Arc::new(Fechamento {
        aviso: Notify::new(),
        codigo: AtomicU16::new(FECHA_SAINDO),
    });
    let escritora = tokio::spawn(escrever(sink, rx, Arc::clone(&fechamento)));
    // Lê o seq antes de assinar: um evento publicado entre as duas coisas chega de novo (e o
    // filtro por seq descarta a repetição), em vez de sumir.
    let seq_ini = hub.seq_atual();
    let mut avisos = hub.avisos.subscribe();
    let mut c = Conexao {
        estado,
        tx,
        contas: cliente.conta.iter().cloned().collect(),
        por: cliente.nome,
        seq_inicial: seq_ini,
        ultimo_info: seq_ini,
    };

    let fim = laco(&mut c, &hub, &mut entrada, &mut avisos).await;

    match fim.codigo() {
        Some(codigo) => {
            fechamento.pedir(codigo);
            let _ = tokio::time::timeout(TEMPO_FECHAR * 2, escritora).await;
        }
        None => {
            drop(c);
            let _ = tokio::time::timeout(Duration::from_secs(1), escritora).await;
        }
    }
    fim
}

async fn laco(
    c: &mut Conexao,
    hub: &Hub,
    entrada: &mut futures_util::stream::SplitStream<WebSocket>,
    avisos: &mut broadcast::Receiver<Aviso>,
) -> Fim {
    let pronta = MensagemServidor::SessaoPronta(SessaoPronta {
        seq: hub.seq_atual(),
    });
    if let Err(f) = c.enviar(Some(hub.seq_atual()), pronta) {
        return f;
    }
    // Reentrega imediata do que a conexão anterior deste token recebeu e não confirmou.
    match tokio::time::timeout(
        BANCO,
        persistencia::comandos::devolver_do_consumidor(&c.estado.db, &c.por),
    )
    .await
    {
        Ok(Ok(_)) => {}
        Ok(Err(e)) => tracing::warn!(erro = %e, "ws: não consegui devolver os reservados"),
        Err(_) => tracing::warn!("ws: devolução dos reservados estourou o tempo"),
    }
    if let Err(f) = c.entregar(&c.contas.clone()).await {
        return f;
    }

    let mut ping = tokio::time::interval_at(Instant::now() + hub.ping, hub.ping);
    ping.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut pong_ate: Option<Instant> = None;
    loop {
        let r: Result<(), Fim> = tokio::select! {
            () = hub.parar.cancelled() => Err(Fim::Shutdown),
            () = async {
                match pong_ate {
                    Some(t) => tokio::time::sleep_until(t).await,
                    None => std::future::pending().await,
                }
            } => Err(Fim::SemPong),
            _ = ping.tick() => {
                if pong_ate.is_none() {
                    pong_ate = Some(Instant::now() + hub.pong_prazo);
                    c.enfileirar(Saida::Ping)
                } else {
                    Ok(())
                }
            }
            msg = entrada.next() => match msg {
                None | Some(Err(_)) | Some(Ok(Message::Close(_))) => Err(Fim::Cliente),
                Some(Ok(Message::Text(t))) => c.do_cliente(t.as_str()).await,
                Some(Ok(Message::Pong(_))) => {
                    pong_ate = None;
                    Ok(())
                }
                // O axum responde Ping sozinho; binário não faz parte do protocolo.
                Some(Ok(_)) => Ok(()),
            },
            a = avisos.recv() => c.avisos(a, avisos).await,
        };
        if let Err(f) = r {
            return f;
        }
    }
}
