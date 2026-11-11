//! Indicadores técnicos sobre os candles diários (já winsorizados).

use super::estat::Analise;
use crate::js;

#[derive(Debug, Clone, PartialEq)]
pub struct Bollinger {
    pub per: usize,
    pub meio: f64,
    pub sup: f64,
    pub inf: f64,
    pub pb: Option<f64>,
    pub largura: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Macd {
    pub linha: f64,
    pub sinal: f64,
    pub hist: f64,
    pub cruz: f64,
    pub adaptado: bool,
    pub per: [usize; 3],
}

#[derive(Debug, Clone, PartialEq)]
pub struct Indicadores {
    pub dias: usize,
    pub ultimo: f64,
    pub sma7: Option<f64>,
    pub sma14: Option<f64>,
    pub ema7: Option<f64>,
    pub ema14: Option<f64>,
    pub rsi: Option<f64>,
    pub rsi_per: usize,
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

fn soma(v: &[f64]) -> f64 {
    v.iter().fold(0.0, |s, x| s + x)
}

fn sma(v: &[f64], n: usize) -> Option<f64> {
    (n > 0 && v.len() >= n).then(|| soma(&v[v.len() - n..]) / n as f64)
}

/// Média móvel exponencial da série inteira (o primeiro ponto é a semente).
fn ema(v: &[f64], n: usize) -> Vec<f64> {
    if v.len() < 2 {
        return v.to_vec();
    }
    let k = 2.0 / (n as f64 + 1.0);
    v[1..].iter().fold(vec![v[0]], |mut o, x| {
        let anterior = o[o.len() - 1];
        o.push(x * k + anterior * (1.0 - k));
        o
    })
}

/// Desvio padrão amostral. `None` com menos de 2 pontos.
fn desvio(v: &[f64]) -> Option<f64> {
    if v.len() < 2 {
        return None;
    }
    let m = soma(v) / v.len() as f64;
    Some((v.iter().fold(0.0, |s, y| s + (y - m).powi(2)) / (v.len() - 1) as f64).sqrt())
}

/// Arredonda em `casas`; `None` para não finitos (`arred` do legado).
fn arred(x: f64, casas: i32) -> Option<f64> {
    x.is_finite().then(|| js::round_casas(x, casas))
}

/// RSI de Wilder, com período adaptado ao histórico (mínimo 3).
pub fn rsi(v: &[f64], n: usize) -> (Option<f64>, usize) {
    let per = n.min(v.len().saturating_sub(1));
    if v.len() < 4 || per < 3 {
        return (None, 0);
    }
    let pf = per as f64;
    let (g, l) = (1..=per).fold((0.0, 0.0), |(g, l), i| {
        let d = v[i] - v[i - 1];
        if d >= 0.0 {
            (g + d, l)
        } else {
            (g, l - d)
        }
    });
    let (g, l) = (per + 1..v.len()).fold((g / pf, l / pf), |(g, l), i| {
        let d = v[i] - v[i - 1];
        (
            (g * (pf - 1.0) + d.max(0.0)) / pf,
            (l * (pf - 1.0) + (-d).max(0.0)) / pf,
        )
    });
    let valor = if l == 0.0 {
        if g == 0.0 {
            50.0
        } else {
            100.0
        }
    } else {
        100.0 - 100.0 / (1.0 + g / l)
    };
    (Some(valor), per)
}

pub fn indicadores(a: &Analise) -> Indicadores {
    let m: Vec<f64> = a.candles.iter().map(|c| c.m).collect();
    let n = m.len();
    let ult = a.ultimo;
    // A série usa o último preço no lugar da mediana do último dia.
    let serie: Vec<f64> = if m.is_empty() {
        vec![]
    } else {
        m[..n - 1].iter().copied().chain([ult]).collect()
    };

    let sma7 = sma(&m, 7).or_else(|| if n >= 3 { sma(&m, n) } else { None });
    let e7 = ema(&m, 7);
    let e14 = ema(&m, 14);
    let (rsi_v, rsi_per) = rsi(&serie, 14);

    let bn = n.min(20);
    let boll = (bn >= 5).then(|| {
        let w = &serie[serie.len() - bn..];
        let mu = soma(w) / bn as f64;
        let sd = desvio(w)
            .filter(|d| *d != 0.0 && !d.is_nan())
            .unwrap_or(0.0);
        let up = mu + 2.0 * sd;
        let lo = 0f64.max(mu - 2.0 * sd);
        Bollinger {
            per: bn,
            meio: mu,
            sup: up,
            inf: lo,
            pb: if up > lo {
                arred((ult - lo) / (up - lo), 3)
            } else {
                Some(0.5)
            },
            largura: if mu != 0.0 && !mu.is_nan() {
                arred((up - lo) / mu, 3)
            } else {
                None
            },
        }
    });

    let macd = (n >= 10).then(|| {
        let f = 12.min(n / 3);
        let sl = 26.min(n * 2 / 3);
        let sg = 9.min(3.max(n / 4));
        let ef = ema(&serie, f);
        let es = ema(&serie, sl);
        let ml: Vec<f64> = ef.iter().zip(&es).map(|(a, b)| a - b).collect();
        let sgl = ema(&ml, sg);
        let l = ml.len() - 1;
        let anterior = ml[l - 1] - sgl[l - 1];
        Macd {
            linha: ml[l],
            sinal: sgl[l],
            hist: ml[l] - sgl[l],
            cruz: super::estat::sinal(ml[l] - sgl[l]) - super::estat::sinal(anterior),
            adaptado: !(f == 12 && sl == 26 && sg == 9),
            per: [f, sl, sg],
        }
    });

    let retornos: Vec<f64> = serie
        .windows(2)
        .filter(|w| w[1] > 0.0 && w[0] > 0.0)
        .map(|w| (w[1] / w[0]).ln())
        .collect();
    let vol = if retornos.len() >= 3 {
        desvio(&retornos).and_then(|d| arred(d, 4))
    } else {
        None
    };

    let k = 7.min(n as i64 - 1);
    let mom = if k >= 2 {
        let anterior = serie[serie.len() - 1 - k as usize];
        (anterior > 0.0)
            .then(|| arred(ult / anterior - 1.0, 4))
            .flatten()
    } else {
        None
    };

    let (sv, tv) = a.candles.iter().fold((0.0, 0.0), |(sv, tv), c| {
        (sv + c.m * c.v as f64, tv + c.v as f64)
    });
    let vwap = (tv != 0.0).then(|| sv / tv);

    let topo = (!m.is_empty()).then(|| js::maximo(m.iter().copied().chain([ult])));
    let drawdown = topo
        .filter(|t| *t != 0.0 && !t.is_nan())
        .and_then(|t| arred(ult / t - 1.0, 4));
    let sm = js::ordenados(m.iter().copied());

    Indicadores {
        dias: n,
        ultimo: ult,
        sma7,
        sma14: sma(&m, 14),
        ema7: (n >= 3).then(|| e7[e7.len() - 1]),
        ema14: (n >= 7).then(|| e14[e14.len() - 1]),
        rsi: rsi_v.and_then(|r| arred(r, 1)),
        rsi_per,
        boll,
        macd,
        vol,
        mom,
        mom_per: k,
        vwap,
        topo,
        drawdown,
        suporte: if n >= 5 {
            js::quantil(&sm, 0.1).unwrap_or(f64::NAN)
        } else {
            a.min30
        },
        resistencia: if n >= 5 {
            js::quantil(&sm, 0.9).unwrap_or(f64::NAN)
        } else {
            a.max30
        },
        min30: a.min30,
        max30: a.max30,
    }
}

#[cfg(test)]
mod testes {
    use proptest::prelude::*;

