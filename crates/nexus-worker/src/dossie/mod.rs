//! Dossiê: porta do `dossie-server.js` (Node) para o `nexus-worker`, em sombra.
//!
//! - Leituras: mesmas consultas do legado nas tabelas `public` (só SELECT).
//! - Escritas: mesmo SQL do legado, mas em tabelas sombra `nexus.*` e uploads num
//!   diretório próprio. Nada é gravado no `public` nem nos arquivos do legado.
//! - Estáticos: lidos do diretório do legado montado só leitura, com as mesmas regras de
//!   acesso (lista pública + chave).
//! - Precificador (`precos.json`, `calc/lite`, `calc/preco`): `precos.rs` (etapa 6b).
//! - Cálculo do dossiê (`calc/catalogo`, `calc/dossie`): `calc/` (etapa 6c).
//!
//! O despacho imita o `handler` do legado: rota exata por `MÉTODO caminho`, senão estático
//! para GET, senão 405. As respostas JSON saem com `JSON.stringify(obj, null, 2)`.

mod acesso;
mod calc;
mod escrita;
mod estaticos;
pub mod js;
mod jsonjs;
mod leitura;
pub mod paridade;
mod precos;
mod tracker_pvp;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Router;
use futures_util::StreamExt;
use serde_json::{json, Value};
use sqlx::PgPool;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

pub use acesso::Limites;

/// Diretórios e segredos do dossiê.
pub struct Dossie {
    pub db: PgPool,
    /// `data/static` do legado (montado só leitura no mesmo caminho).
    pub static_dir: PathBuf,
    /// Onde a sombra grava os uploads (tmpfs no container).
    pub dados_dir: PathBuf,
    /// `DOSSIE_KEY`. Sem chave, as rotas protegidas ficam bloqueadas (igual ao legado).
    pub chave: Option<String>,
    pub limites: Limites,
    /// Catálogo e `precos.json` do precificador (etapa 6b).
    pub precos: precos::Cache,
    /// Produção (cutover): escritas no `public` e uploads na pasta do legado. `false` = sombra.
    pub escrita: bool,
}

/// A conexão chegou pela porta "privada" (no legado: a interface do Tailscale, que libera
/// as rotas protegidas sem chave). Em JS seria um `req.socket.localAddress === TS_HOST`.
#[derive(Clone, Copy)]
pub struct RedePrivada(pub bool);

impl Dossie {
    pub fn fotos_dir(&self) -> PathBuf {
        self.static_dir.join("fotos")
    }
}

/// Pedido já desmontado, como o handler do legado vê.
pub struct Pedido {
    pub metodo: Method,
    /// `req.url.split('?')[0]`, sem decodificar.
    pub caminho: String,
    /// `new URL(req.url).searchParams`.
    pub params: Vec<(String, String)>,
    pub headers: HeaderMap,
    pub ip: String,
    /// Veio pela porta privada.
    pub rede_privada: bool,
}

impl Pedido {
    /// `searchParams.get(nome)`: primeiro valor, ou `None`.
    pub fn param(&self, nome: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|(k, _)| k == nome)
            .map(|(_, v)| v.as_str())
    }

    pub fn header(&self, nome: &str) -> Option<&str> {
        self.headers.get(nome).and_then(|v| v.to_str().ok())
    }
}

/// `readBody`: JSON até 5 MB; corpo inválido vira `{}`.
/// O corpo anda separado do `Pedido`: `Body` não é `Sync`, e o `&Pedido` atravessa `.await`.
pub async fn ler_json(corpo: Body) -> Result<Value, Erro> {
    let bytes = ler_corpo(corpo, 5_000_000, "body muito grande").await?;
    Ok(serde_json::from_slice(&bytes).unwrap_or_else(|_| json!({})))
}

/// Erro de rota: vira `{ erro }` com o código, como o `catch` do legado.
#[derive(Debug)]
pub struct Erro {
    pub codigo: u16,
    pub msg: String,
}

impl Erro {
    pub fn novo(codigo: u16, msg: impl Into<String>) -> Self {
        Self {
            codigo,
            msg: msg.into(),
        }
    }
}

impl From<sqlx::Error> for Erro {
    fn from(e: sqlx::Error) -> Self {
        // O node-pg põe o SQLSTATE em `e.code`; o legado faz `parseInt(e.code)` e só usa
        // se ficar entre 100 e 599 (ex.: 428C9 → 428). Mensagem = a do Postgres.
        match e.as_database_error() {
            Some(db) => {
                let codigo = db
                    .code()
                    .and_then(|c| js::parse_int(&c))
                    .filter(|c| (100..600).contains(c))
                    .and_then(|c| u16::try_from(c).ok())
                    .unwrap_or(500);
                Self::novo(codigo, db.message())
            }
            None => Self::novo(500, e.to_string()),
        }
    }
}

/// `send(res, obj, code)` do legado.
pub fn enviar(v: &Value, codigo: u16) -> Response {
    let corpo = jsonjs::bonito(v);
    (
        StatusCode::from_u16(codigo).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
        [(header::CONTENT_TYPE, "application/json; charset=utf-8")],
        corpo,
    )
        .into_response()
}

