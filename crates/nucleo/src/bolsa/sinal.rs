//! Sinal de negociação (`sinalDe`) e veredito (`veredito`), com os mesmos textos do legado.

use super::estat::{self, Analise, Historico, Liquidez};
use super::indicadores::Indicadores;
use crate::js;

/// `pc` do `bolsa-stats.js`: `+12%` / `−5%` (sinal de menos tipográfico).
pub fn pc(x: Option<f64>, casas: usize) -> String {
    match x {
        None => "–".into(),
        Some(x) => format!(
            "{}{}%",
            if x >= 0.0 { '+' } else { '−' },
            js::to_fixed((x * 100.0).abs(), casas)
        ),
    }
}

/// `fmt` do `bolsa-stats.js`.
pub fn fmt(n: Option<f64>) -> String {
    let Some(n) = n else { return "–".into() };
    if n >= 1e9 {
        format!("{}B", js::to_fixed(n / 1e9, 2))
    } else if n >= 1e6 {
        format!("{}M", js::to_fixed(n / 1e6, 2))
    } else if n >= 1e4 {
        format!("{}k", js::to_fixed(n / 1e3, 1))
    } else if n >= 100.0 {
        js::numero(js::round(n))
    } else if n >= 10.0 {
        js::to_fixed(n, 1)
    } else {
        js::to_fixed(n, 2)
    }
}

/// `fmt` do `bolsa.js` (mais curto: abaixo de 10 mil, inteiro arredondado).
pub fn fmt_bolsa(n: Option<f64>) -> String {
    let Some(n) = n else { return "–".into() };
    if n >= 1e9 {
        format!("{}B", js::to_fixed(n / 1e9, 2))
    } else if n >= 1e6 {
        format!("{}M", js::to_fixed(n / 1e6, 2))
    } else if n >= 1e4 {
        format!("{}k", js::to_fixed(n / 1e3, 1))
    } else {
        js::numero(js::round(n))
    }
}