    use super::super::estat::{analisar, Venda};
    use super::*;

    const AGORA: i64 = 1_800_000_000_000;

    proptest! {
        /// Para qualquer série com preços positivos: RSI em [0, 100], Bollinger ordenado e
        /// nenhum indicador NaN ou infinito.
        #[test]
        fn propriedades(vendas in proptest::collection::vec((0i64..30 * 86_400_000, 1f64..1e7), 8..200)) {
            let vendas: Vec<Venda> = vendas.into_iter().map(|(atraso, unidade)| Venda { em: AGORA - atraso, unidade }).collect();
            let Some(a) = analisar(&vendas, AGORA) else { return Ok(()) };
            let i = indicadores(&a);
            if let Some(r) = i.rsi { prop_assert!((0.0..=100.0).contains(&r)); }
            if let Some(b) = &i.boll {
                prop_assert!(b.inf <= b.meio + 1e-9 && b.meio <= b.sup + 1e-9);
                prop_assert!(b.meio.is_finite() && b.sup.is_finite() && b.inf.is_finite());
            }
            for x in [i.sma7, i.sma14, i.ema7, i.ema14, i.vol, i.mom, i.vwap, i.topo, i.drawdown].into_iter().flatten() {
                prop_assert!(x.is_finite());
            }
            prop_assert!(i.suporte.is_finite() && i.resistencia.is_finite());
            prop_assert!(a.ultimo.is_finite() && a.ma7.is_finite() && a.ma30.is_finite());
            if let Some(m) = &i.macd { prop_assert!(m.linha.is_finite() && m.hist.is_finite()); }
        }

        #[test]
        fn rsi_nunca_sai_da_faixa(v in proptest::collection::vec(0.01f64..1e6, 0..60)) {
            if let (Some(r), _) = rsi(&v, 14) { prop_assert!((0.0..=100.0).contains(&r)); }
        }
    }

    #[test]
    fn serie_vazia_ou_curta_nao_vira_analise() {
        assert!(analisar(&[], AGORA).is_none());
        let um = [Venda {
            em: AGORA,
            unidade: 10.0,
        }];
        assert!(analisar(&um, AGORA).is_none());
    }
}
