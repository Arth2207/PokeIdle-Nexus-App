//! Contrato JSON das rotas `/api/bolsa/*`. Os nomes dos campos seguem o legado em Node.

use serde::{Deserialize, Serialize};

/// Parâmetros de `GET /api/bolsa/serie`.
///
/// `id` é opcional de propósito: o legado responde 404 ("ticker não encontrado") quando
/// ele falta, e não 400. Mantemos o mesmo comportamento.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SerieParams {
    pub id: Option<String>,
}

/// Resposta de `GET /api/bolsa/serie`: `{ id, vendas: [[timestamp_ms, preco_unitario]] }`.
///
/// O serde serializa uma tupla `(i64, f64)` como array de dois elementos, igual ao JS.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SerieResposta {
    pub id: String,
    pub vendas: Vec<(i64, f64)>,
}
