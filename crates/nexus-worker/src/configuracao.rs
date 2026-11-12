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

        Ok(Self {
            database_url,
            porta_metricas,
            log_formato,
            sqlite_path,
            etl_intervalo,
            etl_efic_intervalo,
            escrita_etl,
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
