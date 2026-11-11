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

/// `+v` do JS (conversão para número). `None` é o `undefined`, que vira `NaN`.
pub fn mais(v: Option<&serde_json::Value>) -> f64 {
    use serde_json::Value;
    match v {
        None => f64::NAN,
        Some(Value::Null) => 0.0,
        Some(Value::Bool(b)) => f64::from(u8::from(*b)),
        Some(Value::Number(n)) => n.as_f64().unwrap_or(f64::NAN),
        Some(Value::String(s)) => texto_para_numero(s),
        Some(Value::Array(a)) => match a.as_slice() {
            [] => 0.0,
            [um] if !um.is_array() && !um.is_object() => match um {
                Value::Null => 0.0,
                Value::String(s) => texto_para_numero(s),
                Value::Number(_) => mais(Some(um)),
                _ => f64::NAN,
            },
            _ => f64::NAN,
        },
        Some(Value::Object(_)) => f64::NAN,
    }
}

/// `Number(texto)`: espaços nas pontas ignorados, vazio vira 0, hexadecimal com `0x`, e só
/// `Infinity` escrito assim (o `parse` do Rust aceitaria `inf` e `nan`).
fn texto_para_numero(texto: &str) -> f64 {
    let t = texto.trim();
    if t.is_empty() {
        return 0.0;
    }
    if let Some(hex) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        return u64::from_str_radix(hex, 16).map_or(f64::NAN, |n| n as f64);
    }
    match t {
        "Infinity" | "+Infinity" => return f64::INFINITY,
        "-Infinity" => return f64::NEG_INFINITY,
        _ => {}
    }
    if t.bytes()
        .any(|b| b.is_ascii_alphabetic() && b != b'e' && b != b'E')
    {
        return f64::NAN;
    }
    t.parse().unwrap_or(f64::NAN)
}

/// Veracidade do JS (`!!v`): `undefined`, `null`, `false`, `0`, `NaN` e `""` são falsos.
pub fn verdadeiro(v: Option<&serde_json::Value>) -> bool {
    use serde_json::Value;
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|x| x != 0.0 && !x.is_nan()),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(_) | Value::Object(_)) => true,
    }
}

