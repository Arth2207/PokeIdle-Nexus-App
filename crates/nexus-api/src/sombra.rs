//! Cliente sombra do WebSocket: mede, antes da A2, se o canal entrega "7 dias sem comando
//! perdido nem duplicado". Recebe os comandos do espelho, NÃO executa nada e responde
//! `comando.resultado` com `sombra`. Serve de base para o relay da A2.
//!
//! Variáveis: `NEXUS_SOMBRA_URL` (`ws://host:porta/ws`) e `NEXUS_SOMBRA_TOKEN_FILE` (arquivo com o
//! token de escopo `relay`; o token nunca é logado).

use std::collections::{HashSet, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context};
use contrato::compressao::{self, LIMITE_DESCOMPRIMIDO};
use contrato::ws::{
    ComandoResultado, Envelope, MensagemCliente, MensagemServidor, Retomar, SUBPROTOCOLO,
    SUBPROTOCOLO_DEFLATE,
};
use futures_util::{SinkExt, StreamExt};
use sqlx::{PgPool, Row};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;

use crate::bolsa::agora_ms;
use crate::comandos::RESULTADO_SOMBRA;

const BACKOFF_MIN: Duration = Duration::from_secs(1);
const BACKOFF_MAX: Duration = Duration::from_secs(30);
const TEMPO_CONEXAO: Duration = Duration::from_secs(10);
/// Quantos ids confirmados a sombra lembra para detectar repetição.
const MEMORIA_IDS: usize = 200_000;

pub struct ConfigSombra {
    pub url: String,
    token: String,
}

impl ConfigSombra {
    pub fn nova(url: &str, token: &str) -> anyhow::Result<Self> {
        let url = url.trim();
        if !url.starts_with("ws://") {
            bail!("NEXUS_SOMBRA_URL deve começar com ws:// (o cliente sombra não fala TLS)");
        }
        let token = token.trim();
        if token.is_empty() {
            bail!("o token da sombra está vazio");
        }
        Ok(Self {
            url: url.to_owned(),
            token: token.to_owned(),
        })
    }

    pub fn do_ambiente() -> anyhow::Result<Self> {
        let url = std::env::var("NEXUS_SOMBRA_URL").context("falta NEXUS_SOMBRA_URL")?;
        let caminho =
            std::env::var("NEXUS_SOMBRA_TOKEN_FILE").context("falta NEXUS_SOMBRA_TOKEN_FILE")?;
        let token = std::fs::read_to_string(&caminho)
            .with_context(|| format!("não consegui ler {caminho}"))?;
        Self::nova(&url, &token)
    }
}

/// Contadores do processo, lidos no log periódico e no encerramento.
#[derive(Default)]
pub struct Contadores {
    pub recebidos: AtomicU64,
    pub confirmados: AtomicU64,
    pub duplicados: AtomicU64,
    pub reconexoes: AtomicU64,
    pub ressincronizacoes: AtomicU64,
}

impl Contadores {
    fn log(&self, msg: &str) {
        tracing::info!(
            recebidos = self.recebidos.load(Ordering::Relaxed),
            confirmados = self.confirmados.load(Ordering::Relaxed),
            duplicados = self.duplicados.load(Ordering::Relaxed),
            reconexoes = self.reconexoes.load(Ordering::Relaxed),
            ressincronizacoes = self.ressincronizacoes.load(Ordering::Relaxed),
            "{msg}"
        );
    }
}

/// O que sobrevive entre conexões.
#[derive(Default)]
struct Memoria {
    seq: Option<u64>,
    vistos: HashSet<i64>,
    ordem: VecDeque<i64>,
}

impl Memoria {
    fn registrar(&mut self, id: i64) {
        if self.vistos.insert(id) {
            self.ordem.push_back(id);
            if self.ordem.len() > MEMORIA_IDS {
                if let Some(velho) = self.ordem.pop_front() {
                    self.vistos.remove(&velho);
                }
            }
        }
    }
}

enum Saida {
    Parar,
    Caiu { pronta: bool },
}

/// Roda até `parar` ser cancelado, reconectando com backoff.
pub async fn rodar(cfg: &ConfigSombra, parar: CancellationToken, cont: Arc<Contadores>) {
    let mut mem = Memoria::default();
    let mut espera = BACKOFF_MIN;
    loop {
        match sessao(cfg, &mut mem, &cont, &parar).await {
            Saida::Parar => break,
            Saida::Caiu { pronta } => {
                if pronta {
                    espera = BACKOFF_MIN;
                }
            }
        }
        cont.reconexoes.fetch_add(1, Ordering::Relaxed);
        let dorme = com_jitter(espera);
        tracing::info!(espera_ms = dorme.as_millis() as u64, "reconectando");
        tokio::select! {
            () = parar.cancelled() => break,
            () = tokio::time::sleep(dorme) => {}
        }
        espera = (espera * 2).min(BACKOFF_MAX);
    }
}

