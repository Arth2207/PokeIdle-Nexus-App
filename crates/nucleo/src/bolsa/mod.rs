//! Bolsa: vendas do mercado do jogo viram tickers com indicadores e sinais.

pub mod catalogo;
pub mod mercado;
pub mod repo;
pub mod ticker;
pub mod venda;

/// Um dia em milissegundos. Os timestamps do legado são `bigint` em ms.
pub const DIA_MS: i64 = 86_400_000;
/// Janela de análise: só vendas dos últimos 30 dias entram nos tickers.
pub const JANELA_DIAS: i64 = 30;
/// Mínimo de vendas válidas na janela para um ticker existir.
pub const MIN_VENDAS: usize = 6;
/// Máximo de pontos devolvidos por `/api/bolsa/serie` (os mais recentes).
pub const SERIE_MAX_PONTOS: usize = 5000;
