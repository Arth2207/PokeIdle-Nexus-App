//! Conversão dos tipos do domínio (`nucleo`) para o contrato JSON (`contrato`).
//!
//! Por que funções e não `impl From<&A> for B`? A "orphan rule" do Rust só deixa implementar
//! um trait para um tipo se o trait ou o tipo for do crate atual. Aqui `From` é da std, `A`
//! é do `nucleo` e `B` do `contrato`: nenhum é deste crate, então o compilador recusa. A regra
//! existe para que dois crates nunca deem implementações conflitantes para o mesmo par.

use contrato::bolsa as dto;
use nucleo::bolsa::indicadores::Indicadores;
use nucleo::bolsa::mercado::{Hist, Ticker};
use nucleo::bolsa::ofertas::OfertaAvaliada;
use nucleo::bolsa::sinal::{EstatOfertas, Veredito};
use nucleo::precificador::motor::Referencia;

pub fn resumo(t: &Ticker) -> dto::ResumoTicker {
    let a = &t.a;
    dto::ResumoTicker {
        id: t.texto.clone(),
        tipo: if t.pokemon { "pokemon" } else { "item" }.into(),
        moeda: t.moeda.como_str().into(),
        nome: t.nome.clone(),
        species_id: t.especie,
        item_id: t.item_id,
        shiny: t.shiny,
        ultimo: a.ultimo,
        ma7: a.ma7,
        ma30: a.ma30,
        var24h: a.var24h,
        var7d: a.var7d,
        tendencia: a.tendencia,
        desconto: a.desconto,
        vol24h: a.vol24h as u64,
        vol30d: a.vol30d as u64,
        liquidez: a.liquidez.como_str().into(),
        base: a.base,
        z: a.z,
        tend_z: a.tend_z,
        historico: a.historico.como_str().into(),
        dias: a.dias as u64,
        confianca: a.confianca,
        spd: a.spd,
        max30_bruto: a.max30_bruto,
        acao: t.vd.acao.como_str().into(),
        vscore: t.vd.score,
        vconf: t.vd.confianca,
        vprov: t.vd.provisorio,
        vmot: t
            .vd
            .motivos
            .iter()
            .find(|m| !m.starts_with("Veredito PROVISÓRIO"))
            .cloned(),
        alvo_v: t.vd.preco_alvo_venda,
        alvo_c: t.vd.preco_alvo_compra,
        ultima_venda: a.ultima_venda,
        sinal: t.sinal.sinal.como_str().into(),
        score: t.sinal.score,
        motivo: t.sinal.motivo.clone(),
        spark: a.spark.clone(),
        sprite: t.sprite.clone(),
        ask_min: t.ask_min,
        ask_anuncios: t.ask_anuncios,
        normalizado: t.normalizado,
    }
}

pub fn veredito(v: &Veredito) -> dto::Veredito {
    dto::Veredito {
        acao: v.acao.como_str().into(),
        score: v.score,
        confianca: v.confianca,
        provisorio: v.provisorio,
        preco_alvo_venda: v.preco_alvo_venda,
        preco_alvo_compra: v.preco_alvo_compra,
        stop: v.stop,
        motivos: v.motivos.clone(),
    }
}
pub fn estat_ofertas(o: &EstatOfertas) -> dto::EstatOfertas {
    dto::EstatOfertas {
        n: o.n,
        min: o.min,
        p25: o.p25,
        med: o.med,
        p75: o.p75,
        max: o.max,
    }
}
pub fn hist(h: &Hist) -> dto::Hist {
    dto::Hist {
        n: h.n,
        min: h.min,
        max: h.max,
        p05: h.p05,
        p95: h.p95,
        desde: h.desde,
    }
}
pub fn referencia(r: &Referencia) -> dto::Referencia {
    dto::Referencia {
        nivel: r.nivel,
        pve: r.pve,
        nota: r.nota,
    }
}
pub fn indicadores(i: &Indicadores) -> dto::Indicadores {
    dto::Indicadores {
        dias: i.dias as u64,
        ultimo: i.ultimo,
        sma7: i.sma7,
        sma14: i.sma14,
        ema7: i.ema7,
        ema14: i.ema14,
        rsi: i.rsi,
        rsi_per: i.rsi_per as u64,
        boll: i.boll.as_ref().map(|b| dto::Bollinger {
            per: b.per as u64,
            meio: b.meio,
            sup: b.sup,
            inf: b.inf,
            pb: b.pb,
            largura: b.largura,
        }),
        macd: i.macd.as_ref().map(|m| dto::Macd {
            linha: m.linha,
            sinal: m.sinal,
            hist: m.hist,
            cruz: m.cruz,
            adaptado: m.adaptado,
            per: m.per.map(|p| p as u64),
        }),
        vol: i.vol,
        mom: i.mom,
        mom_per: i.mom_per,
        vwap: i.vwap,
        topo: i.topo,
        drawdown: i.drawdown,
        suporte: i.suporte,
        resistencia: i.resistencia,
        min30: i.min30,
        max30: i.max30,
    }
}
pub fn oferta(o: &OfertaAvaliada) -> dto::OfertaAvaliada {
    dto::OfertaAvaliada {
        oferta_id: o.oferta_id,
        tipo: o.tipo.clone(),
        moeda: o.moeda.clone(),
        sprite: o.sprite.clone(),
        nome: o.nome.clone(),
        preco: o.preco,
        unidade: o.unidade,
        quantidade: o.quantidade,
        ficha: o.ficha.clone(),
        justo: o.justo,
        faixa: o.faixa,
        confianca: o.confianca.as_ref().map(|c| dto::Confianca {
            chance: c.chance,
            percent: c.percent,
            lowest: c.lowest.into(),
        }),
        comparaveis: o.comparaveis,
        fonte: o.fonte.into(),
        desconto: o.desconto,
        motivo: o.motivo.clone(),
        tendencia: o.tendencia,
    }
}
