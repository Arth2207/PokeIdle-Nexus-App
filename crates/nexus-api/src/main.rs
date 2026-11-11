//! nexus-api: servidor HTTP e WebSocket do Nexus.
//!
//! Uso:
//!   nexus-api                                  sobe o servidor
//!   nexus-api migrar                           aplica as migrations pendentes
//!   nexus-api token criar <nome> <escopos>     cria um token (escopos separados por vírgula)
//!   nexus-api catalogo importar < itens.json   grava o catálogo de itens do jogo (lido do stdin)
//!   nexus-api healthcheck                      checa o /health/ready local (usado pelo Docker)

mod auth;
mod bolsa;
mod configuracao;
mod desligamento;
mod estado;
mod metricas;
mod rotas;
mod telemetria;

use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;

use anyhow::{bail, ensure, Context};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_util::sync::CancellationToken;

use crate::auth::Escopo;
use crate::configuracao::Configuracao;
use crate::estado::Estado;

const USO: &str =
    "uso: nexus-api [migrar | token criar <nome> <escopos> | catalogo importar | healthcheck]";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Em desenvolvimento lê o arquivo .env. Em produção ele não existe, e tudo bem.
    dotenvy::dotenv().ok();

    let cfg = Configuracao::carregar().context("configuração inválida")?;
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();

    match args.as_slice() {
        [] => servir(cfg).await,
        ["migrar"] => migrar(&cfg).await,
        ["token", "criar", nome, escopos] => criar_token(&cfg, nome, escopos).await,
        ["catalogo", "importar"] => importar_catalogo(&cfg).await,
        ["healthcheck"] => healthcheck(&cfg).await,
        _ => bail!(USO),
    }
}

async fn servir(cfg: Configuracao) -> anyhow::Result<()> {
    telemetria::iniciar(cfg.log_formato).context("não consegui iniciar os logs")?;
    tracing::info!(?cfg, "configuração carregada");

    let metricas = metricas::instalar().context("não consegui iniciar as métricas")?;
    let estado = Estado::novo(&cfg).context("NEXUS_DATABASE_URL inválida")?;

    let app = rotas::router(estado.clone());
    let app_metricas = metricas::router(metricas, estado);

    let endereco = SocketAddr::new(cfg.endereco, cfg.porta);
    let endereco_metricas = SocketAddr::new(cfg.endereco, cfg.porta_metricas);
    let listener = abrir(endereco).await?;
    let listener_metricas = abrir(endereco_metricas).await?;
    tracing::info!(%endereco, %endereco_metricas, "nexus-api ouvindo");

    // Um único sinal de parada para os dois servidores.
    let parar = CancellationToken::new();
    tokio::spawn({
        let parar = parar.clone();
        async move {
            desligamento::sinal_de_parada().await;
            parar.cancel();
        }
    });

    let api = axum::serve(listener, app).with_graceful_shutdown(parar.clone().cancelled_owned());
    let met = axum::serve(listener_metricas, app_metricas)
        .with_graceful_shutdown(parar.cancelled_owned());
    tokio::try_join!(api, met).context("servidor HTTP falhou")?;

    tracing::info!("nexus-api encerrado");
    Ok(())
}

async fn abrir(endereco: SocketAddr) -> anyhow::Result<TcpListener> {
    TcpListener::bind(endereco)
        .await
        .with_context(|| format!("não consegui abrir {endereco}"))
}

async fn migrar(cfg: &Configuracao) -> anyhow::Result<()> {
    let estado = Estado::novo(cfg).context("NEXUS_DATABASE_URL inválida")?;
    sqlx::migrate!("../../migrations")
        .run(&estado.db)
        .await
        .context("falha ao aplicar as migrations")?;
    println!("migrations aplicadas");
    Ok(())
}

async fn criar_token(cfg: &Configuracao, nome: &str, escopos: &str) -> anyhow::Result<()> {
    let escopos = escopos
        .split(',')
        .map(|e| e.trim().parse::<Escopo>())
        .collect::<Result<Vec<_>, _>>()?;

    let estado = Estado::novo(cfg).context("NEXUS_DATABASE_URL inválida")?;
    let token = auth::criar_token(&estado.db, nome, &escopos).await?;

    println!("token criado para {nome:?} com escopos {escopos:?}");
    println!("guarde agora, ele não será mostrado de novo:\n\n{token}\n");
    Ok(())
}

/// Formato do `items-catalogo.json` do legado: `{ "t": ..., "itens": [{ id, nome, ... }] }`.
#[derive(serde::Deserialize)]
struct ArquivoCatalogo {
    itens: Vec<ItemArquivo>,
}

#[derive(serde::Deserialize)]
struct ItemArquivo {
    id: i32,
    nome: String,
    categoria: Option<String>,
    #[serde(default)]
    raro: bool,
    npc: Option<i64>,
    icone: Option<String>,
}

async fn importar_catalogo(cfg: &Configuracao) -> anyhow::Result<()> {
    // Leitura bloqueante do stdin: é um comando de linha, roda uma vez e sai.
    let texto = std::io::read_to_string(std::io::stdin()).context("não consegui ler o stdin")?;
    let arquivo: ArquivoCatalogo =
        serde_json::from_str(&texto).context("JSON do catálogo inválido")?;
    ensure!(
        !arquivo.itens.is_empty(),
        "o catálogo veio vazio; nada foi alterado"
    );

    let itens: Vec<_> = arquivo
        .itens
        .into_iter()
        .map(|i| nucleo::bolsa::catalogo::ItemCatalogo {
            id: i.id,
            nome: i.nome,
            categoria: i.categoria,
            raro: i.raro,
            preco_npc: i.npc,
            icone: i.icone,
        })
        .collect();

    let estado = Estado::novo(cfg).context("NEXUS_DATABASE_URL inválida")?;
    let gravados = persistencia::PgRepoMercado::novo(estado.db)
        .gravar_catalogo(&itens)
        .await
        .context("falha ao gravar o catálogo")?;
    println!("catálogo importado: {gravados} itens");
    Ok(())
}

/// Faz um GET mínimo em /health/ready. A imagem final não tem curl nem shell,
/// então o próprio binário faz o papel de cliente.
async fn healthcheck(cfg: &Configuracao) -> anyhow::Result<()> {
    let checagem = async {
        let mut conexao = TcpStream::connect((Ipv4Addr::LOCALHOST, cfg.porta)).await?;
        conexao
            .write_all(b"GET /health/ready HTTP/1.0\r\nHost: localhost\r\n\r\n")
            .await?;
        let mut inicio = [0u8; 12];
        conexao.read_exact(&mut inicio).await?;
        anyhow::Ok(inicio)
    };

    let inicio = tokio::time::timeout(Duration::from_secs(3), checagem)
        .await
        .context("healthcheck: sem resposta em 3s")??;
    ensure!(
        inicio.ends_with(b" 200"),
        "healthcheck: resposta {:?}",
        String::from_utf8_lossy(&inicio)
    );
    Ok(())
}