/// ±20% de jitter. Se o sistema não der aleatoriedade, usa a espera pura.
fn com_jitter(base: Duration) -> Duration {
    let mut b = [0u8; 4];
    if getrandom::fill(&mut b).is_err() {
        return base;
    }
    let f = f64::from(u32::from_le_bytes(b)) / f64::from(u32::MAX);
    base.mul_f64(0.8 + 0.4 * f)
}

async fn enviar<S>(ws: &mut S, msg: MensagemCliente) -> bool
where
    S: futures_util::Sink<Message> + Unpin,
{
    let env = Envelope::novo(None, agora_ms(), msg);
    let Ok(texto) = serde_json::to_string(&env) else {
        return false;
    };
    ws.send(Message::text(texto)).await.is_ok()
}

async fn sessao(
    cfg: &ConfigSombra,
    mem: &mut Memoria,
    cont: &Contadores,
    parar: &CancellationToken,
) -> Saida {
    let caiu = Saida::Caiu { pronta: false };
    let mut req = match cfg.url.as_str().into_client_request() {
        Ok(r) => r,
        Err(erro) => {
            tracing::error!(%erro, "NEXUS_SOMBRA_URL inválida");
            return caiu;
        }
    };
    let Ok(mut auth) = HeaderValue::from_str(&format!("Bearer {}", cfg.token)) else {
        tracing::error!("token da sombra tem caracteres inválidos para um header");
        return caiu;
    };
    auth.set_sensitive(true);
    req.headers_mut().insert("Authorization", auth);
    req.headers_mut().insert(
        "Sec-WebSocket-Protocol",
        HeaderValue::from_static("nexus.v1.deflate, nexus.v1"),
    );
    debug_assert_eq!(SUBPROTOCOLO_DEFLATE, "nexus.v1.deflate");
    debug_assert_eq!(SUBPROTOCOLO, "nexus.v1");

    let conexao = tokio::select! {
        () = parar.cancelled() => return Saida::Parar,
        r = tokio::time::timeout(TEMPO_CONEXAO, tokio_tungstenite::connect_async(req)) => r,
    };
    let (mut ws, resposta) = match conexao {
        Ok(Ok(par)) => par,
        Ok(Err(erro)) => {
            tracing::warn!(%erro, "falha ao conectar");
            return caiu;
        }
        Err(_) => {
            tracing::warn!("tempo esgotado ao conectar");
            return caiu;
        }
    };
    let sub = resposta
        .headers()
        .get("sec-websocket-protocol")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned();
    tracing::info!(subprotocolo = %sub, retomar_de = ?mem.seq, "conectado");

    if let Some(desde) = mem.seq {
        if !enviar(&mut ws, MensagemCliente::Retomar(Retomar { desde })).await {
            return caiu;
        }
    }

    let mut pronta = false;
    loop {
        let m = tokio::select! {
            () = parar.cancelled() => {
                let _ = tokio::time::timeout(Duration::from_secs(2), ws.close(None)).await;
                return Saida::Parar;
            }
            m = ws.next() => m,
        };
        let m = match m {
            None | Some(Ok(Message::Close(_))) => {
                tracing::warn!("conexão encerrada pelo servidor");
                return Saida::Caiu { pronta };
            }
            Some(Err(erro)) => {
                tracing::warn!(%erro, "erro na conexão");
                return Saida::Caiu { pronta };
            }
            Some(Ok(m)) => m,
        };
        let texto = match m {
            Message::Text(t) => t.as_str().to_owned(),
            Message::Binary(b) => match compressao::descomprimir(&b, LIMITE_DESCOMPRIMIDO) {
                Ok(t) => t,
                Err(erro) => {
                    tracing::warn!(?erro, "frame binário inválido");
                    continue;
                }
            },
            Message::Ping(p) => {
                if ws.send(Message::Pong(p)).await.is_err() {
                    return Saida::Caiu { pronta };
                }
                continue;
            }
            _ => continue,
        };
        let env: Envelope<MensagemServidor> = match serde_json::from_str(&texto) {
            Ok(e) => e,
            Err(erro) => {
                tracing::warn!(%erro, "mensagem do servidor não entendida");
                continue;
            }
        };
        if let Some(s) = env.seq {
            mem.seq = Some(s);
        }
        match env.msg {
            MensagemServidor::SessaoPronta(p) => {
                pronta = true;
                mem.seq = Some(p.seq);
            }
            MensagemServidor::SessaoRessincronizar(r) => {
                // Os comandos vêm da fila no banco, então não há estado HTTP a recarregar:
                // só descarta o seq para não pedir de novo um ponto que o servidor não tem.
                cont.ressincronizacoes.fetch_add(1, Ordering::Relaxed);
                tracing::warn!(motivo = %r.motivo, "servidor pediu ressincronização");
                mem.seq = None;
            }
            MensagemServidor::ComandoNovo(c) => {
                let repetido = mem.vistos.contains(&c.id);
                if repetido {
                    cont.duplicados.fetch_add(1, Ordering::Relaxed);
                    metrics::counter!("nexus_sombra_duplicados_total").increment(1);
                    tracing::warn!(id = c.id, "comando recebido de novo depois de confirmado");
                } else {
                    cont.recebidos.fetch_add(1, Ordering::Relaxed);
                }
                let resp = MensagemCliente::ComandoResultado(ComandoResultado {
                    id: c.id,
                    resultado: RESULTADO_SOMBRA.to_owned(),
                });
                if !enviar(&mut ws, resp).await {
                    return Saida::Caiu { pronta };
                }
                if !repetido {
                    cont.confirmados.fetch_add(1, Ordering::Relaxed);
                    mem.registrar(c.id);
                }
            }
            _ => {}
        }
    }
}

