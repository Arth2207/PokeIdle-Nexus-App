//! Portas de saída da bolsa (arquitetura hexagonal).
//!
//! O domínio diz *o que* precisa ("me dê as vendas entre A e B"); quem implementa decide
//! *como* (Postgres em produção, um `Vec` em memória nos testes). Em JS seria como receber
//! um objeto `{ vendas(desde, ate) }` por injeção de dependência, só que aqui o compilador
//! garante que a implementação tem todos os métodos com os tipos certos.

use std::future::Future;

use super::catalogo::ItemCatalogo;
use super::venda::VendaBruta;

/// Erro de qualquer repositório. O domínio não sabe que existe sqlx, então o erro de origem
/// vem embrulhado numa `Box<dyn Error>`: "algum erro, de algum tipo".
#[derive(Debug, thiserror::Error)]
#[error("falha ao ler dados do mercado: {0}")]
pub struct ErroRepo(#[source] pub Box<dyn std::error::Error + Send + Sync>);

/// `Send + Sync` no trait: a implementação pode ser usada por várias tasks do tokio ao
/// mesmo tempo. Os métodos devolvem `impl Future + Send` em vez de `async fn` para deixar
/// explícito que o future pode trocar de thread, o que o axum exige.
pub trait RepoMercado: Send + Sync {
    /// Vendas com `desde_ms <= em <= ate_ms`, em ordem de `em`.
    fn vendas(
        &self,
        desde_ms: i64,
        ate_ms: i64,
    ) -> impl Future<Output = Result<Vec<VendaBruta>, ErroRepo>> + Send;

    fn catalogo_itens(&self) -> impl Future<Output = Result<Vec<ItemCatalogo>, ErroRepo>> + Send;
}