/// `Math.pow` do V8 (Node 20): porte do `pow` do fdlibm como o V8 11.3 o compila em
/// `base/ieee754.cc`. O `powf` da libc e o crate `libm` erram o último bit em relação a ele
/// em cerca de 1 caso em 10, o que basta para quebrar a paridade exata com o legado.
// As constantes ficam com todos os dígitos do fdlibm, para conferir com o original.
#[allow(
    clippy::many_single_char_names,
    clippy::too_many_lines,
    clippy::excessive_precision,
    clippy::approx_constant,
    clippy::eq_op
)]
pub fn pow(x: f64, y: f64) -> f64 {
    const BP: [f64; 2] = [1.0, 1.5];
    const DP_H: [f64; 2] = [0.0, 5.849_624_872_207_641_601_56e-1];
    const DP_L: [f64; 2] = [0.0, 1.350_039_202_129_748_971_28e-8];
    const TWO53: f64 = 9_007_199_254_740_992.0;
    const HUGE: f64 = 1.0e300;
    const TINY: f64 = 1.0e-300;
    const L1: f64 = 5.999_999_999_999_946_487_25e-1;
    const L2: f64 = 4.285_714_285_785_501_842_52e-1;
    const L3: f64 = 3.333_333_298_183_774_329_18e-1;
    const L4: f64 = 2.727_281_238_085_340_064_89e-1;
    const L5: f64 = 2.306_607_457_755_617_540_67e-1;
    const L6: f64 = 2.069_750_178_003_384_177_84e-1;
    const P1: f64 = 1.666_666_666_666_660_190_37e-1;
    const P2: f64 = -2.777_777_777_701_559_338_42e-3;
    const P3: f64 = 6.613_756_321_437_934_361_17e-5;
    const P4: f64 = -1.653_390_220_546_525_153_90e-6;
    const P5: f64 = 4.138_136_797_057_238_460_39e-8;
    const LG2: f64 = 6.931_471_805_599_452_862_27e-1;
    const LG2_H: f64 = 6.931_471_824_645_996_093_75e-1;
    const LG2_L: f64 = -1.904_654_299_957_768_045_25e-9;
    const OVT: f64 = 8.008_566_259_537_294_437_2e-17;
    const CP: f64 = 9.617_966_939_259_755_543_29e-1;
    const CP_H: f64 = 9.617_967_009_544_372_558_59e-1;
    const CP_L: f64 = -7.028_461_650_952_758_265_16e-9;
    const IVLN2: f64 = 1.442_695_040_888_963_387_00;
    const IVLN2_H: f64 = 1.442_695_021_629_333_496_09;
    const IVLN2_L: f64 = 1.925_962_991_126_617_468_87e-8;

    let alto = |v: f64| (v.to_bits() >> 32) as u32 as i32;
    let baixo = |v: f64| v.to_bits() as u32;
    let com_baixo =
        |v: f64, b: u32| f64::from_bits((v.to_bits() & 0xffff_ffff_0000_0000) | u64::from(b));
    let com_alto =
        |v: f64, a: i32| f64::from_bits((u64::from(a as u32) << 32) | (v.to_bits() & 0xffff_ffff));

    let (hx, lx) = (alto(x), baixo(x));
    let (hy, ly) = (alto(y), baixo(y));
    let mut ix = hx & 0x7fff_ffff;
    let iy = hy & 0x7fff_ffff;

    if (iy as u32 | ly) == 0 {
        return 1.0;
    }
    if ix > 0x7ff0_0000
        || (ix == 0x7ff0_0000 && lx != 0)
        || iy > 0x7ff0_0000
        || (iy == 0x7ff0_0000 && ly != 0)
    {
        return x + y;
    }

    // yisint: 0 não inteiro, 1 inteiro ímpar, 2 inteiro par (só importa com x < 0).
    let mut yisint = 0;
    if hx < 0 {
        if iy >= 0x4340_0000 {
            yisint = 2;
        } else if iy >= 0x3ff0_0000 {
            let k = (iy >> 20) - 0x3ff;
            if k > 20 {
                let j = ly >> (52 - k);
                if (j << (52 - k)) == ly {
                    yisint = 2 - (j & 1) as i32;
                }
            } else if ly == 0 {
                let j = iy >> (20 - k);
                if (j << (20 - k)) == iy {
                    yisint = 2 - (j & 1);
                }
            }
        }
    }

    if ly == 0 {
        if iy == 0x7ff0_0000 {
            return if ((ix - 0x3ff0_0000) as u32 | lx) == 0 {
                y - y
            } else if ix >= 0x3ff0_0000 {
                if hy >= 0 {
                    y
                } else {
                    0.0
                }
            } else if hy < 0 {
                -y
            } else {
                0.0
            };
        }
        if iy == 0x3ff0_0000 {
            return if hy < 0 { 1.0 / x } else { x };
        }
        if hy == 0x4000_0000 {
            return x * x;
        }
        if hy == 0x3fe0_0000 && hx >= 0 {
            return x.sqrt();
        }
    }

    let mut ax = x.abs();
    if lx == 0 && (ix == 0x7ff0_0000 || ix == 0 || ix == 0x3ff0_0000) {
        let mut z = ax;
        if hy < 0 {
            z = 1.0 / z;
        }
        if hx < 0 {
            if ((ix - 0x3ff0_0000) | yisint) == 0 {
                z = f64::NAN;
            } else if yisint == 1 {
                z = -z;
            }
        }
        return z;
    }

    let mut n = (hx >> 31) + 1;
    if (n | yisint) == 0 {
        return f64::NAN;
    }
    let s = if (n | (yisint - 1)) == 0 { -1.0 } else { 1.0 };

    let (t1, t2);
    if iy > 0x41e0_0000 {
        if iy > 0x43f0_0000 {
            if ix <= 0x3fef_ffff {
                return if hy < 0 { HUGE * HUGE } else { TINY * TINY };
            }
            if ix >= 0x3ff0_0000 {
                return if hy > 0 { HUGE * HUGE } else { TINY * TINY };
            }
        }
        if ix < 0x3fef_ffff {
            return if hy < 0 {
                s * HUGE * HUGE
            } else {
                s * TINY * TINY
            };
        }
        if ix > 0x3ff0_0000 {
            return if hy > 0 {
                s * HUGE * HUGE
            } else {
                s * TINY * TINY
            };
        }
        let t = ax - 1.0;
        let w = (t * t) * (0.5 - t * (0.333_333_333_333_333_333_333_3 - t * 0.25));
        let u = IVLN2_H * t;
        let v = t * IVLN2_L - w * IVLN2;
        t1 = com_baixo(u + v, 0);
        t2 = v - (t1 - u);
    } else {
        n = 0;
        if ix < 0x0010_0000 {
            ax *= TWO53;
            n -= 53;
            ix = alto(ax);
        }
        n += (ix >> 20) - 0x3ff;
        let j = ix & 0x000f_ffff;
        ix = j | 0x3ff0_0000;
        let k: usize = if j <= 0x3988E {
            0
        } else if j < 0xBB67A {
            1
        } else {
            n += 1;
            ix -= 0x0010_0000;
            0
        };
        ax = com_alto(ax, ix);

        let u = ax - BP[k];
        let v = 1.0 / (ax + BP[k]);
        let ss = u * v;
        let s_h = com_baixo(ss, 0);
        let t_h = com_alto(
            0.0,
            ((ix >> 1) | 0x2000_0000) + 0x0008_0000 + ((k as i32) << 18),
        );
        let t_l = ax - (t_h - BP[k]);
        let s_l = v * ((u - s_h * t_h) - s_h * t_l);
        let mut s2 = ss * ss;
        let mut r = s2 * s2 * (L1 + s2 * (L2 + s2 * (L3 + s2 * (L4 + s2 * (L5 + s2 * L6)))));
        r += s_l * (s_h + ss);
        s2 = s_h * s_h;
        let t_h = com_baixo(3.0 + s2 + r, 0);
        let t_l = r - ((t_h - 3.0) - s2);
        let u = s_h * t_h;
        let v = s_l * t_h + t_l * ss;
        let p_h = com_baixo(u + v, 0);
        let p_l = v - (p_h - u);
        let z_h = CP_H * p_h;
        let z_l = CP_L * p_h + p_l * CP + DP_L[k];
        let t = f64::from(n);
        t1 = com_baixo(((z_h + z_l) + DP_H[k]) + t, 0);
        t2 = z_l - (((t1 - t) - DP_H[k]) - z_h);
    }

    let y1 = com_baixo(y, 0);
    let p_l = (y - y1) * t1 + y * t2;
    let mut p_h = y1 * t1;
    let z = p_l + p_h;
    let (j, i) = (alto(z), baixo(z));
    if j >= 0x4090_0000 {
        if ((j - 0x4090_0000) as u32 | i) != 0 || p_l + OVT > z - p_h {
            return s * HUGE * HUGE;
        }
    } else if (j & 0x7fff_ffff) >= 0x4090_cc00
        && ((j.wrapping_sub(0xc090_cc00_u32 as i32)) as u32 | i != 0 || p_l <= z - p_h)
    {
        return s * TINY * TINY;
    }

    let i = j & 0x7fff_ffff;
    let mut k = (i >> 20) - 0x3ff;
    let mut n = 0i32;
    if i > 0x3fe0_0000 {
        n = j.wrapping_add(0x0010_0000 >> (k + 1));
        k = ((n & 0x7fff_ffff) >> 20) - 0x3ff;
        let t = com_alto(0.0, n & !(0x000f_ffff >> k));
        n = ((n & 0x000f_ffff) | 0x0010_0000) >> (20 - k);
        if j < 0 {
            n = -n;
        }
        p_h -= t;
    }
    let t = com_baixo(p_l + p_h, 0);
    let u = t * LG2_H;
    let v = (p_l - (t - p_h)) * LG2 + t * LG2_L;
    let mut z = u + v;
    let w = v - (z - u);
    let t = z * z;
    let t1 = z - t * (P1 + t * (P2 + t * (P3 + t * (P4 + t * P5))));
    let r = (z * t1) / ((t1 - 2.0) - (w + z * w));
    z = 1.0 - (r - z);
    let j = alto(z).wrapping_add(((n as u32) << 20) as i32);
    z = if (j >> 20) <= 0 {
        libm::scalbn(z, n)
    } else {
        com_alto(z, j)
    };
    s * z
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
