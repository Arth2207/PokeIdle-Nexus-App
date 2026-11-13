//! `JSON.stringify` do JS: compacto ou com recuo de 2 espaços, e números no formato do JS.
//!
//! O serializador do serde difere em dois pontos que mudam bytes: `123.0` sai "123.0" (o
//! JS escreve "123") e `1e21` sai "1e21" (o JS escreve "1e+21"). Strings são escapadas igual
//! (`\b \f \n \r \t`, `\u00xx` minúsculo, sem escapar `/` nem não-ASCII).

use serde_json::Value;

/// `Number.prototype.toString()` (base 10) para números finitos.
pub fn numero(x: f64) -> String {
    if x == 0.0 {
        return "0".into(); // inclui -0, que o JS escreve "0"
    }
    // `{:e}` dá os dígitos mais curtos que voltam ao mesmo valor (como o JS) e o expoente.
    let exp = format!("{:e}", x.abs());
    let (mant, e) = exp.split_once('e').unwrap_or((exp.as_str(), "0"));
    let e: i32 = e.parse().unwrap_or(0);
    let digitos: String = mant.chars().filter(char::is_ascii_digit).collect();
    let k = digitos.len() as i32;
    let n = e + 1;
    let corpo = if k <= n && n <= 21 {
        format!("{digitos}{}", "0".repeat((n - k) as usize))
    } else if 0 < n && n <= 21 {
        let (a, b) = digitos.split_at(n as usize);
        format!("{a}.{b}")
    } else if -6 < n && n <= 0 {
        format!("0.{}{digitos}", "0".repeat((-n) as usize))
    } else {
        let sinal = if n >= 1 { '+' } else { '-' };
        let ee = (n - 1).abs();
        if k == 1 {
            format!("{digitos}e{sinal}{ee}")
        } else {
            let (a, b) = digitos.split_at(1);
            format!("{a}.{b}e{sinal}{ee}")
        }
    };
    if x < 0.0 {
        format!("-{corpo}")
    } else {
        corpo
    }
}

fn escrever_numero(n: &serde_json::Number, out: &mut String) {
    if n.is_i64() || n.is_u64() {
        out.push_str(&n.to_string());
    } else {
        match n.as_f64() {
            Some(x) if x.is_finite() => out.push_str(&numero(x)),
            _ => out.push_str("null"),
        }
    }
}

fn escrever_str(s: &str, out: &mut String) {
    // O escape do serde para strings é o mesmo do JSON.stringify.
    out.push_str(&serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into()));
}

fn escrever(v: &Value, recuo: Option<&str>, atual: &str, out: &mut String) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => escrever_numero(n, out),
        Value::String(s) => escrever_str(s, out),
        Value::Array(a) => {
            if a.is_empty() {
                out.push_str("[]");
                return;
            }
            out.push('[');
            let dentro = recuo.map(|r| format!("{atual}{r}"));
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                if let Some(d) = &dentro {
                    out.push('\n');
                    out.push_str(d);
                }
                escrever(x, recuo, dentro.as_deref().unwrap_or(""), out);
            }
            if recuo.is_some() {
                out.push('\n');
                out.push_str(atual);
            }
            out.push(']');
        }
        Value::Object(o) => {
            if o.is_empty() {
                out.push_str("{}");
                return;
            }
            out.push('{');
            let dentro = recuo.map(|r| format!("{atual}{r}"));
            for (i, (k, x)) in o.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                if let Some(d) = &dentro {
                    out.push('\n');
                    out.push_str(d);
                }
                escrever_str(k, out);
                out.push(':');
                if recuo.is_some() {
                    out.push(' ');
                }
                escrever(x, recuo, dentro.as_deref().unwrap_or(""), out);
            }
            if recuo.is_some() {
                out.push('\n');
                out.push_str(atual);
            }
            out.push('}');
        }
    }
}

/// `JSON.stringify(v)`.
pub fn compacto(v: &Value) -> String {
    let mut out = String::new();
    escrever(v, None, "", &mut out);
    out
}

/// `JSON.stringify(v, null, 2)`.
pub fn bonito(v: &Value) -> String {
    let mut out = String::new();
    escrever(v, Some("  "), "", &mut out);
    out
}

#[cfg(test)]
mod testes {
    use super::*;
    use serde_json::json;

    #[test]
    fn numeros_como_o_js() {
        assert_eq!(numero(123.0), "123");
        assert_eq!(numero(-0.0), "0");
        assert_eq!(numero(0.1 + 0.2), "0.30000000000000004");
        assert_eq!(numero(1e21), "1e+21");
        assert_eq!(numero(1.5e21), "1.5e+21");
        assert_eq!(numero(1e20), "100000000000000000000");
        assert_eq!(numero(0.000001), "0.000001");
        assert_eq!(numero(1e-7), "1e-7");
        assert_eq!(numero(-2.5e-7), "-2.5e-7");
        assert_eq!(numero(1792254360000.0), "1792254360000");
        assert_eq!(numero(3.749), "3.749");
    }

    #[test]
    fn formatos_do_stringify() {
        let v = json!({"a": [1, 2.0, {"b": null}], "c": {}, "d": [], "e": "x\n\"y\""});
        assert_eq!(
            compacto(&v),
            r#"{"a":[1,2,{"b":null}],"c":{},"d":[],"e":"x\n\"y\""}"#
        );
        assert_eq!(
            bonito(&v),
            "{\n  \"a\": [\n    1,\n    2,\n    {\n      \"b\": null\n    }\n  ],\n  \"c\": {},\n  \"d\": [],\n  \"e\": \"x\\n\\\"y\\\"\"\n}"
        );
    }
}
