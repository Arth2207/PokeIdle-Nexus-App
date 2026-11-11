//! Contrato JSON das rotas `/api/bolsa/*`. Os nomes dos campos seguem o legado em Node.
//!
//! Convenções:
//! - `Option<T>` sem anotação vira `null` no JSON, como os `?? null` do legado;
//! - `skip_serializing_if = "Option::is_none"` marca campos que o legado deixa `undefined`
//!   (o `JSON.stringify` os omite).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Parâmetros de consulta. Todos opcionais e em texto: o legado aceita qualquer coisa e
/// converte como o JS (`+valor`), e o servidor faz o mesmo.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Params {
    pub id: Option<String>,
    pub tipo: Option<String>,
    pub moeda: Option<String>,
    pub acao: Option<String>,
    pub q: Option<String>,
    pub limite: Option<String>,
    pub pagina: Option<String>,
}

/// Parâmetros de `GET /api/bolsa/serie` (mantido por compatibilidade com o primeiro corte).
pub type SerieParams = Params;

/// `GET /api/bolsa/serie`: `{ id, vendas: [[timestamp_ms, preco_unitario]] }`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SerieResposta {
    pub id: String,
    pub vendas: Vec<(i64, f64)>,
}

/// Resumo de um ticker (`resumoT` do legado), usado nas listas.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResumoTicker {
    pub id: String,
    pub tipo: String,
    pub moeda: String,
    pub nome: String,
    pub species_id: Option<i32>,
    pub item_id: Option<i32>,
    pub shiny: bool,
    pub ultimo: f64,
    pub ma7: f64,
    pub ma30: f64,
    pub var24h: Option<f64>,
    pub var7d: Option<f64>,
    pub tendencia: f64,
    pub desconto: Option<f64>,
    pub vol24h: u64,
    pub vol30d: u64,
    pub liquidez: String,
    pub base: Option<f64>,
    pub z: Option<f64>,
    pub tend_z: f64,
    pub historico: String,
    pub dias: u64,
    pub confianca: f64,
    pub spd: f64,
    pub max30_bruto: f64,
    pub acao: String,
    pub vscore: f64,
    pub vconf: f64,
    pub vprov: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vmot: Option<String>,
    pub alvo_v: Option<f64>,
    pub alvo_c: Option<f64>,
    pub ultima_venda: i64,
    pub sinal: String,
    pub score: f64,
    pub motivo: String,
    pub spark: Vec<f64>,
    pub sprite: Option<String>,
    pub ask_min: Option<f64>,
    pub ask_anuncios: Option<i64>,
    pub normalizado: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResumoSinais {
    pub comprar: u64,
    pub vender: u64,
    pub observar: u64,
    pub manter: u64,
    pub sem_base: u64,
    pub vendas: u64,
}

