//! Avaliação de ofertas abertas (`avaliarOfertas`): desconto em relação ao valor justo.

use std::collections::HashMap;

use serde_json::{Map, Value};

use super::estat::Historico;
use super::mercado::Mercado;
use super::repo::OfertaBruta;
use super::sinal::{fmt_bolsa, pct};
use super::ticker::{ChaveItem, Moeda, TickerId};
use crate::js;
use crate::precificador::ficha::Ficha;
use crate::precificador::vizinhos::{Confianca, ValorFicha};

#[derive(Debug, Clone, PartialEq)]
pub struct OfertaAvaliada {
    pub oferta_id: i64,
    pub tipo: Option<String>,
    pub moeda: Option<String>,
    pub sprite: Option<String>,
    pub nome: Option<String>,
    pub preco: f64,
    pub unidade: f64,
    pub quantidade: f64,
    /// Resumo da ficha (só os campos presentes na oferta).
    pub ficha: Option<Value>,
    pub justo: f64,
    pub faixa: Option<[f64; 2]>,
    pub confianca: Option<Confianca>,
    pub comparaveis: f64,
    pub fonte: &'static str,
    pub desconto: f64,
    pub motivo: String,
    pub tendencia: Option<f64>,
}

/// Valor justo pré-calculado de uma oferta de pokémon (a ficha guardada confere se a oferta
/// mudou desde o cálculo).
#[derive(Debug, Clone)]
pub struct ValorOferta {
    pub ficha: Option<Value>,
    pub valor: Option<ValorFicha>,
}

/// A oferta é de pokémon em orbs, sem refino: dá para precificar pela ficha.
fn precificavel(o: &OfertaBruta) -> Option<Ficha> {
    if o.tipo.as_deref() != Some("pokemon") || o.moeda.as_deref() != Some("orb") {
        return None;
    }
    let ficha = o.ficha.as_ref().map(Ficha::de_json).unwrap_or_default();
    (!ficha.refinada_qualquer).then(|| Ficha {
        nome: ficha
            .nome
            .clone()
            .filter(|n| !n.is_empty())
            .or_else(|| o.nome.clone()),
        ..ficha
    })
}

/// Calcula o valor justo de todas as ofertas de pokémon precificáveis. Roda em segundo plano,
/// junto com a montagem do mercado: as rotas só consultam o resultado.
pub fn precalcular(m: &Mercado, ofertas: &[OfertaBruta], agora: i64) -> HashMap<i64, ValorOferta> {
    if m.central.is_empty() {
        return HashMap::new();
    }
    let avaliador = m.avaliador();
    ofertas
        .iter()
        .filter_map(|o| {
            let ficha = precificavel(o)?;
            Some((
                o.id,
                ValorOferta {
                    ficha: o.ficha.clone(),
                    valor: avaliador.avaliar(&ficha, agora),
                },
            ))
        })
        .collect()
}

/// `avaliarOfertas`, ordenado do maior desconto para o menor.
pub fn avaliar(
    m: &Mercado,
    ofertas: &[OfertaBruta],
    valores: &HashMap<i64, ValorOferta>,
) -> Vec<OfertaAvaliada> {
    let mut saida: Vec<OfertaAvaliada> = ofertas
        .iter()
        .filter_map(|o| {
            let q = f64::from(o.quantidade.filter(|q| *q != 0).unwrap_or(1)).max(1.0);
            let preco = o.preco.unwrap_or(0) as f64;
            let unidade = preco / q;
            let pokemon = o.tipo.as_deref() == Some("pokemon");
            let item = if pokemon {
                avaliar_pokemon(o, unidade, valores)?
            } else {
                avaliar_item(m, o, unidade)?
            };
            let ficha_shiny = o
                .ficha
                .as_ref()
                .is_some_and(|f| matches!(f.get("shiny"), Some(Value::Bool(true))));
            Some(OfertaAvaliada {
                oferta_id: o.id,
                tipo: o.tipo.clone(),
                moeda: o.moeda.clone(),
                sprite: if pokemon {
                    m.sprite_especie(o.especie, ficha_shiny)
                } else {
                    m.icone_item(o.item_id)
                },
                nome: o.nome.clone().filter(|n| !n.is_empty()).or_else(|| {
                    o.item_id
                        .and_then(|i| m.catalogo.item(i).map(|c| c.nome.clone()))
                }),
                preco,
                unidade,
                quantidade: q,
                ficha: o.ficha.as_ref().map(resumo_ficha),
                ..item
            })
        })
        .collect();
    saida.sort_by(|a, b| a.desconto.total_cmp(&b.desconto));
    saida
}

