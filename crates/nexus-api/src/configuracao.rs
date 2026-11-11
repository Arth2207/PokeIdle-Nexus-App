//! Configuração do processo, lida de variáveis de ambiente com prefixo `NEXUS_`.
//!
//! | Variável              | Obrigatória | Padrão      |
//! |-----------------------|-------------|-------------|
//! | `NEXUS_DATABASE_URL`  | sim*        | —           |
//! | `NEXUS_ENDERECO`      | não         | `127.0.0.1` |
//! | `NEXUS_PORTA`         | não         | `8190`      |
//! | `NEXUS_PORTA_METRICAS`| não         | `9190`      |
//! | `NEXUS_LOG_FORMATO`   | não         | `texto`     |
//! | `NEXUS_ESCRITA_BOLSA` | não         | `false`     |
//! | `NEXUS_ESCRITA_EQUIPES` | não       | `false`     |
//! | `NEXUS_CPU_THREADS`   | não         | `2`         |
//! | `NEXUS_MODELO`        | não         | `legado`    |
//! | `NEXUS_MELHORIAS`     | não         | `false`     |
//! | `NEXUS_CORS_ORIGENS`  | não         | (nenhuma)   |
//! | `NEXUS_LIMITE_RPS`    | não         | `10`        |
//! | `NEXUS_LIMITE_RAJADA` | não         | `40`        |
//! | `NEXUS_BOLSA_CICLO_S` | não         | `180`       |
//! | `NEXUS_PARIDADE_URL`  | não         | (desligado) |
//! | `NEXUS_PARIDADE_CICLO_S` | não      | `600`       |
//! | `NEXUS_PARIDADE_EQUIPE` | não       | `false`     |
//!
//! - `NEXUS_ESCRITA_BOLSA`: liga as rotas de ingestão da bolsa e a limpeza de ofertas. Elas
//!   gravam em tabelas do legado; só ligar no cutover da fase 1.
//! - `NEXUS_ESCRITA_EQUIPES`: liga o `POST /api/equipes/ingest`. Até o cutover da fase 2 o
//!   relay manda para o legado, e o estado chega pela importação.
//! - `NEXUS_MELHORIAS`: liga o que diverge do legado de propósito: busca exata nos perfis de
//!   equipe, chance de vitória, ordem de compra e preços típicos corrigidos. Quebra a paridade
//!   (o comparador vai acusar), então só depois do cutover da fase 2.
//! - `NEXUS_MODELO`: modelo de combate das respostas (`legado` ou `jogo`). O outro roda ao
//!   lado só nas métricas. Trocar para `jogo` só com lutas reais suficientes para calibrar.
//! - `NEXUS_CPU_THREADS`: tamanho do pool de CPU do cálculo de equipes (fora do tokio).
//! - `NEXUS_CORS_ORIGENS`: origens do painel web, separadas por vírgula.
//! - `NEXUS_LIMITE_*`: rate limit por token (requisições por segundo e rajada).
//! - `NEXUS_PARIDADE_URL`: URL do legado (ex.: `http://host:8181`). Liga o comparador.
//! - `NEXUS_PARIDADE_EQUIPE`: o comparador também chama `/api/bolsa/equipe` e
//!   `/equipe-propria` no legado, o que dispara o cálculo pesado dele. Desligado por padrão.
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
    #[serde(default)]
    pub escrita_bolsa: bool,
    #[serde(default)]
    pub escrita_equipes: bool,
    #[serde(default)]
    pub paridade_equipe: bool,
    #[serde(default)]
    pub melhorias: bool,
    #[serde(default = "modelo_padrao")]
    pub modelo: String,
    #[serde(default = "cpu_threads_padrao")]
    pub cpu_threads: usize,
    #[serde(default)]
    pub cors_origens: String,
    #[serde(default = "limite_rps_padrao")]
    pub limite_rps: f64,
    #[serde(default = "limite_rajada_padrao")]
    pub limite_rajada: f64,
    #[serde(default = "bolsa_ciclo_padrao")]
    pub bolsa_ciclo_s: u64,
    #[serde(default)]
    pub paridade_url: Option<String>,
    #[serde(default = "paridade_ciclo_padrao")]
    pub paridade_ciclo_s: u64,
}

impl Configuracao {
    /// Origens do CORS, já separadas.
    pub fn origens_cors(&self) -> Vec<String> {
        self.cors_origens
            .split(',')
            .map(str::trim)
            .filter(|o| !o.is_empty())
            .map(str::to_owned)
            .collect()
    }
}

fn modelo_padrao() -> String {
    "legado".into()
}

fn cpu_threads_padrao() -> usize {
    2
}

fn limite_rps_padrao() -> f64 {
    10.0
}

fn limite_rajada_padrao() -> f64 {
    40.0
}

fn bolsa_ciclo_padrao() -> u64 {
    180
}

fn paridade_ciclo_padrao() -> u64 {
    600
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