/// `GET /api/bolsa/mercado`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MercadoResposta {
    pub gerado_em: i64,
    pub janela_dias: i64,
    pub total: u64,
    pub resumo: ResumoSinais,
    pub tickers: Vec<ResumoTicker>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bollinger {
    pub per: u64,
    pub meio: f64,
    pub sup: f64,
    pub inf: f64,
    pub pb: Option<f64>,
    pub largura: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Macd {
    pub linha: f64,
    pub sinal: f64,
    pub hist: f64,
    pub cruz: f64,
    pub adaptado: bool,
    pub per: [u64; 3],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Indicadores {
    pub dias: u64,
    pub ultimo: f64,
    pub sma7: Option<f64>,
    pub sma14: Option<f64>,
    pub ema7: Option<f64>,
    pub ema14: Option<f64>,
    pub rsi: Option<f64>,
    pub rsi_per: u64,
    pub boll: Option<Bollinger>,
    pub macd: Option<Macd>,
    pub vol: Option<f64>,
    pub mom: Option<f64>,
    pub mom_per: i64,
    pub vwap: Option<f64>,
    pub topo: Option<f64>,
    pub drawdown: Option<f64>,
    pub suporte: f64,
    pub resistencia: f64,
    pub min30: f64,
    pub max30: f64,
}

/// Veredito (os nomes com `_` vêm assim do legado).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Veredito {
    pub acao: String,
    pub score: f64,
    pub confianca: f64,
    pub provisorio: bool,
    pub preco_alvo_venda: Option<f64>,
    pub preco_alvo_compra: Option<f64>,
    pub stop: Option<f64>,
    pub motivos: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EstatOfertas {
    pub n: i64,
    pub min: f64,
    pub p25: f64,
    pub med: f64,
    pub p75: f64,
    pub max: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hist {
    pub n: i64,
    pub min: f64,
    pub max: f64,
    pub p05: f64,
    pub p95: f64,
    pub desde: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Referencia {
    pub nivel: f64,
    pub pve: f64,
    pub nota: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Candle {
    pub d: String,
    pub o: f64,
    pub h: f64,
    pub l: f64,
    pub c: f64,
    pub m: f64,
    pub v: u64,
    pub hb: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VendaRecente {
    pub at: i64,
    pub preco: f64,
    pub level: Option<f64>,
    pub pve: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OfertaTicker {
    pub id: i64,
    pub preco: f64,
    pub quantidade: Option<i32>,
    pub nome: Option<String>,
}

/// `GET /api/bolsa/ticker`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TickerResposta {
    #[serde(flatten)]
    pub resumo: ResumoTicker,
    pub ind: Indicadores,
    pub vd: Veredito,
    pub of_stat: Option<EstatOfertas>,
    pub hist: Option<Hist>,
    pub min30: f64,
    pub max30: f64,
    pub vol7d: u64,
    pub referencia: Option<Referencia>,
    pub candles: Vec<Candle>,
    pub recentes: Vec<VendaRecente>,
    pub ofertas: Vec<OfertaTicker>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MacdResumo {
    pub hist: f64,
    pub adaptado: bool,
}

/// `GET /api/bolsa/balao`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BalaoResposta {
    pub id: String,
    pub nome: String,
    pub tipo: String,
    pub moeda: String,
    pub sprite: Option<String>,
    pub ultimo: f64,
    pub ma7: f64,
    pub ma30: f64,
    pub min30: f64,
    pub max30: f64,
    pub hist: Option<Hist>,
    pub var24h: Option<f64>,
    pub var7d: Option<f64>,
    pub tendencia: f64,
    pub tend_z: f64,
    pub liquidez: String,
    pub vol24h: u64,
    pub spd: f64,
    pub dias: u64,
    pub historico: String,
    pub normalizado: bool,
    pub ask_min: Option<f64>,
    pub of_stat: Option<EstatOfertas>,
    pub rsi: Option<f64>,
    pub pb: Option<f64>,
    pub largura: Option<f64>,
    pub macd: Option<MacdResumo>,
    pub vol_dia: Option<f64>,
    pub mom: Option<f64>,
    pub mom_per: i64,
    pub vwap: Option<f64>,
    pub drawdown: Option<f64>,
    pub suporte: f64,
    pub resistencia: f64,
    pub vd: Veredito,
}

/// `GET /api/bolsa/feedback`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedbackResposta {
    pub gerado_em: i64,
    pub total: u64,
    pub contagem: BTreeMap<String, u64>,
    pub pagina: f64,
    pub limite: f64,
    pub itens: Vec<ResumoTicker>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Confianca {
    pub chance: f64,
    pub percent: f64,
    pub lowest: String,
}

/// Uma oferta avaliada contra o valor justo.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OfertaAvaliada {
    pub oferta_id: i64,
    pub tipo: Option<String>,
    pub moeda: Option<String>,
    pub sprite: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nome: Option<String>,
    pub preco: f64,
    pub unidade: f64,
    pub quantidade: f64,
    pub ficha: Option<Value>,
    pub justo: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub faixa: Option<[f64; 2]>,
    pub confianca: Option<Confianca>,
    pub comparaveis: f64,
    pub fonte: String,
    pub desconto: f64,
    pub motivo: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tendencia: Option<f64>,
}

/// `GET /api/bolsa/dicas`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DicasResposta {
    pub gerado_em: i64,
    pub ofertas_ativas: u64,
    pub aviso: Option<String>,
    pub comprar: Vec<ResumoTicker>,
    pub vender: Vec<ResumoTicker>,
    pub oportunidades: Vec<OfertaAvaliada>,
}

/// `GET /api/bolsa/ofertas`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OfertasResposta {
    pub gerado_em: i64,
    pub ultima_captura: Option<i64>,
    pub total: u64,
    pub avaliadas: u64,
    pub ofertas: Vec<OfertaAvaliada>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeuItem {
    pub item_id: i32,
    pub nome: String,
    pub quantidade: i64,
    pub sprite: Option<String>,
    pub id: Option<String>,
    pub ultimo: Option<f64>,
    pub valor: Option<f64>,
    pub acao: String,
    pub vscore: f64,
    pub vconf: f64,
    pub vprov: bool,
    pub alvo_v: Option<f64>,
    pub valor_alvo: Option<f64>,
    pub vmot: String,
}

/// `GET /api/bolsa/meus-itens`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MeusItensResposta {
    pub moeda: String,
    pub ts: Option<i64>,
    pub total: f64,
    pub itens: Vec<MeuItem>,
    pub aviso: Option<String>,
}

/// `POST /api/bolsa/ofertas`. Os itens chegam como JSON livre: o relay manda números em
/// texto às vezes, e a validação segue as regras do legado.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OfertasEntrada {
    #[serde(default)]
    pub ofertas: Value,
    #[serde(default)]
    pub completo: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OfertasGravadas {
    pub ok: bool,
    pub recebidas: u64,
    pub validas: u64,
    pub gravadas: u64,
}

/// `POST /api/bolsa/resumo-itens`: `{ resumo: { [itemId]: { minOrb, minGold, anuncios } } }`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ResumoEntrada {
    #[serde(default)]
    pub resumo: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResumoGravado {
    pub ok: bool,
    pub itens: u64,
    pub gravadas: u64,
}

/// `POST /api/bolsa/meus-itens`: `{ itens, limpar, somar }`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MeusItensEntrada {
    #[serde(default)]
    pub itens: Value,
    #[serde(default)]
    pub limpar: Value,
    #[serde(default)]
    pub somar: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MeusItensGravados {
    pub ok: bool,
    pub itens: u64,
}
