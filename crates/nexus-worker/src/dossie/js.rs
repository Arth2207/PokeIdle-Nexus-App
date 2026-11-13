//! Coerções do JS usadas pelo `dossie-server.js`, para a porta dar o mesmo resultado
//! com entradas estranhas (string no lugar de número, `null`, campo ausente...).
//!
//! `Option<&Value>`: `None` é o `undefined` do JS (campo ausente).

use serde_json::Value;

/// `Number(texto)` do JS. Diferente do `parse` do Rust: aceita hexa/octal/binário,
/// `Infinity`, string vazia = 0, e recusa `inf`/`nan`.
/// Espaço em branco do `trim()`/`Number()` do JS (o do Rust inclui U+0085 e não o BOM).
pub fn espaco_js(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202f}'
                | '\u{205f}'
                | '\u{3000}'
                | '\u{feff}'
    )
}

pub fn numero_str(s: &str) -> f64 {
    let t = s.trim_matches(espaco_js);
    if t.is_empty() {
        return 0.0;
    }
    match t {
        "Infinity" | "+Infinity" => return f64::INFINITY,
        "-Infinity" => return f64::NEG_INFINITY,
        _ => {}
    }
    for (prefixo, base) in [
        ("0x", 16),
        ("0X", 16),
        ("0o", 8),
        ("0O", 8),
        ("0b", 2),
        ("0B", 2),
    ] {
        if let Some(resto) = t.strip_prefix(prefixo) {
            // Sem sinal depois do prefixo; acima de u64 o JS ainda dá um número (aproximado).
            if resto.is_empty() || !resto.chars().all(|c| c.is_digit(base)) {
                return f64::NAN;
            }
            return resto
                .chars()
                .filter_map(|c| c.to_digit(base))
                .fold(0.0, |acc, d| acc * f64::from(base) + f64::from(d));
        }
    }
    let valido = t
        .chars()
        .all(|c| c.is_ascii_digit() || matches!(c, '.' | 'e' | 'E' | '+' | '-'));
    if !valido {
        return f64::NAN;
    }
    t.parse().unwrap_or(f64::NAN)
}

/// `+v` do JS.
pub fn numero(v: Option<&Value>) -> f64 {
    match v {
        None => f64::NAN,
        Some(Value::Null) => 0.0,
        Some(Value::Bool(b)) => f64::from(u8::from(*b)),
        Some(Value::Number(n)) => n.as_f64().unwrap_or(f64::NAN),
        Some(Value::String(s)) => numero_str(s),
        // Array e objeto passam por `String()` antes (`+[5]` = 5, `+{}` = NaN).
        Some(outro) => numero_str(&texto(Some(outro))),
    }
}

/// `String(v)` do JS.
pub fn texto(v: Option<&Value>) -> String {
    match v {
        None => "undefined".into(),
        Some(Value::Null) => "null".into(),
        Some(Value::Bool(b)) => b.to_string(),
        Some(Value::Number(n)) => nucleo::js::numero(n.as_f64().unwrap_or(f64::NAN)),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(a)) => a
            .iter()
            .map(|x| match x {
                Value::Null => String::new(),
                outro => texto(Some(outro)),
            })
            .collect::<Vec<_>>()
            .join(","),
        Some(Value::Object(_)) => "[object Object]".into(),
    }
}

/// Valor "verdadeiro" no JS (`if (v)`).
pub fn verdade(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|x| x != 0.0 && !x.is_nan()),
        Some(Value::String(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

/// `a ?? b`: `null` e `undefined` caem para o próximo.
pub fn ou<'a>(a: Option<&'a Value>, b: Option<&'a Value>) -> Option<&'a Value> {
    match a {
        None | Some(Value::Null) => b,
        _ => a,
    }
}

/// Campo de objeto (`v?.campo`); `None` se `v` não é objeto.
pub fn campo<'a>(v: Option<&'a Value>, nome: &str) -> Option<&'a Value> {
    v.and_then(|x| x.as_object()).and_then(|o| o.get(nome))
}

/// `texto.slice(0, n)` do JS, que conta em unidades UTF-16. Se o corte cair no meio de um
/// par substituto (emoji), o JS deixaria meia letra; aqui a letra inteira sai.
pub fn fatia_utf16(s: &str, n: usize) -> String {
    let mut usado = 0;
    let mut fim = 0;
    for (i, c) in s.char_indices() {
        usado += c.len_utf16();
        if usado > n {
            return s[..i].to_owned();
        }
        fim = i + c.len_utf8();
    }
    s[..fim].to_owned()
}

/// `parseInt(texto, 10)`: dígitos do começo (com sinal), `None` se não houver.
pub fn parse_int(s: &str) -> Option<i64> {
    let t = s.trim_start();
    let (sinal, resto) = match t.as_bytes().first() {
        Some(b'-') => (-1, &t[1..]),
        Some(b'+') => (1, &t[1..]),
        _ => (1, t),
    };
    let digitos: String = resto.chars().take_while(char::is_ascii_digit).collect();
    digitos.parse::<i64>().ok().map(|v| sinal * v)
}

