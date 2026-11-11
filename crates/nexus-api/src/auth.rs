//! Autenticação por token Bearer.
//!
//! O token tem o formato `nxs_<64 hex>`. O banco guarda só o SHA-256 dele, então um
//! vazamento da tabela não entrega tokens utilizáveis.

use std::fmt;
use std::str::FromStr;

use axum::extract::{Request, State};
use axum::http::{header, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::PgPool;

use crate::estado::Estado;

/// Prefixo fixo: facilita reconhecer um token vazado (em log, em commit) por busca.
const PREFIXO: &str = "nxs_";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Escopo {
    Leitura,
    Escrita,
    Relay,
    Admin,
}

impl Escopo {
    pub fn como_str(self) -> &'static str {
        match self {
            Escopo::Leitura => "leitura",
            Escopo::Escrita => "escrita",
            Escopo::Relay => "relay",
            Escopo::Admin => "admin",
        }
    }
}

impl fmt::Display for Escopo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.como_str())
    }
}

impl FromStr for Escopo {
    type Err = ErroEscopo;

    fn from_str(texto: &str) -> Result<Self, Self::Err> {
        match texto {
            "leitura" => Ok(Escopo::Leitura),
            "escrita" => Ok(Escopo::Escrita),
            "relay" => Ok(Escopo::Relay),
            "admin" => Ok(Escopo::Admin),
            outro => Err(ErroEscopo(outro.to_owned())),
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("escopo desconhecido: {0:?} (use leitura, escrita, relay ou admin)")]
pub struct ErroEscopo(String);

/// Quem fez a requisição. O middleware coloca isto nas extensões da requisição.
#[derive(Debug, Clone, Serialize)]
pub struct Cliente {
    pub nome: String,
    pub escopos: Vec<Escopo>,
}

#[derive(Debug, thiserror::Error)]
pub enum ErroCriarToken {
    #[error("não consegui gerar bytes aleatórios: {0}")]
    Aleatorio(#[from] getrandom::Error),
    #[error("já existe um token com o nome {0:?}")]
    NomeRepetido(String),
    #[error(transparent)]
    Banco(#[from] sqlx::Error),
}

/// Gera um token novo, grava o hash e devolve o token em texto. Ele não pode ser recuperado depois.
pub async fn criar_token(
    db: &PgPool,
    nome: &str,
    escopos: &[Escopo],
) -> Result<String, ErroCriarToken> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes)?;
    let token = format!("{PREFIXO}{}", hex::encode(bytes));

    let escopos: Vec<String> = escopos.iter().map(|e| e.como_str().to_owned()).collect();
    let resultado = sqlx::query!(
        "INSERT INTO nexus.api_token (nome, hash, escopos) VALUES ($1, $2, $3)",
        nome,
        &hash_token(&token)[..],
        &escopos,
    )
    .execute(db)
    .await;

    match resultado {
        Ok(_) => Ok(token),
        Err(sqlx::Error::Database(erro)) if erro.is_unique_violation() => {
            Err(ErroCriarToken::NomeRepetido(nome.to_owned()))
        }
        Err(erro) => Err(erro.into()),
    }
}

/// Middleware: só deixa passar requisições com token válido.
pub async fn exigir_token(State(estado): State<Estado>, mut req: Request, next: Next) -> Response {
    // `extrair_bearer` devolve um &str que aponta para dentro de `req`. Calculamos o hash
    // (um valor próprio, [u8; 32]) e o empréstimo acaba aqui, liberando `req` para ser
    // modificado logo abaixo.
    let Some(hash) = extrair_bearer(&req).map(hash_token) else {
        return nao_autorizado("token ausente ou mal formado");
    };

    match buscar_cliente(&estado.db, &hash).await {
        Ok(Some(cliente)) => {
            tracing::Span::current().record("cliente", cliente.nome.as_str());
            req.extensions_mut().insert(cliente);
            next.run(req).await
        }
        Ok(None) => nao_autorizado("token inválido, expirado ou revogado"),
        Err(erro) => {
            tracing::warn!(%erro, "não consegui validar o token");
            erro_json(
                StatusCode::SERVICE_UNAVAILABLE,
                "não foi possível validar o token agora",
            )
        }
    }
}

/// Middleware: exige que o cliente (já autenticado por [`exigir_token`]) tenha ao menos um
/// dos escopos pedidos. `admin` passa em qualquer rota.
///
/// Os escopos chegam como `&'static [Escopo]`: uma lista fixa, escrita no código ao montar o
/// roteador, que vive o programa inteiro. Por isso o middleware não precisa copiá-la.
pub async fn exigir_escopo(escopos: &'static [Escopo], req: Request, next: Next) -> Response {
    let permitido = req.extensions().get::<Cliente>().is_some_and(|cliente| {
        cliente
            .escopos
            .iter()
            .any(|e| *e == Escopo::Admin || escopos.contains(e))
    });
    if permitido {
        next.run(req).await
    } else {
        erro_json(
            StatusCode::FORBIDDEN,
            "o token não tem permissão para esta rota",
        )
    }
}

fn extrair_bearer(req: &Request) -> Option<&str> {
    let valor = req.headers().get(header::AUTHORIZATION)?.to_str().ok()?;
    let token = valor.strip_prefix("Bearer ")?.trim();
    token.starts_with(PREFIXO).then_some(token)
}

fn hash_token(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

async fn buscar_cliente(db: &PgPool, hash: &[u8; 32]) -> Result<Option<Cliente>, sqlx::Error> {
    let linha = sqlx::query!(
        r#"
        SELECT nome, escopos
        FROM nexus.api_token
        WHERE hash = $1
          AND revogado_em IS NULL
          AND (expira_em IS NULL OR expira_em > now())
        "#,
        &hash[..],
    )
    .fetch_optional(db)
    .await?;

    Ok(linha.map(|l| Cliente {
        nome: l.nome,
        // A tabela tem CHECK nos valores, então o filter_map nunca descarta nada na prática.
        escopos: l.escopos.iter().filter_map(|e| e.parse().ok()).collect(),
    }))
}

fn nao_autorizado(mensagem: &'static str) -> Response {
    let mut resposta = erro_json(StatusCode::UNAUTHORIZED, mensagem);
    resposta.headers_mut().insert(
        header::WWW_AUTHENTICATE,
        header::HeaderValue::from_static("Bearer"),
    );
    resposta
}

#[derive(Serialize)]
struct CorpoErro {
    erro: &'static str,
}

fn erro_json(status: StatusCode, mensagem: &'static str) -> Response {
    (status, Json(CorpoErro { erro: mensagem })).into_response()
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn escopo_ida_e_volta() {
        for e in [
            Escopo::Leitura,
            Escopo::Escrita,
            Escopo::Relay,
            Escopo::Admin,
        ] {
            assert_eq!(e.como_str().parse::<Escopo>().unwrap(), e);
        }
        assert!("root".parse::<Escopo>().is_err());
    }

    #[test]
    fn hash_e_estavel_e_tem_32_bytes() {
        assert_eq!(hash_token("nxs_abc"), hash_token("nxs_abc"));
        assert_ne!(hash_token("nxs_abc"), hash_token("nxs_abd"));
    }

    fn requisicao_com(autorizacao: &str) -> Request {
        Request::builder()
            .header(header::AUTHORIZATION, autorizacao)
            .body(axum::body::Body::empty())
            .unwrap()
    }

    #[test]
    fn extrai_so_bearer_com_prefixo() {
        assert_eq!(
            extrair_bearer(&requisicao_com("Bearer nxs_123")),
            Some("nxs_123")
        );
        assert_eq!(extrair_bearer(&requisicao_com("Bearer outro_123")), None);
        assert_eq!(extrair_bearer(&requisicao_com("Basic nxs_123")), None);
    }
}
