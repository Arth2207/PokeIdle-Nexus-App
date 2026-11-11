//! Portas de saída da bolsa (arquitetura hexagonal).
//!
//! O domínio diz *o que* precisa ("me dê as vendas entre A e B"); quem implementa decide
//! *como* (Postgres em produção, um `Vec` em memória nos testes). Em JS seria como receber
//! um objeto `{ vendas(desde, ate) }` por injeção de dependência, só que aqui o compilador
//! garante que a implementação tem todos os métodos com os tipos certos.

use std::future::Future;

use serde_json::Value;

use super::catalogo::ItemCatalogo;
use super::venda::VendaBruta;
use crate::precificador::cotacao::PrecoItemDia;
use crate::precificador::especie::Especie;
use crate::precificador::motor::LinhaMotor;

/// Erro de qualquer repositório. O domínio não sabe que existe sqlx, então o erro de origem
/// vem embrulhado numa `Box<dyn Error>`: "algum erro, de algum tipo".
#[derive(Debug, thiserror::Error)]
#[error("falha ao acessar os dados da bolsa: {0}")]
pub struct ErroRepo(#[source] pub Box<dyn std::error::Error + Send + Sync>);

/// Dados que mudam pouco: catálogos, base antiga do precificador, parâmetros.
#[derive(Debug, Clone, Default)]
pub struct Estaticos {
    /// Na ordem da fonte.
    pub especies: Vec<Especie>,
    pub base: Vec<LinhaMotor>,
    pub itens_base: Vec<PrecoItemDia>,
    /// Na ordem da fonte.
    pub catalogo: Vec<ItemCatalogo>,
    pub nomes_dim_item: Vec<(i32, String)>,
    pub pesos_confianca: Option<[f64; 5]>,
    pub vies: Option<f64>,
}

/// Menor anúncio recente de um item (`fato_resumo_item`).
#[derive(Debug, Clone, PartialEq)]
pub struct Ask {
    pub item_id: i32,
    pub moeda: String,
    pub anuncios: Option<i64>,
    pub preco_min: f64,
}

/// Estatística agregada (ofertas abertas ou histórico de vendas) de um grupo.
#[derive(Debug, Clone, PartialEq)]
pub struct Agregado {
    pub n: i64,
    pub min: f64,
    pub max: f64,
    pub q: Vec<f64>,
    /// Só no histórico: primeira venda.
    pub desde: Option<i64>,
}

/// Grupo de um agregado de item: id (ou nome normalizado) e moeda.
#[derive(Debug, Clone, PartialEq)]
pub struct AgregadoItem {
    /// No histórico, o id do item em texto ou o nome normalizado; nas ofertas, o id.
    pub chave: String,
    pub moeda: String,
    pub agregado: Agregado,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AgregadoPokemon {
    pub especie: i32,
    pub shiny: bool,
    pub moeda: String,
    pub agregado: Agregado,
}

/// Agregados que o legado recalcula a cada montagem.
#[derive(Debug, Clone, Default)]
pub struct Agregados {
    pub asks: Vec<Ask>,
    pub ofertas_itens: Vec<AgregadoItem>,
    pub ofertas_pokemon: Vec<AgregadoPokemon>,
    pub hist_itens: Vec<AgregadoItem>,
    pub hist_pokemon: Vec<AgregadoPokemon>,
}

/// Uma linha de `fato_oferta_mercado`.
#[derive(Debug, Clone, PartialEq)]
pub struct OfertaBruta {
    pub id: i64,
    pub ts_visto: i64,
    pub tipo: Option<String>,
    pub moeda: Option<String>,
    pub preco: Option<i64>,
    pub especie: Option<i32>,
    pub item_id: Option<i32>,
    pub quantidade: Option<i32>,
    pub nome: Option<String>,
    pub ficha: Option<Value>,
}

/// Filtro de ofertas ativas.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FiltroOfertas {
    pub corte_item: i64,
    pub corte_pokemon: i64,
    pub tipo: Option<String>,
    pub moeda: Option<String>,
    pub limite: i64,
}

/// `Send + Sync` no trait: a implementação pode ser usada por várias tasks do tokio ao
/// mesmo tempo. Os métodos devolvem `impl Future + Send` em vez de `async fn` para deixar
/// explícito que o future pode trocar de thread, o que o axum exige.
pub trait RepoMercado: Send + Sync {
    /// Vendas com `desde_ms <= em <= ate_ms`, em ordem de `em` e `id`.
    fn vendas(
        &self,
        desde_ms: i64,
        ate_ms: i64,
    ) -> impl Future<Output = Result<Vec<VendaBruta>, ErroRepo>> + Send;

    /// Vendas com `em >= em_desde` (carga incremental, usa o índice de `em`).
    fn vendas_recentes(
        &self,
        em_desde: i64,
    ) -> impl Future<Output = Result<Vec<VendaBruta>, ErroRepo>> + Send;

    fn estaticos(&self) -> impl Future<Output = Result<Estaticos, ErroRepo>> + Send;

    /// Agregados com os cortes de frescor dados (ms). O histórico (que varre a tabela de
    /// vendas inteira) só vem com `com_historico`.
    fn agregados(
        &self,
        corte_item: i64,
        corte_pokemon: i64,
        corte_resumo: i64,
        com_historico: bool,
    ) -> impl Future<Output = Result<Agregados, ErroRepo>> + Send;

    /// Ofertas ativas, da mais recente para a mais antiga.
    fn ofertas_ativas(
        &self,
        filtro: &FiltroOfertas,
    ) -> impl Future<Output = Result<Vec<OfertaBruta>, ErroRepo>> + Send;

    /// Até 200 ofertas ativas de um ticker, da mais barata para a mais cara.
    fn ofertas_do_ticker(
        &self,
        pokemon: bool,
        id: i32,
        moeda: &str,
        corte: i64,
    ) -> impl Future<Output = Result<Vec<OfertaBruta>, ErroRepo>> + Send;
}