/// `res.writeHead(code); res.end(texto)`: sem content-type.
pub fn cru(codigo: u16, texto: impl Into<String>) -> Response {
    let mut r = Response::new(Body::from(texto.into()));
    *r.status_mut() = StatusCode::from_u16(codigo).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    r
}

/// Lê o corpo inteiro com teto de bytes; passou do teto → 413 com a mensagem do legado.
pub async fn ler_corpo(corpo: Body, max: usize, msg: &str) -> Result<Vec<u8>, Erro> {
    let mut fluxo = corpo.into_data_stream();
    let mut buf = Vec::new();
    while let Some(parte) = fluxo.next().await {
        let parte = parte.map_err(|e| Erro::novo(500, e.to_string()))?;
        if buf.len() + parte.len() > max {
            return Err(Erro::novo(413, msg));
        }
        buf.extend_from_slice(&parte);
    }
    Ok(buf)
}

pub fn router(d: Arc<Dossie>) -> Router {
    Router::new().fallback(atender).with_state(d)
}

/// Sobe o servidor do dossiê. Erro de bind só é logado: o ETL continua.
pub async fn servir(porta: u16, d: Arc<Dossie>, parar: CancellationToken, privada: bool) {
    let addr = SocketAddr::from(([0, 0, 0, 0], porta));
    let listener = match TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            error!(erro = %e, porta, "dossiê: não consegui abrir a porta");
            return;
        }
    };
    info!(porta, privada, "dossiê ouvindo");
    let app = router(d)
        .layer(axum::Extension(RedePrivada(privada)))
        .into_make_service_with_connect_info::<SocketAddr>();
    if let Err(e) = axum::serve(listener, app)
        .with_graceful_shutdown(parar.cancelled_owned())
        .await
    {
        error!(erro = %e, "dossiê: servidor caiu");
    }
}

async fn atender(
    State(d): State<Arc<Dossie>>,
    ConnectInfo(remoto): ConnectInfo<SocketAddr>,
    axum::Extension(RedePrivada(rede_privada)): axum::Extension<RedePrivada>,
    req: Request,
) -> Response {
    let (partes, corpo) = req.into_parts();
    let caminho = partes.uri.path().to_owned();
    let params = partes
        .uri
        .query()
        .map(|q| form_urlencoded::parse(q.as_bytes()).into_owned().collect())
        .unwrap_or_default();
    let xff = partes
        .headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .map(str::trim)
        .unwrap_or_default();
    let ip = if xff.is_empty() {
        remoto.ip().to_string()
    } else {
        xff.to_owned()
    };
    let p = Pedido {
        metodo: partes.method,
        caminho,
        params,
        headers: partes.headers,
        ip,
        rede_privada,
    };

    let get = p.metodo == Method::GET;
    if p.metodo == Method::OPTIONS {
        let mut r = cru(204, "");
        if !p.headers.contains_key(header::ORIGIN) {
            let h = r.headers_mut();
            h.insert(
                header::ACCESS_CONTROL_ALLOW_ORIGIN,
                HeaderValue::from_static("null"),
            );
            h.insert(
                header::ACCESS_CONTROL_ALLOW_METHODS,
                HeaderValue::from_static("GET"),
            );
            h.insert(
                header::ACCESS_CONTROL_ALLOW_HEADERS,
                HeaderValue::from_static("Content-Type"),
            );
        }
        return r;
    }

    let mut r = match despachar(&d, &p, corpo).await {
        Ok(r) => r,
        Err(e) => enviar(&json!({ "erro": e.msg }), e.codigo),
    };
    if get {
        r.headers_mut().insert(
            header::ACCESS_CONTROL_ALLOW_ORIGIN,
            HeaderValue::from_static("*"),
        );
    }
    metrics::counter!("nexus_dossie_respostas_total", "status" => r.status().as_u16().to_string())
        .increment(1);
    r
}