fn resumo_ficha(f: &Value) -> Value {
    let campos = ["level", "quality", "potencia", "ivTotal", "shiny"];
    Value::Object(
        campos
            .iter()
            .filter_map(|k| f.get(*k).map(|v| ((*k).to_owned(), v.clone())))
            .collect::<Map<String, Value>>(),
    )
}

fn base_vazia() -> OfertaAvaliada {
    OfertaAvaliada {
        oferta_id: 0,
        tipo: None,
        moeda: None,
        sprite: None,
        nome: None,
        preco: 0.0,
        unidade: 0.0,
        quantidade: 1.0,
        ficha: None,
        justo: 0.0,
        faixa: None,
        confianca: None,
        comparaveis: 0.0,
        fonte: "",
        desconto: 0.0,
        motivo: String::new(),
        tendencia: None,
    }
}

fn avaliar_pokemon(
    o: &OfertaBruta,
    unidade: f64,
    valores: &HashMap<i64, ValorOferta>,
) -> Option<OfertaAvaliada> {
    precificavel(o)?;
    let v = valores.get(&o.id).filter(|v| v.ficha == o.ficha)?;
    let r = v.valor.as_ref()?;
    if r.faltam_adicionais {
        return None;
    }
    let e = &r.estimativa;
    Some(OfertaAvaliada {
        justo: e.median,
        faixa: Some([e.low, e.high]),
        confianca: Some(e.confianca.clone()),
        comparaveis: e.mercado_total as f64,
        fonte: "precificador",
        desconto: unidade / e.median - 1.0,
        motivo: format!(
            "Preço pedido {} vs valor justo {} (faixa {}–{}), calculado de {} vendas reais pelo PvE {}, nível {}; {} vendas da família {}. Confiança {}%.",
            fmt_bolsa(Some(unidade)),
            fmt_bolsa(Some(e.median)),
            fmt_bolsa(Some(e.low)),
            fmt_bolsa(Some(e.high)),
            e.mercado_total,
            js::numero(js::round(r.pve)),
            js::numero(r.nivel),
            e.familia_total,
            r.familia_nome,
            js::numero(e.confianca.percent),
        ),
        ..base_vazia()
    })
}

fn avaliar_item(m: &Mercado, o: &OfertaBruta, unidade: f64) -> Option<OfertaAvaliada> {
    let moeda: Moeda = o.moeda.as_deref()?.parse().ok()?;
    let por_id = o.item_id.and_then(|i| {
        m.ticker(&TickerId::Item {
            item: ChaveItem::Id(i),
            moeda,
        })
    });
    let nome = o.nome.as_deref().unwrap_or("").to_lowercase();
    let t = por_id.or_else(|| {
        m.tickers
            .iter()
            .find(|x| !x.pokemon && x.moeda == moeda && x.nome.to_lowercase() == nome)
    })?;
    if t.a.historico != Historico::Ok {
        return None;
    }
    Some(OfertaAvaliada {
        justo: t.a.ma7,
        fonte: "mediana 7d do item",
        desconto: unidade / t.a.ma7 - 1.0,
        confianca: None,
        comparaveis: t.a.vol7d as f64,
        motivo: format!(
            "Unidade a {} vs mediana 7d {}; tendência do item {}/dia.",
            fmt_bolsa(Some(unidade)),
            fmt_bolsa(Some(t.a.ma7)),
            pct(t.a.tendencia)
        ),
        tendencia: Some(t.a.tendencia),
        ..base_vazia()
    })
}