/// Ponto de entrada do subcomando `sombra`.
pub async fn executar(db: PgPool) -> anyhow::Result<()> {
    let cfg = ConfigSombra::do_ambiente()?;
    tracing::info!(url = %cfg.url, "cliente sombra iniciando");
    let cont = Arc::new(Contadores::default());
    let parar = CancellationToken::new();

    let p = parar.clone();
    tokio::spawn(async move {
        crate::desligamento::sinal_de_parada().await;
        p.cancel();
    });
    let (c, p) = (cont.clone(), parar.clone());
    tokio::spawn(async move {
        loop {
            tokio::select! {
                () = p.cancelled() => break,
                () = tokio::time::sleep(Duration::from_secs(300)) => c.log("sombra"),
            }
        }
    });

    if let Some(base) = intervalo_gerador() {
        tracing::info!(intervalo_s = base.as_secs(), "gerador sintético ligado");
        tokio::spawn(gerar(db, base, parar.clone()));
    }

    rodar(&cfg, parar, cont.clone()).await;
    cont.log("sombra encerrada");
    Ok(())
}

/// Números do relatório de um período.
#[derive(Debug, Default, PartialEq)]
pub struct Relatorio {
    pub total: i64,
    pub entregues: i64,
    pub confirmados: i64,
    pub perdidos: i64,
    pub sem_confirmacao: i64,
    pub pendentes: i64,
    pub duplicados: i64,
    pub p50_s: Option<f64>,
    pub p95_s: Option<f64>,
}

impl Relatorio {
    pub fn aprovado(&self) -> bool {
        self.perdidos == 0 && self.duplicados == 0 && self.sem_confirmacao == 0
    }
}

/// Só SELECT. Entregue = já foi reservado por algum consumidor (`reservado_por`, que fica
/// mesmo depois de devolvido). Não há coluna de contagem de entregas; `tentativas` conta cada
/// devolução à fila, então `tentativas >= 1` significa que o comando foi entregue mais de uma vez.
pub async fn relatorio(db: &PgPool, horas: i32, origem: &str) -> Result<Relatorio, sqlx::Error> {
    let l = sqlx::query(
        "SELECT count(*) AS total,
                count(*) FILTER (WHERE reservado_por IS NOT NULL) AS entregues,
                count(*) FILTER (WHERE resultado = 'sombra') AS confirmados,
                count(*) FILTER (WHERE resultado = 'expirado' AND reservado_por IS NULL) AS perdidos,
                count(*) FILTER (WHERE executado_em IS NOT NULL
                                   AND resultado IS DISTINCT FROM 'sombra'
                                   AND NOT (resultado = 'expirado' AND reservado_por IS NULL)
                                ) AS sem_confirmacao,
                count(*) FILTER (WHERE executado_em IS NULL) AS pendentes,
                count(*) FILTER (WHERE tentativas >= 1) AS duplicados,
                (percentile_cont(0.5) WITHIN GROUP
                    (ORDER BY extract(epoch FROM executado_em - criado_em))
                    FILTER (WHERE resultado = 'sombra'))::float8 AS p50,
                (percentile_cont(0.95) WITHIN GROUP
                    (ORDER BY extract(epoch FROM executado_em - criado_em))
                    FILTER (WHERE resultado = 'sombra'))::float8 AS p95
         FROM nexus.comando
         WHERE origem = $2 AND criado_em >= now() - make_interval(hours => $1)",
    )
    .bind(horas)
    .bind(origem)
    .fetch_one(db)
    .await?;
    Ok(Relatorio {
        total: l.try_get("total")?,
        entregues: l.try_get("entregues")?,
        confirmados: l.try_get("confirmados")?,
        perdidos: l.try_get("perdidos")?,
        sem_confirmacao: l.try_get("sem_confirmacao")?,
        pendentes: l.try_get("pendentes")?,
        duplicados: l.try_get("duplicados")?,
        p50_s: l.try_get("p50")?,
        p95_s: l.try_get("p95")?,
    })
}