async fn despachar(d: &Dossie, p: &Pedido, corpo: Body) -> Result<Response, Erro> {
    let rota = format!("{} {}", p.metodo, p.caminho);
    match rota.as_str() {
        "GET /robots.txt" => Ok(estaticos::robots(p)),
        "GET /sitemap.xml" => estaticos::sitemap(d).await,
        "POST /api/dossie/foto" => escrita::foto(d, p, corpo).await,
        "POST /api/dossie/arquivo" => escrita::arquivo(d, p, corpo).await,
        "POST /api/dossie/vendas" => escrita::vendas(d, p, corpo).await,
        "DELETE /api/dossie/foto" => escrita::apagar_foto(d, p).await,
        "POST /api/dossie/ficha" => escrita::ficha(d, p, corpo).await,
        "DELETE /api/dossie/ficha" => escrita::apagar_ficha(d, p).await,
        "GET /api/dossie/fotos" => leitura::fotos(d).await,
        "GET /api/dossie/fichas" => leitura::fichas(d, p).await,
        "GET /api/dossie/especie" => leitura::especie(d, p).await,
        "GET /api/dossie/dataset" => leitura::dataset(d).await,
        "GET /api/dossie/cacada-medidas" => leitura::cacada_medidas(d, p).await,
        "GET /api/dossie/cacada-contas" => {
            if !acesso::privado(d, p) {
                return Ok(enviar(&json!({"contas":[]}), 200));
            }
            let contas:Vec<String>=sqlx::query_scalar("SELECT janela->>'nick' FROM nexus.motor_janela WHERE em>now()-interval '90 seconds' ORDER BY conta").fetch_all(&d.db).await?;
            Ok(enviar(&json!({"contas":contas}), 200))
        }
        "POST /api/dossie/cacada-previsao" => {
            if !d.limites.ok(&p.ip, 30) {
                return Ok(enviar(&json!({"erro":"muitas requisições"}), 429));
            }
            let b = ler_json(corpo).await?;
            let ficha = b.get("ficha").filter(|f| f.is_object()).cloned();
            let nick = if acesso::privado(d, p) {
                b["conta"]
                    .as_str()
                    .filter(|s| !s.is_empty() && s.len() <= 128)
                    .map(str::to_owned)
            } else {
                None
            };
            let nick = match nick {
                Some(n) => n,
                None => {
                    let f = ficha.as_ref().ok_or_else(|| {
                        Erro::novo(400, "Cole a ficha completa para identificar a amostra.")
                    })?;
                    let janelas:Vec<serde_json::Value>=sqlx::query_scalar("SELECT janela FROM nexus.motor_janela WHERE em>now()-interval '90 seconds'").fetch_all(&d.db).await?;
                    let mut matches: Vec<(f64, String)> = janelas
                        .iter()
                        .filter_map(|j| {
                            let a = &j["ultimo"]["io"]["active"];
                            if !["speciesId", "quality", "potencia", "shiny", "ivs"]
                                .iter()
                                .all(|k| f.get(*k) == a.get(*k))
                            {
                                return None;
                            }
                            let delta = (f["level"].as_f64()? - a["level"].as_f64()?).abs();
                            Some((delta, j["nick"].as_str()?.to_owned()))
                        })
                        .collect();
                    matches.sort_by(|a, b| a.0.total_cmp(&b.0));
                    if matches.is_empty() || matches.get(1).is_some_and(|m| m.0 == matches[0].0) {
                        return Ok(enviar(
                            &json!({"erro":"Não encontrei uma amostra única compatível. Copie a ficha atual completa do Pokémon."}),
                            400,
                        ));
                    }
                    matches.remove(0).1
                }
            };
            let entrada = persistencia::cacada_contexto::entradas(
                &d.db,
                &nick,
                chrono::Utc::now().timestamp_millis(),
            )
            .await?
            .ok_or_else(|| Erro::novo(404, "Aguardando dados recentes da conta."))?;
            let resultado = tokio::task::spawn_blocking(move || entrada.calcular(ficha))
                .await
                .map_err(|e| Erro::novo(500, e.to_string()))?;
            match resultado {
                Ok(mut v) => {
                    if !acesso::privado(d, p) {
                        if let Some(m) = v.as_object_mut() {
                            m.retain(|k, _| {
                                [
                                    "ranking",
                                    "melhorAgora",
                                    "calibracao",
                                    "bonusAtual",
                                    "multXpObservado",
                                    "multXpPokemonObservado",
                                    "nivelTreinador",
                                    "dia",
                                    "notaModelo",
                                    "comparacaoXp",
                                ]
                                .contains(&k.as_str())
                            });
                        }
                    }
                    Ok(enviar(&v, 200))
                }
                Err(e) => Ok(enviar(&json!({"erro":e}), 400)),
            }
        }
        "GET /api/ia/memoria"
        | "GET /api/ia/playbook"
        | "GET /api/ia/performance"
        | "GET /api/ia/estado" => leitura::ia(d, p),
        "GET /api/dossie/pvp/presenca" => tracker_pvp::presenca(d, p).await,
        "GET /api/dossie/calc/catalogo" => Ok(enviar(&calc::catalogo()?, 200)),
        "POST /api/dossie/calc/dossie" => {
            if !d.limites.ok(&p.ip, 120) {
                return Ok(enviar(
                    &json!({ "erro": "muitas requisições, tenta de novo em 1 min" }),
                    429,
                ));
            }
            let b = ler_json(corpo).await?;
            // O cálculo é CPU puro: sai da thread do servidor para não travar outras rotas.
            let r = tokio::task::spawn_blocking(move || calc::calcular(&b))
                .await
                .map_err(|e| Erro::novo(500, e.to_string()))??;
            Ok(enviar(&r, 200))
        }
        "GET /api/dossie/calc/lite" => precos::rota_lite(d, p).await,
        "POST /api/dossie/calc/preco" => precos::rota_preco(d, p, corpo).await,
        "GET /precificador/precos.json" => {
            if !acesso::privado(d, p) {
                return Ok(cru(404, "404"));
            }
            precos::rota_precos(d).await
        }
        _ if p.metodo != Method::GET => Ok(cru(405, "")),
        _ => estaticos::servir(d, p).await,
    }
}
