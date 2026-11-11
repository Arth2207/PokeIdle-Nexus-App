//! Comportamentos numéricos e de texto do JavaScript que o legado usa e que precisam sair
//! idênticos aqui para as respostas baterem (arredondamento, `toFixed`, datas em UTC−3).

/// `Math.round` do JS: arredonda meio para cima (em direção a +∞), inclusive em negativos
/// (`Math.round(-2.5) === -2`). O `f64::round` do Rust arredonda meio para longe do zero.
pub fn round(x: f64) -> f64 {
    let piso = x.floor();
    if x - piso >= 0.5 {
        piso + 1.0
    } else {
        piso
    }
}

/// `Math.round(x * 10^casas) / 10^casas`, como o legado faz em vários lugares.
pub fn round_casas(x: f64, casas: i32) -> f64 {
    let fator = 10f64.powi(casas);
    round(x * fator) / fator
}

/// `Number.prototype.toFixed`. O JS arredonda o valor binário exato e, no empate exato,
/// escolhe o maior; o `format!` do Rust desempata para o par. Para valores do dia a dia a
/// diferença é rara, mas existe (`(0.5).toFixed(0)` é "1" no JS e "0" no Rust).
pub fn to_fixed(x: f64, casas: usize) -> String {
    if x.is_nan() {
        return "NaN".into();
    }
    if x.is_infinite() {
        return if x > 0.0 { "Infinity" } else { "-Infinity" }.into();
    }
    if x.abs() >= 1e21 {
        return numero(x);
    }
    // Com 1100 casas, o Rust escreve a expansão decimal exata do f64 (no máximo 1074).
    let exato = format!("{:.1100}", x.abs());
    let (inteira, fracao) = exato.split_once('.').unwrap_or((&exato, ""));
    let mut digitos: Vec<u8> = inteira
        .bytes()
        .chain(fracao.bytes().take(casas))
        .map(|b| b - b'0')
        .collect();
    let arredonda = fracao.as_bytes().get(casas).is_some_and(|&b| b >= b'5');
    if arredonda {
        // Soma 1 na última casa, propagando o "vai um".
        let mut i = digitos.len();
        loop {
            if i == 0 {
                digitos.insert(0, 1);
                break;
            }
            i -= 1;
            if digitos[i] == 9 {
                digitos[i] = 0;
            } else {
                digitos[i] += 1;
                break;
            }
        }
    }
    let texto: String = digitos.iter().map(|d| char::from(b'0' + d)).collect();
    let (int, frac) = texto.split_at(texto.len() - casas);
    let int = if int.is_empty() { "0" } else { int };
    let sinal = if x < 0.0 { "-" } else { "" };
    if casas == 0 {
        format!("{sinal}{int}")
    } else {
        format!("{sinal}{int}.{frac}")
    }
}

/// `String(numero)` do JS para os valores que aparecem em textos (sem notação científica).
pub fn numero(x: f64) -> String {
    if x.is_nan() {
        "NaN".into()
    } else if x.is_infinite() {
        if x > 0.0 { "Infinity" } else { "-Infinity" }.into()
    } else if x == 0.0 {
        "0".into()
    } else {
        // O Display do f64 no Rust já imprime a menor representação que volta ao mesmo valor
        // e omite ".0" em inteiros, como o JS.
        format!("{x}")
    }
}

const DIA_MS: i64 = 86_400_000;
/// Fuso de São Paulo fixo (UTC−3), como no legado.
const SP_MS: i64 = -3 * 3_600_000;

/// Número do dia (desde 1970-01-01) do instante, no fuso UTC−3.
pub fn dia_sp(em_ms: i64) -> i64 {
    (em_ms + SP_MS).div_euclid(DIA_MS)
}

/// `new Date(at - 3h).toISOString().slice(0, 10)`: a data `AAAA-MM-DD` em UTC−3.
pub fn data_sp(em_ms: i64) -> String {
    data_do_dia(dia_sp(em_ms))
}