/// Número do JS para JSON: inteiro sai sem `.0`, como no `JSON.stringify`.
pub fn json_num(x: f64) -> Value {
    if x.is_finite() && x.fract() == 0.0 && x.abs() < 9.007_199_254_740_992e15 {
        Value::from(x as i64)
    } else if x.is_finite() {
        Value::from(x)
    } else {
        // JSON.stringify(NaN/Infinity) = null
        Value::Null
    }
}

/// Valor de um `$n` como o node-pg manda para o Postgres (texto), ou `None` para NULL.
/// Array vira literal de array do Postgres; objeto vira JSON (a ordem de chaves numéricas
/// do `JSON.stringify` não é reproduzida: caso patológico).
pub fn param_pg(v: Option<&Value>) -> Option<String> {
    match v? {
        Value::Null => None,
        Value::String(s) => Some(s.clone()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Number(n) => Some(nucleo::js::numero(n.as_f64().unwrap_or(f64::NAN))),
        Value::Array(a) => Some(literal_array(a)),
        outro => Some(outro.to_string()),
    }
}

/// Número "verdadeiro" (`+x || …`): Infinity conta, 0 e NaN não.
pub fn verdade_num(x: f64) -> bool {
    x != 0.0 && !x.is_nan()
}

/// `arrayString` do node-pg: `["a", 1, null]` → `{"a","1",NULL}`.
fn literal_array(a: &[Value]) -> String {
    let itens: Vec<String> = a
        .iter()
        .map(|v| match v {
            Value::Null => "NULL".to_owned(),
            Value::Array(x) => literal_array(x),
            outro => {
                let t = match outro {
                    Value::String(s) => s.clone(),
                    Value::Object(_) => outro.to_string(),
                    x => texto(Some(x)),
                };
                format!("\"{}\"", t.replace('\\', "\\\\").replace('"', "\\\""))
            }
        })
        .collect();
    format!("{{{}}}", itens.join(","))
}

/// `v || null` seguido do envio do node-pg.
pub fn param_ou_null(v: Option<&Value>) -> Option<String> {
    if verdade(v) {
        param_pg(v)
    } else {
        None
    }
}

#[cfg(test)]
mod testes {
    use super::*;
    use serde_json::json;

    #[test]
    fn numero_como_o_js() {
        assert_eq!(numero_str(" 12 "), 12.0);
        assert_eq!(numero_str(""), 0.0);
        assert_eq!(numero_str("0x10"), 16.0);
        assert_eq!(numero_str("1e3"), 1000.0);
        assert!(numero_str("inf").is_nan());
        assert!(numero_str("12abc").is_nan());
        assert_eq!(numero_str("-Infinity"), f64::NEG_INFINITY);
        assert!(numero(None).is_nan());
        assert_eq!(numero(Some(&json!(null))), 0.0);
        assert_eq!(numero(Some(&json!([7]))), 7.0);
        assert!(numero(Some(&json!({}))).is_nan());
        assert_eq!(numero(Some(&json!(true))), 1.0);
    }

    #[test]
    fn texto_como_o_js() {
        assert_eq!(texto(Some(&json!([1, null, "a"]))), "1,,a");
        assert_eq!(texto(Some(&json!(1.5))), "1.5");
        assert_eq!(texto(Some(&json!(3))), "3");
        assert_eq!(texto(Some(&json!({"a": 1}))), "[object Object]");
    }

    #[test]
    fn fatia_conta_utf16() {
        assert_eq!(fatia_utf16("abc", 2), "ab");
        assert_eq!(fatia_utf16("a😀b", 3), "a😀");
        assert_eq!(fatia_utf16("a😀b", 2), "a");
        assert_eq!(fatia_utf16("é", 5), "é");
    }

    #[test]
    fn parse_int_como_o_js() {
        assert_eq!(parse_int("22P02"), Some(22));
        assert_eq!(parse_int("428C9"), Some(428));
        assert_eq!(parse_int("XX000"), None);
        assert_eq!(parse_int("413"), Some(413));
    }

    #[test]
    fn array_como_o_node_pg() {
        assert_eq!(
            param_pg(Some(&json!(["a\"b", 1, null, [2]]))).as_deref(),
            Some("{\"a\\\"b\",\"1\",NULL,{\"2\"}}")
        );
        assert!(numero_str("0x+1").is_nan());
        assert!(numero_str("\u{85}").is_nan());
        assert_eq!(numero_str("\u{feff}7\u{a0}"), 7.0);
    }

    #[test]
    fn json_num_sem_ponto_zero() {
        assert_eq!(json_num(3.0).to_string(), "3");
        assert_eq!(json_num(1.5).to_string(), "1.5");
        assert_eq!(json_num(f64::NAN), Value::Null);
    }
}
