//! Núcleo estatístico da bolsa (`bolsa-stats.js`): análise robusta de uma série de vendas.
//!
//! Trabalha em log-preço, com estimadores robustos (mediana, quantis, winsorização pela
//! metade inferior da distribuição) e tendência por Theil–Sen + Mann–Kendall.

use crate::js;

pub const JANELA_DIAS: i64 = 30;
pub const MIN_VENDAS: usize = 8;
pub const MIN_DIAS_OK: usize = 5;
pub const MIN_ANTES: usize = 5;
pub const MIN_SPAN_BASE: f64 = 2.0;
pub const EDGE: f64 = 0.12;
pub const Z: f64 = 2.0;
pub const Z_TEND: f64 = 1.28;
pub const Z_MOM: f64 = 1.645;
pub const TEND_MIN: f64 = 0.02;
pub const MIN_ESCALA: f64 = 0.05;
pub const LIQ_ALTA: f64 = 6.0;
pub const LIQ_MEDIA: f64 = 2.0;
pub const MIN_V24: usize = 3;

const DIA: i64 = 86_400_000;
const DIA_F: f64 = 86_400_000.0;

/// Candle diário (preços winsorizados; `hb` é o maior preço bruto do dia).
#[derive(Debug, Clone, PartialEq)]
pub struct Candle {
    pub d: String,
    pub dia: i64,
    pub o: f64,
    pub h: f64,
    pub l: f64,
    pub c: f64,
    pub m: f64,
    pub v: usize,
    pub hb: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Liquidez {
    Alta,
    Media,
    Baixa,
}

impl Liquidez {
    pub fn como_str(self) -> &'static str {
        match self {
            Liquidez::Alta => "alta",
            Liquidez::Media => "média",
            Liquidez::Baixa => "baixa",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Historico {
    Ok,
    Curto,
    Insuficiente,
}

impl Historico {
    pub fn como_str(self) -> &'static str {
        match self {
            Historico::Ok => "ok",
            Historico::Curto => "curto",
            Historico::Insuficiente => "insuficiente",
        }
    }
}

/// Resultado de `analisar`.
#[derive(Debug, Clone, PartialEq)]
pub struct Analise {
    pub n: usize,
    pub dias: usize,
    pub ultimo: f64,
    pub ma7: f64,
    pub ma30: f64,
    pub base: Option<f64>,
    pub desconto: Option<f64>,
    pub z: Option<f64>,
    pub var24h: Option<f64>,
    pub var7d: Option<f64>,
    pub tendencia: f64,
    pub tend_z: f64,
    pub tend_ok: bool,
    pub vol24h: usize,
    pub vol7d: usize,
    pub vol30d: usize,
    pub spd: f64,
    pub liquidez: Liquidez,
    pub historico: Historico,
    pub confianca: f64,
    pub escala: f64,
    pub outliers: f64,
    pub min30: f64,
    pub max30: f64,
    pub max30_bruto: f64,
    pub candles: Vec<Candle>,
    pub spark: Vec<f64>,
    pub ultima_venda: i64,
}

struct Winsor {
    lw: Vec<f64>,
    s: f64,
    hi: f64,
    lo: f64,
    fora: f64,
}

/// Winsoriza em log: escala pela metade inferior; teto +3σ (mín. ×3), piso −4σ (mín. ÷5).
fn winsor(l: &[f64]) -> Winsor {
    let s0 = js::ordenados(l.iter().copied());
    let mu = js::quantil(&s0, 0.5).unwrap_or(f64::NAN);
    let q25 = js::quantil(&s0, 0.25).unwrap_or(f64::NAN);
    let s = MIN_ESCALA.max((mu - q25) / 0.6745);
    let hi = mu + (3.0 * s).max(js::ln(3.0));
    let lo = mu - (4.0 * s).max(js::ln(5.0));
    let lw: Vec<f64> = l.iter().map(|&x| x.min(hi).max(lo)).collect();
    let n = l.iter().filter(|&&x| x > hi || x < lo).count();
    Winsor {
        lw,
        s,
        hi,
        lo,
        fora: n as f64 / l.len() as f64,
    }
}

/// Theil–Sen (inclinação mediana) + Mann–Kendall. `pontos`: (dia, log preço).
pub fn tendencia_robusta(pontos: &[(f64, f64)]) -> (f64, f64, bool) {
    let n = pontos.len();
    if n < MIN_DIAS_OK {
        return (0.0, 0.0, false);
    }
    let pares = (0..n).flat_map(|i| (i + 1..n).map(move |j| (i, j)));
    let (inclinacoes, s) = pares.fold((Vec::new(), 0.0), |(mut sl, s), (i, j)| {
        let dt = pontos[j].0 - pontos[i].0;
        if dt <= 0.0 {
            return (sl, s);
        }
        let dy = pontos[j].1 - pontos[i].1;
        sl.push(dy / dt);
        (sl, s + sinal(dy))
    });
    let nf = n as f64;
    let v = nf * (nf - 1.0) * (2.0 * nf + 5.0) / 18.0;
    let z = if s > 0.0 {
        (s - 1.0) / v.sqrt()
    } else if s < 0.0 {
        (s + 1.0) / v.sqrt()
    } else {
        0.0
    };
    // `med([])` é null no JS, e `Math.exp(null) - 1` vale 0.
    (js::mediana(inclinacoes).unwrap_or(0.0), z, true)
}

/// `Math.sign`.
pub fn sinal(x: f64) -> f64 {
    if x > 0.0 {
        1.0
    } else if x < 0.0 {
        -1.0
    } else {
        x
    }
}

fn med(xs: impl IntoIterator<Item = f64>) -> f64 {
    js::mediana(xs).unwrap_or(f64::NAN)
}

/// Um ponto bruto da série.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Venda {
    pub em: i64,
    pub unidade: f64,
}

struct Item {
    em: i64,
    bruto: f64,
    p: f64,
}

/// Analisa a série de vendas de um ativo no instante `agora`. `None` sem amostra mínima.
pub fn analisar(vendas: &[Venda], agora: i64) -> Option<Analise> {
    let inicio = agora - JANELA_DIAS * DIA;
    let mut w: Vec<Venda> = vendas
        .iter()
        .copied()
        .filter(|v| v.em <= agora && v.em > inicio && v.unidade > 0.0)
        .collect();
    w.sort_by_key(|v| v.em);
    if w.len() < MIN_VENDAS {
        return None;
    }
    let wz = winsor(&w.iter().map(|v| js::ln(v.unidade)).collect::<Vec<_>>());
    let it: Vec<Item> = w
        .iter()
        .zip(&wz.lw)
        .map(|(v, l)| Item {
            em: v.em,
            bruto: v.unidade,
            p: js::exp(*l),
        })
        .collect();
    let primeiro = it.first()?;
    let ultimo_item = it.last()?;
    let span_dias = (1.0 / 24.0f64).max((agora - primeiro.em) as f64 / DIA_F);

    // Candles por dia (UTC−3), em ordem de data.
    let mut por_dia: Vec<(i64, Vec<&Item>)> = Vec::new();
    it.iter().for_each(|x| {
        let d = js::dia_sp(x.em);
        match por_dia.iter_mut().find(|(k, _)| *k == d) {
            Some((_, l)) => l.push(x),
            None => por_dia.push((d, vec![x])),
        }
    });
    por_dia.sort_by_key(|(d, _)| *d);
    let candles: Vec<Candle> = por_dia
        .iter()
        .map(|(d, l)| {
            let p: Vec<f64> = l.iter().map(|x| x.p).collect();
            Candle {
                d: js::data_do_dia(*d),
                dia: *d,
                o: p[0],
                h: js::maximo(p.iter().copied()),
                l: js::minimo(p.iter().copied()),
                c: p[p.len() - 1],
                m: med(p.iter().copied()),
                v: l.len(),
                hb: js::maximo(l.iter().map(|x| x.bruto)),
            }
        })
        .collect();

    let filtro = |f: &dyn Fn(i64) -> bool| -> Vec<f64> {
        it.iter().filter(|x| f(x.em)).map(|x| x.p).collect()
    };
    let r24 = filtro(&|t| t > agora - DIA);
    let p24 = filtro(&|t| t <= agora - DIA && t > agora - 4 * DIA);
    let w7 = filtro(&|t| t > agora - 7 * DIA);
    let antes: Vec<&Item> = it.iter().filter(|x| x.em <= agora - DIA).collect();
    let antes7 = filtro(&|t| t <= agora - 7 * DIA);
    let pool3 = filtro(&|t| t > agora - 3 * DIA);
    let k24 = r24.len();

    let ultimo = if k24 >= MIN_V24 {
        med(r24.iter().copied())
    } else if pool3.len() >= 3 {
        med(pool3[pool3.len().saturating_sub(5)..].iter().copied())
    } else {
        med(it[it.len().saturating_sub(3)..].iter().map(|x| x.p))
    };
    let base_ok = antes.len() >= MIN_ANTES
        && match (antes.first(), antes.last()) {
            (Some(a), Some(b)) => (b.em - a.em) as f64 / DIA_F >= MIN_SPAN_BASE,
            _ => false,
        };
    let base = base_ok.then(|| med(antes.iter().map(|x| x.p)));
    let ma30 = base.unwrap_or_else(|| med(it.iter().map(|x| x.p)));
    let ma7 = if w7.is_empty() {
        ma30
    } else {
        med(w7.iter().copied())
    };
    // `base ? ... : null`: base 0 conta como ausente no JS (não acontece com preços > 0).
    let base_v = base.filter(|b| *b != 0.0);
    let desconto = base_v.map(|b| ultimo / b - 1.0);
    let ku = (MIN_V24 as f64).max((k24 as f64).min(30.0));
    let se = wz.s
        * 1.2533
        * (1.0 / ku
            + if base_ok {
                1.0 / antes.len() as f64
            } else {
                1.0
            })
        .sqrt();
    let z = base_v.map(|b| (js::ln(ultimo / b) / se).clamp(-9.0, 9.0));
    let var24h = (k24 >= 3 && p24.len() >= 5)
        .then(|| med(r24.iter().copied()) / med(p24.iter().copied()) - 1.0);
    let var7d = (w7.len() >= 3 && antes7.len() >= MIN_ANTES)
        .then(|| med(w7.iter().copied()) / med(antes7.iter().copied()) - 1.0);

    let com_volume: Vec<&Candle> = candles.iter().filter(|c| c.v >= 2).collect();
    let dc: Vec<&Candle> = if com_volume.len() < MIN_DIAS_OK {
        candles.iter().collect()
    } else {
        com_volume
    };
    let pontos: Vec<(f64, f64)> = dc[dc.len().saturating_sub(10)..]
        .iter()
        .map(|c| (c.dia as f64, js::ln(c.m)))
        .collect();
    let (inclinacao, tend_z, tend_ok) = tendencia_robusta(&pontos);
    let tendencia = if tend_ok {
        js::exp(inclinacao) - 1.0
    } else {
        0.0
    };

    let spd = w.len() as f64 / (JANELA_DIAS as f64).min(1f64.max(span_dias));
    let liquidez = if spd >= LIQ_ALTA {
        Liquidez::Alta
    } else if spd >= LIQ_MEDIA {
        Liquidez::Media
    } else {
        Liquidez::Baixa
    };
    let dias = candles.len();
    let historico = if base.is_none() {
        Historico::Insuficiente
    } else if dias >= MIN_DIAS_OK {
        Historico::Ok
    } else {
        Historico::Curto
    };
    let n = w.len() as f64;
    let confianca = js::round(
        100.0
            * (0.35 * n / (n + 25.0)
                + 0.25 * (dias as f64 / 10.0).min(1.0)
                + 0.25 * (spd / 6.0).min(1.0)
                + 0.15 * (1.0 - (wz.fora * 2.0).min(1.0))),
    );
    let _ = (wz.hi, wz.lo);

    Some(Analise {
        n: w.len(),
        dias,
        ultimo,
        ma7,
        ma30,
        base,
        desconto,
        z,
        var24h,
        var7d,
        tendencia,
        tend_z,
        tend_ok,
        vol24h: k24,
        vol7d: w7.len(),
        vol30d: w.len(),
        spd,
        liquidez,
        historico,
        confianca,
        escala: wz.s,
        outliers: wz.fora,
        min30: js::minimo(it.iter().map(|x| x.p)),
        max30: js::maximo(it.iter().map(|x| x.p)),
        max30_bruto: js::maximo(w.iter().map(|v| v.unidade)),
        spark: candles[candles.len().saturating_sub(14)..]
            .iter()
            .map(|c| c.m)
            .collect(),
        candles,
        ultima_venda: ultimo_item.em,
    })
}