/// `pct` do `bolsa.js`: `+12%` / `-5%` (hífen comum).
pub fn pct(x: f64) -> String {
    format!(
        "{}{}%",
        if x >= 0.0 { "+" } else { "" },
        js::to_fixed(x * 100.0, 0)
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sinal {
    Comprar,
    Vender,
    Observar,
    Manter,
    SemBase,
}

impl Sinal {
    pub fn como_str(self) -> &'static str {
        match self {
            Sinal::Comprar => "COMPRAR",
            Sinal::Vender => "VENDER",
            Sinal::Observar => "OBSERVAR",
            Sinal::Manter => "MANTER",
            Sinal::SemBase => "SEM_BASE",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResultadoSinal {
    pub sinal: Sinal,
    pub score: f64,
    pub motivo: String,
}

/// Contexto do ticker para o sinal.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContextoSinal {
    pub pokemon: bool,
    pub orb: bool,
    pub normalizado: bool,
    pub ask_min: Option<f64>,
    pub ask_anuncios: Option<i64>,
}

fn dias_txt(d: usize) -> &'static str {
    if d > 1 {
        "s"
    } else {
        ""
    }
}

/// `sinalDe(a, o)`.
pub fn sinal_de(a: &Analise, o: &ContextoSinal) -> ResultadoSinal {
    let sem = |motivo: String| ResultadoSinal {
        sinal: Sinal::SemBase,
        score: 0.0,
        motivo,
    };
    if o.pokemon && !o.normalizado {
        return sem("Pokémon em coins: cartas de qualidade muito diferente e sem normalização — sinal desligado (só informativo).".into());
    }
    match a.historico {
        Historico::Insuficiente => {
            return sem(format!(
            "Sem histórico anterior para comparar ({} dia{} de vendas). O sinal liga com 5+ dias.",
            a.dias,
            dias_txt(a.dias)
        ))
        }
        Historico::Curto => {
            return sem(format!(
                "Histórico curto ({} dias com vendas; mínimo {}). Valores só informativos.",
                a.dias,
                estat::MIN_DIAS_OK
            ))
        }
        Historico::Ok => {}
    }
    // Com histórico ok, base, desconto e z existem.
    let (d, z) = (a.desconto.unwrap_or(0.0), a.z.unwrap_or(0.0));
    let tick_rel = if o.orb && a.ma30 < 40.0 {
        1.5 / a.ma30
    } else {
        0.0
    };
    let edge = 0.5f64.min(estat::EDGE.max(tick_rel));
    let liq_f = (a.spd / 3.0).min(1.0);
    let caindo = a.tend_ok && a.tend_z <= -estat::Z_TEND && a.tendencia <= -estat::TEND_MIN;
    let subindo = a.tend_ok && a.tend_z >= estat::Z_MOM && a.tendencia >= estat::TEND_MIN;
    let ctx = format!(
        "{} vendas/{} dias, liquidez {}",
        a.n,
        a.dias,
        a.liquidez.como_str()
    );
    let mut s = Sinal::Manter;
    let mut score = 0.0;
    let mut motivo = format!(
        "Preço dentro da faixa normal: {} da base (z {}). {ctx}.",
        pc(Some(d), 0),
        js::to_fixed(z, 1)
    );
    let ask_d = match o.ask_min {
        Some(ask) if ask != 0.0 && a.ma7 != 0.0 => Some(ask / a.ma7 - 1.0),
        _ => None,
    };
    let ask_barato = ask_d.is_some_and(|x| x <= -edge);

    if ask_barato && a.vol7d >= 3 {
        let ask_d = ask_d.unwrap_or(0.0);
        s = Sinal::Comprar;
        score = (ask_d.abs() / 0.1).min(6.0) * liq_f;
        motivo = format!(
            "Há anúncio a {}, {} da mediana de 7 dias ({}); {} anúncios abertos. {ctx}.",
            fmt(o.ask_min),
            pc(Some(ask_d), 0),
            fmt(Some(a.ma7)),
            o.ask_anuncios
                .map_or_else(|| "?".to_owned(), |n| n.to_string())
        );
    } else if d <= -edge && z <= -estat::Z {
        s = Sinal::Comprar;
        score = (-z).min(6.0) * liq_f;
        motivo = format!(
            "Preço {} abaixo da base de {} (z {}: desvio estatisticamente relevante). {ctx}.",
            pc(Some(d), 0),
            fmt(a.base),
            js::to_fixed(z, 1)
        );
    } else if subindo && d < edge {
        s = Sinal::Comprar;
        score = a.tend_z.min(4.0) * liq_f;
        motivo = format!(
            "Alta sustentada: {}/dia (Mann-Kendall z {}), preço ainda não esticado ({}). {ctx}.",
            pc(Some(a.tendencia), 1),
            js::to_fixed(a.tend_z, 1),
            pc(Some(d), 0)
        );
    } else if d >= edge && z >= estat::Z {
        s = Sinal::Vender;
        score = z.min(6.0) * liq_f;
        motivo = format!(
            "Preço {} acima da base de {} (z {}): bom momento para vender. {ctx}.",
            pc(Some(d), 0),
            fmt(a.base),
            js::to_fixed(z, 1)
        );
    } else if d.abs() >= edge {
        s = Sinal::Observar;
        score = 0.0;
        motivo = format!(
            "Desvio de {} sem relevância estatística (z {}): amostra pequena ou preço muito volátil. {ctx}.",
            pc(Some(d), 0),
            js::to_fixed(z, 1)
        );
    }

    if s == Sinal::Comprar && caindo && d < 0.0 && !ask_barato {
        s = Sinal::Observar;
        score = 0.0;
        motivo = format!(
            "Barato ({}), mas em queda sustentada ({}/dia): esperar estabilizar. {ctx}.",
            pc(Some(d), 0),
            pc(Some(a.tendencia), 1)
        );
    } else if s == Sinal::Comprar && caindo && ask_barato {
        motivo.push_str(&format!(
            " Atenção: tendência de queda ({}/dia).",
            pc(Some(a.tendencia), 1)
        ));
    }
    if s == Sinal::Vender && subindo && d < 2.0 * edge {
        s = Sinal::Observar;
        score = 0.0;
        motivo = format!(
            "Acima da base ({}), mas em alta sustentada ({}/dia): pode subir mais. {ctx}.",
            pc(Some(d), 0),
            pc(Some(a.tendencia), 1)
        );
    }
    if matches!(s, Sinal::Comprar | Sinal::Vender) && a.vol24h < estat::MIN_V24 {
        s = Sinal::Observar;
        score = 0.0;
        motivo = format!(
            "Poucas vendas em 24h ({}) para confirmar o movimento. {ctx}.",
            a.vol24h
        );
    }
    if a.liquidez == Liquidez::Baixa && s != Sinal::Manter {
        motivo.push_str(" Pouca liquidez: sinal menos confiável.");
    }
    ResultadoSinal {
        sinal: s,
        score: js::round(score * 10.0) / 10.0,
        motivo,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Acao {
    Comprar,
    Vender,
    AguardarParaComprar,
    AguardarParaVender,
    Manter,
    SemBase,
}

impl Acao {
    pub fn como_str(self) -> &'static str {
        match self {
            Acao::Comprar => "COMPRAR",
            Acao::Vender => "VENDER",
            Acao::AguardarParaComprar => "AGUARDAR_PARA_COMPRAR",
            Acao::AguardarParaVender => "AGUARDAR_PARA_VENDER",
            Acao::Manter => "MANTER",
            Acao::SemBase => "SEM_BASE",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Veredito {
    pub acao: Acao,
    pub score: f64,
    pub confianca: f64,
    pub provisorio: bool,
    pub preco_alvo_venda: Option<f64>,
    pub preco_alvo_compra: Option<f64>,
    pub stop: Option<f64>,
    pub motivos: Vec<String>,
}

impl Veredito {
    pub fn sem_base(motivo: &str) -> Self {
        Self {
            acao: Acao::SemBase,
            score: 0.0,
            confianca: 0.0,
            provisorio: false,
            preco_alvo_venda: None,
            preco_alvo_compra: None,
            stop: None,
            motivos: vec![motivo.to_owned()],
        }
    }
}

/// Estatísticas das ofertas abertas do ticker.
#[derive(Debug, Clone, PartialEq)]
pub struct EstatOfertas {
    pub n: i64,
    pub min: f64,
    pub p25: f64,
    pub med: f64,
    pub p75: f64,
    pub max: f64,
}

/// `Math.max(a, Math.min(b, x))`: NaN continua NaN, como no JS.
fn clip(x: f64, a: f64, b: f64) -> f64 {
    if x.is_nan() {
        f64::NAN
    } else {
        a.max(b.min(x))
    }
}

/// `veredito(t, ctx)`. Score > 0 favorece compra; < 0, venda.
pub fn veredito(
    a: &Analise,
    ind: &Indicadores,
    pokemon: bool,
    normalizado: bool,
    ofertas: Option<&EstatOfertas>,
    ask_min: Option<f64>,
) -> Veredito {
    if pokemon && !normalizado {
        return Veredito::sem_base(
            "Pokémon em coins: cartas de qualidade muito diferente e sem normalização — só informativo.",
        );
    }
    let prov = a.historico != Historico::Ok;
    let referencia = a.base.or(Some(a.ma7)).filter(|r| *r != 0.0 && !r.is_nan());
    let Some(rf) = referencia else {
        return Veredito::sem_base("Sem preço de referência.");
    };
    let ult = a.ultimo;
    let d = ult / rf - 1.0;
    let mut m: Vec<String> = Vec::new();
    if prov {
        m.push(format!(
            "Veredito PROVISÓRIO: só {} dia{} de histórico (o sinal firme liga com {}+). Confiança baixa.",
            a.dias,
            dias_txt(a.dias),
            estat::MIN_DIAS_OK
        ));
    }
    let caindo = a.tend_ok && a.tend_z <= -estat::Z_TEND && a.tendencia <= -estat::TEND_MIN;
    let subindo = a.tend_ok && a.tend_z >= estat::Z_MOM && a.tendencia >= estat::TEND_MIN;
    let mut s = 0.0;
    let mut n = 0usize;
    let mut add = |v: f64, txt: Option<String>, m: &mut Vec<String>| {
        s += v;
        n += 1;
        if let Some(t) = txt {
            m.push(t);
        }
    };
    add(
        clip(-d / 0.25, -1.0, 1.0) * 30.0,
        Some(format!(
            "Preço {} da {} ({}).",
            pc(Some(ult / rf - 1.0), 0),
            if a.base.is_some_and(|b| b != 0.0) {
                "base"
            } else {
                "mediana"
            },
            fmt(Some(rf))
        )),
        &mut m,
    );
    if let Some(z) = a.z {
        add(
            clip(-z / 2.5, -1.0, 1.0) * 20.0,
            (z.abs() >= 1.5).then(|| {
                format!(
                    "Desvio z {}: {} do normal.",
                    js::to_fixed(z, 1),
                    if z < 0.0 { "abaixo" } else { "acima" }
                )
            }),
            &mut m,
        );
    }
    if let Some(rsi) = ind.rsi {
        add(
            clip((50.0 - rsi) / 25.0, -1.0, 1.0) * 15.0,
            if rsi <= 30.0 {
                Some(format!("RSI {}: sobrevendido.", js::numero(rsi)))
            } else if rsi >= 70.0 {
                Some(format!("RSI {}: sobrecomprado.", js::numero(rsi)))
            } else {
                None
            },
            &mut m,
        );
    }
    if let Some(b) = &ind.boll {
        // `pb` nulo vale 0 na aritmética e nas comparações do JS (`.5 - null === .5`).
        let pb = b.pb.unwrap_or(0.0);
        add(
            clip((0.5 - pb) / 0.5, -1.0, 1.0) * 10.0,
            if pb <= 0.1 {
                Some("Colado na banda inferior de Bollinger.".into())
            } else if pb >= 0.9 {
                Some("Colado na banda superior de Bollinger.".into())
            } else {
                None
            },
            &mut m,
        );
    }
    let ref_ask = ofertas
        .map(|o| o.med)
        .filter(|v| *v != 0.0 && !v.is_nan() && !normalizado);
    let ask = ask_min.filter(|v| *v != 0.0 && !v.is_nan() && !normalizado);
    let ma7_ok = a.ma7 != 0.0 && !a.ma7.is_nan();
    if let (Some(ask), true) = (ask, ma7_ok) {
        let r = ask / a.ma7 - 1.0;
        add(
            clip(-r / 0.2, -1.0, 1.0) * 15.0,
            Some(format!(
                "Menor anúncio aberto {} ({} da mediana 7d).",
                fmt(Some(ask)),
                pc(Some(r), 0)
            )),
            &mut m,
        );
    }
    if let (Some(ra), true) = (ref_ask, ma7_ok) {
        let r = ra / a.ma7 - 1.0;
        if r.abs() >= 0.1 {
            m.push(format!(
                "Mediana dos anúncios abertos {} ({} da mediana 7d).",
                fmt(Some(ra)),
                pc(Some(r), 0)
            ));
        }
    }
    if a.tend_ok {
        add(
            clip(a.tendencia / 0.05, -1.0, 1.0) * 10.0,
            if subindo {
                Some(format!(
                    "Tendência de alta {}/dia (Mann-Kendall z {}).",
                    pc(Some(a.tendencia), 1),
                    js::to_fixed(a.tend_z, 1)
                ))
            } else if caindo {
                Some(format!(
                    "Tendência de queda {}/dia (Mann-Kendall z {}).",
                    pc(Some(a.tendencia), 1),
                    js::to_fixed(a.tend_z, 1)
                ))
            } else {
                None
            },
            &mut m,
        );
    } else if let Some(mom) = ind.mom {
        add(
            clip(mom / 0.3, -1.0, 1.0) * 5.0,
            Some(format!("Momentum {}d {}.", ind.mom_per, pc(Some(mom), 0))),
            &mut m,
        );
    }
    if let Some(macd) = &ind.macd {
        m.push(format!(
            "MACD {}{}.",
            if macd.hist >= 0.0 {
                "positivo"
            } else {
                "negativo"
            },
            if macd.adaptado {
                " (períodos adaptados)"
            } else {
                ""
            }
        ));
    }
    let liq = match a.liquidez {
        Liquidez::Baixa => 0.6,
        Liquidez::Media => 0.85,
        Liquidez::Alta => 1.0,
    };
    let s = js::round(s * liq * 10.0) / 10.0;
    let mut acao = Acao::Manter;
    if a.vol24h < estat::MIN_V24 && s.abs() >= 12.0 {
        m.push(format!(
            "Poucas vendas em 24h ({}): sem confirmação do movimento.",
            a.vol24h
        ));
    } else if s >= 25.0 {
        acao = if caindo && s < 45.0 {
            Acao::AguardarParaComprar
        } else {
            Acao::Comprar
        };
    } else if s >= 12.0 {
        acao = Acao::AguardarParaComprar;
    } else if s <= -25.0 {
        acao = if subindo && s > -45.0 {
            Acao::AguardarParaVender
        } else {
            Acao::Vender
        };
    } else if s <= -12.0 {
        acao = Acao::AguardarParaVender;
    }
    if acao == Acao::Manter {
        m.insert(
            0,
            format!("Preço dentro da faixa normal (score {}).", js::numero(s)),
        );
    }
    if a.liquidez == Liquidez::Baixa {
        m.push("Pouca liquidez: execução lenta e sinal menos confiável.".into());
    }
    let vol = ind.vol.filter(|v| *v != 0.0);
    let edge = estat::EDGE.max(vol.map_or(0.0, |v| (v * 2.2).min(0.3)));
    let alvo_v = ult.max(
        ind.boll
            .as_ref()
            .map_or(a.max30, |b| b.sup.max(rf))
            .min(a.max30)
            .min(rf * (1.0 + 2.0 * edge)),
    );
    let alvo_c = a.min30.max(
        ult.min(rf * (1.0 - edge))
            .min(ind.boll.as_ref().map_or(ult, |b| b.inf.max(a.min30))),
    );
    let arr = |x: f64| {
        if x >= 100.0 {
            js::round(x)
        } else {
            js::round(x * 100.0) / 100.0
        }
    };
    let mut conf = a.confianca
        * (0.55 + 0.45 * (n as f64 / 5.0).min(1.0))
        * if a.liquidez == Liquidez::Baixa {
            0.8
        } else {
            1.0
        };
    if prov {
        conf = conf.min(40.0);
    }
    m.truncate(7);
    Veredito {
        acao,
        score: s,
        confianca: js::round(conf),
        provisorio: prov,
        preco_alvo_venda: Some(arr(alvo_v)),
        preco_alvo_compra: Some(arr(alvo_c)),
        stop: Some(arr(alvo_c * (1.0 - 0.08f64.max(2.0 * vol.unwrap_or(0.0))))),
        motivos: m,
    }
}
