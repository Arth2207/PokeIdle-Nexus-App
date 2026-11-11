//! Configuração do processo, lida de variáveis de ambiente com prefixo `NEXUS_`.
//!
//! | Variável              | Obrigatória | Padrão      |
//! |-----------------------|-------------|-------------|
//! | `NEXUS_DATABASE_URL`  | sim*        | —           |
//! | `NEXUS_ENDERECO`      | não         | `127.0.0.1` |
//! | `NEXUS_PORTA`         | não         | `8190`      |
//! | `NEXUS_PORTA_METRICAS`| não         | `9190`      |
//! | `NEXUS_LOG_FORMATO`   | não         | `texto`     |
//!
//! \* Ou `NEXUS_DATABASE_URL_FILE` com o caminho de um arquivo que contém a URL
//! (é assim que o Docker entrega secrets, em `/run/secrets/`).

use std::fmt;
use std::net::{IpAddr, Ipv4Addr};

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Configuracao {
    #[serde(default = "endereco_padrao")]
    pub endereco: IpAddr,
    #[serde(default = "porta_padrao")]
    pub porta: u16,
    #[serde(default = "porta_metricas_padrao")]
    pub porta_metricas: u16,
    pub database_url: Segredo,
    #[serde(default)]
    pub log_formato: FormatoLog,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FormatoLog {
    /// Legível para humanos, para desenvolvimento.
    #[default]
    Texto,
    /// Uma linha JSON por evento, para produção.
    Json,
}

/// Texto sensível (senha, URL com credencial) que nunca aparece em log.
#[derive(Clone, Deserialize)]
#[serde(transparent)]
pub struct Segredo(String);

impl Segredo {
    /// Acesso explícito ao valor. Fica fácil de achar com uma busca no código.
    pub fn expor(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Segredo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Segredo(***)")
    }
}

impl Configuracao {
    pub fn carregar() -> Result<Self, ErroConfiguracao> {
        let mut construtor = config::Config::builder()
            .add_source(config::Environment::with_prefix("NEXUS").try_parsing(true));

        if let Ok(caminho) = std::env::var("NEXUS_DATABASE_URL_FILE") {
            let url = std::fs::read_to_string(&caminho)
                .map_err(|fonte| ErroConfiguracao::Arquivo { caminho, fonte })?;
            construtor = construtor.set_override("database_url", url.trim())?;
        }

        Ok(construtor.build()?.try_deserialize()?)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ErroConfiguracao {
    #[error("não consegui ler o arquivo {caminho}")]
    Arquivo {
        caminho: String,
        #[source]
        fonte: std::io::Error,
    },
    #[error(transparent)]
    Config(#[from] config::ConfigError),
}

fn endereco_padrao() -> IpAddr {
    IpAddr::V4(Ipv4Addr::LOCALHOST)
}

fn porta_padrao() -> u16 {
    8190
}

fn porta_metricas_padrao() -> u16 {
    9190
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn segredo_nao_vaza_no_debug() {
        let s = Segredo("senha-super-secreta".to_string());
        assert_eq!(format!("{s:?}"), "Segredo(***)");
    }
}