pub fn imprimir(horas: i32, origem: &str, r: &Relatorio) {
    let t = |v: Option<f64>| v.map_or("-".to_owned(), |s| format!("{s:.2} s"));
    println!("comandos de origem {origem} nas últimas {horas} h");
    println!("  total:                     {}", r.total);
    println!("  entregues:                 {}", r.entregues);
    println!("  confirmados (sombra):      {}", r.confirmados);
    println!("  perdidos (expiraram sem entrega): {}", r.perdidos);
    println!(
        "  sem confirmação (entregues, não confirmados): {}",
        r.sem_confirmacao
    );
    println!("  pendentes (ainda abertos): {}", r.pendentes);
    println!(
        "  entregues mais de 1 vez (tentativas >= 1): {}",
        r.duplicados
    );
    println!(
        "  atraso criação->confirmação p50: {}  p95: {}",
        t(r.p50_s),
        t(r.p95_s)
    );
    println!(
        "  veredito: {}",
        if r.aprovado() {
            "OK (nenhum perdido, duplicado ou sem confirmação)"
        } else {
            "FALHOU"
        }
    );
}

/// Ponto de entrada do subcomando `sombra-relatorio`. `origem`: espelho, sintetico ou todas.
pub async fn executar_relatorio(db: &PgPool, horas: i32, origem: &str) -> anyhow::Result<()> {
    let origens: &[&str] = match origem {
        "espelho" => &["espelho"],
        "sintetico" => &["sintetico"],
        "todas" => &["espelho", "sintetico"],
        o => bail!("--origem inválida: {o} (espelho|sintetico|todas)"),
    };
    for o in origens {
        let r = relatorio(db, horas, o)
            .await
            .context("falha ao consultar")?;
        imprimir(horas, o, &r);
    }
    Ok(())
}

/// Gerador de carga sintética: a cada `base` (jitter ±20%) insere `sombra.ping` direto no banco.
/// O trigger NOTIFY entrega ao próprio cliente sombra. Falha de banco só gera aviso.
pub async fn gerar(db: PgPool, base: Duration, parar: CancellationToken) {
    let mut n: u64 = 0;
    loop {
        tokio::select! {
            () = parar.cancelled() => return,
            () = tokio::time::sleep(com_jitter(base)) => {}
        }
        n += 1;
        let agora = agora_ms();
        let params = serde_json::json!({"n": n, "gerado_em": agora});
        let novo = persistencia::comandos::NovoComando {
            conta: "sombra-teste",
            tipo: "sombra.ping",
            params: &params,
            prioridade: 5,
            origem: "sintetico",
            expira_ms: agora + 600_000,
        };
        if let Err(e) = persistencia::comandos::inserir(&db, &novo).await {
            tracing::warn!(erro = %e, "sombra: falha ao gerar comando sintético");
        }
    }
}

/// Lê NEXUS_SOMBRA_GERAR_S; vazio, 0 ou inválido = desligado.
pub fn intervalo_gerador() -> Option<Duration> {
    let v = std::env::var("NEXUS_SOMBRA_GERAR_S").ok()?;
    match v.trim().parse::<u64>() {
        Ok(s) if s > 0 => Some(Duration::from_secs(s)),
        Ok(_) => None,
        Err(_) if v.trim().is_empty() => None,
        Err(_) => {
            tracing::warn!("NEXUS_SOMBRA_GERAR_S inválida; gerador desligado");
            None
        }
    }
}
