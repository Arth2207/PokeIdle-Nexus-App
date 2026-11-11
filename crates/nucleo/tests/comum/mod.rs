//! Utilidades compartilhadas pelos testes de paridade.

#![allow(dead_code, clippy::unwrap_used)]

use serde_json::Value;

/// Primeira diferença entre dois JSONs.
pub fn diferenca(a: &Value, b: &Value, caminho: &str) -> Option<String> {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => {
            let (x, y) = (x.as_f64().unwrap(), y.as_f64().unwrap());
            (x.to_bits() != y.to_bits() && x != y).then(|| format!("{caminho}: {x:?} ≠ {y:?}"))
        }
        (Value::Array(x), Value::Array(y)) => {
            if x.len() != y.len() {
                return Some(format!("{caminho}: tamanho {} ≠ {}", x.len(), y.len()));
            }
            x.iter()
                .zip(y)
                .enumerate()
                .find_map(|(i, (a, b))| diferenca(a, b, &format!("{caminho}[{i}]")))
        }
        (Value::Object(x), Value::Object(y)) => {
            let mut chaves: Vec<&String> = x.keys().chain(y.keys()).collect();
            chaves.sort();
            chaves.dedup();
            chaves.into_iter().find_map(|k| {
                let nulo = Value::Null;
                diferenca(
                    x.get(k).unwrap_or(&nulo),
                    y.get(k).unwrap_or(&nulo),
                    &format!("{caminho}.{k}"),
                )
            })
        }
        _ => (a != b).then(|| format!("{caminho}: {a} ≠ {b}")),
    }
}
