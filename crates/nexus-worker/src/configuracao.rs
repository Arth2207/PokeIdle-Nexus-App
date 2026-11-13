//! Configuração do worker, lida de variáveis de ambiente com prefixo `NEXUS_`.
//!
//! | Variável                   | Obrigatória | Padrão               |
//! |----------------------------|-------------|----------------------|
//! | `NEXUS_DATABASE_URL`       | sim*        | —                    |
//! | `NEXUS_DATABASE_URL_FILE`  | sim*        | —                    |
//! | `NEXUS_PORTA_METRICAS`     | não         | `9191`               |
//! | `NEXUS_LOG_FORMATO`        | não         | `texto`              |
//! | `NEXUS_SQLITE_PATH`        | não         | `/opt/mapper/data/mapa.db` |
//! | `NEXUS_ETL_INTERVALO_S`    | não         | `300`                |
//! | `NEXUS_ETL_EFIC_INTERVALO_S` | não       | `3600`               |
//! | `NEXUS_ESCRITA_ETL`        | não         | `false`              |
//! | `NEXUS_DOSSIE_PORTA`       | não         | `0` (desligado)      |
//! | `NEXUS_DOSSIE_STATIC`      | não         | `/opt/mapper/data/static` |
//! | `NEXUS_DOSSIE_DADOS`       | não         | `/dados`             |
//! | `NEXUS_DOSSIE_CHAVE_FILE`  | não         | — (rotas com chave bloqueadas) |
//! | `NEXUS_DOSSIE_LEGADO_URL`  | não         | `http://127.0.0.1:8899` |
//! | `NEXUS_DOSSIE_NEXUS_URL`   | não         | `http://127.0.0.1:18899` |
//! | `NEXUS_DOSSIE_PARIDADE_CICLO_S` | não    | `1800`               |
//! | `NEXUS_DOSSIE_LEGADO_PRIVADO_URL` | não  | — (casos privados pulados) |
//!
//! * Uma das duas formas de DATABASE_URL é obrigatória.
//!
//! - `NEXUS_ESCRITA_ETL`: quando `true`, o worker grava em `public.fato_kill`,
//!   `public.fato_hunt_eficiencia` e `public.fato_sessao_contexto` (cutover da fase 5).
//!   Enquanto `false`, só grava nas tabelas shadow do schema `nexus`.

use std::fs;
use std::time::Duration;

use anyhow::{bail, Context};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatoLog {
    Json,
    Texto,
}

#[derive(Debug, Clone)]
pub struct Configuracao {
    pub database_url: String,
    pub porta_metricas: u16,
    pub log_formato: FormatoLog,
    pub sqlite_path: String,
    /// Intervalo entre ciclos do ETL contínuo.
    pub etl_intervalo: Duration,
    /// Intervalo entre ciclos do ETL de eficiência.
    pub etl_efic_intervalo: Duration,
    /// Liga a escrita nas tabelas `public` (cutover). Enquanto `false`, só shadow.
    pub escrita_etl: bool,
    /// Porta do dossiê em sombra; 0 = não sobe.
    pub dossie_porta: u16,
    pub dossie_static: String,
    pub dossie_dados: String,
    /// Arquivo com a `DOSSIE_KEY` (o valor não fica na struct, que vai para log).
    pub dossie_chave_arquivo: Option<String>,
    pub dossie_legado_url: String,
    pub dossie_nexus_url: String,
    pub dossie_paridade_ciclo: Duration,
    /// Legado pela interface do Tailscale, para as rotas privadas (`precos.json`).
    pub dossie_legado_privado_url: Option<String>,
}

impl Configuracao {
    pub fn carregar() -> anyhow::Result<Self> {
        let database_url = ler_database_url()?;
        let porta_metricas = var_u16("NEXUS_PORTA_METRICAS", 9191)?;
        let log_formato = match std::env::var("NEXUS_LOG_FORMATO")
            .unwrap_or_default()
            .as_str()
        {
            "json" => FormatoLog::Json,
            _ => FormatoLog::Texto,
        };
        let sqlite_path = std::env::var("NEXUS_SQLITE_PATH")
            .unwrap_or_else(|_| "/opt/mapper/data/mapa.db".to_owned());
        let etl_intervalo = Duration::from_secs(var_u64("NEXUS_ETL_INTERVALO_S", 300)?);
        let etl_efic_intervalo = Duration::from_secs(var_u64("NEXUS_ETL_EFIC_INTERVALO_S", 3600)?);
        let escrita_etl = var_bool("NEXUS_ESCRITA_ETL", false)?;
        let texto = |nome: &str, padrao: &str| {
            std::env::var(nome)
                .ok()
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| padrao.to_owned())
        };

        Ok(Self {
            database_url,
            porta_metricas,
            log_formato,
            sqlite_path,
            etl_intervalo,
            etl_efic_intervalo,
            escrita_etl,
            dossie_porta: var_u16("NEXUS_DOSSIE_PORTA", 0)?,
            dossie_static: texto("NEXUS_DOSSIE_STATIC", "/opt/mapper/data/static"),
            dossie_dados: texto("NEXUS_DOSSIE_DADOS", "/dados"),
            dossie_chave_arquivo: std::env::var("NEXUS_DOSSIE_CHAVE_FILE")
                .ok()
                .filter(|v| !v.is_empty()),
            dossie_legado_url: texto("NEXUS_DOSSIE_LEGADO_URL", "http://127.0.0.1:8899"),
            dossie_nexus_url: texto("NEXUS_DOSSIE_NEXUS_URL", "http://127.0.0.1:18899"),
            dossie_legado_privado_url: std::env::var("NEXUS_DOSSIE_LEGADO_PRIVADO_URL")
                .ok()
                .filter(|v| !v.is_empty()),
            dossie_paridade_ciclo: Duration::from_secs(var_u64(
                "NEXUS_DOSSIE_PARIDADE_CICLO_S",
                1800,
            )?),
        })
    }
}

fn ler_database_url() -> anyhow::Result<String> {
    if let Ok(url) = std::env::var("NEXUS_DATABASE_URL") {
        if !url.is_empty() {
            return Ok(url);
        }
    }
    if let Ok(path) = std::env::var("NEXUS_DATABASE_URL_FILE") {
        let conteudo = fs::read_to_string(&path)
            .with_context(|| format!("não consegui ler NEXUS_DATABASE_URL_FILE={path}"))?;
        let url = conteudo.trim().to_owned();
        if !url.is_empty() {
            return Ok(url);
        }
    }
    bail!("NEXUS_DATABASE_URL ou NEXUS_DATABASE_URL_FILE é obrigatório")
}

fn var_u16(nome: &str, padrao: u16) -> anyhow::Result<u16> {
    match std::env::var(nome) {
        Ok(v) => v
            .parse()
            .with_context(|| format!("{nome} deve ser um número entre 1 e 65535")),
        Err(_) => Ok(padrao),
    }
}

fn var_u64(nome: &str, padrao: u64) -> anyhow::Result<u64> {
    match std::env::var(nome) {
        Ok(v) => v
            .parse()
            .with_context(|| format!("{nome} deve ser um número inteiro positivo")),
        Err(_) => Ok(padrao),
    }
}

fn var_bool(nome: &str, padrao: bool) -> anyhow::Result<bool> {
    match std::env::var(nome).as_deref() {
        Ok("true" | "1") => Ok(true),
        Ok("false" | "0") => Ok(false),
        Ok(outro) => bail!("{nome} deve ser true ou false, não {outro:?}"),
        Err(_) => Ok(padrao),
    }
}