/// Converte número do dia em data civil (algoritmo de Howard Hinnant, sem crate de datas).
pub fn data_do_dia(dia: i64) -> String {
    let z = dia + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Inverso de [`data_do_dia`] para `AAAA-MM-DD`. `None` se o texto não for uma data.
pub fn dia_da_data(texto: &str) -> Option<i64> {
    let mut partes = texto.splitn(3, '-');
    let y: i64 = partes.next()?.parse().ok()?;
    let m: i64 = partes.next()?.parse().ok()?;
    let d: i64 = partes.next()?.parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

/// Ordena números como `[...a].sort((x, y) => x - y)`.
pub fn ordenados(valores: impl IntoIterator<Item = f64>) -> Vec<f64> {
    let mut v: Vec<f64> = valores.into_iter().collect();
    v.sort_by(f64::total_cmp);
    v
}

/// Quantil com interpolação linear sobre uma lista já ordenada (`quant` do legado).
pub fn quantil(ordenada: &[f64], q: f64) -> Option<f64> {
    if ordenada.is_empty() {
        return None;
    }
    let p = (ordenada.len() - 1) as f64 * q;
    let lo = p.floor() as usize;
    let hi = p.ceil() as usize;
    Some(ordenada[lo] + (ordenada[hi] - ordenada[lo]) * (p - lo as f64))
}

/// Mediana (quantil 0,5) de valores em qualquer ordem.
pub fn mediana(valores: impl IntoIterator<Item = f64>) -> Option<f64> {
    quantil(&ordenados(valores), 0.5)
}

/// Mediana do `bolsa.js` (`(a + b) / 2` no caso par). Difere da [`mediana`] do
/// `bolsa-stats.js` (`a + (b - a) * 0,5`) no último bit, por isso as duas existem.
pub fn mediana_meio(valores: impl IntoIterator<Item = f64>) -> Option<f64> {
    let s = ordenados(valores);
    let m = s.len() / 2;
    match s.len() {
        0 => None,
        n if n % 2 == 1 => Some(s[m]),
        _ => Some((s[m - 1] + s[m]) / 2.0),
    }
}

/// Média aritmética somando da esquerda para a direita, como o `reduce` do JS.
pub fn media(valores: &[f64]) -> f64 {
    valores.iter().fold(0.0, |s, v| s + v) / valores.len() as f64
}

/// `Math.max(...lista)`: −∞ para lista vazia.
pub fn maximo(valores: impl IntoIterator<Item = f64>) -> f64 {
    valores.into_iter().fold(f64::NEG_INFINITY, |m, v| {
        if v.is_nan() || m.is_nan() {
            f64::NAN
        } else {
            m.max(v)
        }
    })
}

/// `Math.min(...lista)`: +∞ para lista vazia.
pub fn minimo(valores: impl IntoIterator<Item = f64>) -> f64 {
    valores.into_iter().fold(f64::INFINITY, |m, v| {
        if v.is_nan() || m.is_nan() {
            f64::NAN
        } else {
            m.min(v)
        }
    })
}

/// `norm` do precificador: decompõe acentos (NFD), remove as marcas, minúsculas e trim.
pub fn normalizar_nome(texto: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    texto
        .nfd()
        .filter(|c| !('\u{0300}'..='\u{036f}').contains(c))
        .collect::<String>()
        .to_lowercase()
        .trim()
        .to_owned()
}

#[cfg(test)]
mod testes {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn round_como_js() {
        assert_eq!(round(2.5), 3.0);
        assert_eq!(round(-2.5), -2.0);
        assert_eq!(round(0.49999999999999994), 0.0);
        assert_eq!(round(-0.4), -0.0);
    }

    #[test]
    fn to_fixed_como_js() {
        // Valores conferidos no Node.
        assert_eq!(to_fixed(0.5, 0), "1");
        assert_eq!(to_fixed(2.5, 0), "3");
        assert_eq!(to_fixed(1.005, 2), "1.00");
        assert_eq!(to_fixed(1.255, 2), "1.25");
        assert_eq!(to_fixed(0.125, 2), "0.13");
        assert_eq!(to_fixed(-1.5, 1), "-1.5");
        assert_eq!(to_fixed(-0.001, 2), "-0.00");
        assert_eq!(to_fixed(99.95, 1), "100.0");
        assert_eq!(to_fixed(12345.678, 0), "12346");
        assert_eq!(to_fixed(0.0, 2), "0.00");
    }

    #[test]
    fn datas_em_sp() {
        // 2026-11-11 02:00 UTC ainda é dia 10 em São Paulo.
        assert_eq!(data_sp(1_794_362_400_000), "2026-11-10");
        assert_eq!(data_do_dia(0), "1970-01-01");
        assert_eq!(dia_da_data("2026-11-10"), Some(dia_sp(1_794_362_400_000)));
    }

    #[test]
    fn normaliza_acentos() {
        assert_eq!(normalizar_nome("  Flabébé "), "flabebe");
    }

    proptest! {
        #[test]
        fn data_ida_e_volta(dia in -100_000i64..100_000) {
            prop_assert_eq!(dia_da_data(&data_do_dia(dia)), Some(dia));
        }

        #[test]
        fn to_fixed_bate_com_format_fora_do_empate(x in -1e9f64..1e9, casas in 0usize..4) {
            // Fora de empates exatos, os dois arredondamentos coincidem.
            let a = to_fixed(x, casas);
            let b = format!("{:.*}", casas, x);
            let empate = format!("{:.1100}", x.abs())
                .split_once('.')
                .map(|(_, f)| f.as_bytes().get(casas) == Some(&b'5') && f[casas + 1..].bytes().all(|c| c == b'0'))
                .unwrap_or(false);
            if !empate && !(b.starts_with("-0") && !a.starts_with('-')) {
                prop_assert_eq!(a, b);
            }
        }
    }
}
