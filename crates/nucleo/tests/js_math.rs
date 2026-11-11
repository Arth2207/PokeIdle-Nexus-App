//! Portes de `Math.log`, `Math.exp` do V8 conferidos bit a bit com o Node
//! (`ferramentas/fixtures/js-math.cjs`).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::Value;

fn confere(nome: &str, f: fn(f64) -> f64) {
    let v: Value = serde_json::from_str(include_str!("fixtures/js-math.json")).unwrap();
    let casos = v[nome].as_array().unwrap();
    let mut erros = Vec::new();
    for c in casos {
        let x = c[0].as_f64().unwrap();
        let nosso = f(x);
        let ok = match c[1].as_f64() {
            Some(js) => nosso.to_bits() == js.to_bits() || (nosso == 0.0 && js == 0.0),
            None => !nosso.is_finite(),
        };
        if !ok {
            erros.push(format!("{nome}({x:?}): nexus={nosso:?} js={}", c[1]));
        }
    }
    assert!(
        erros.is_empty(),
        "{} de {}:\n{}",
        erros.len(),
        casos.len(),
        erros[..erros.len().min(10)].join("\n")
    );
}

#[test]
fn ln_igual_ao_v8() {
    confere("ln", nucleo::js::ln);
}

#[test]
fn exp_igual_ao_v8() {
    confere("exp", nucleo::js::exp);
}

#[test]
fn libc_diverge_do_v8() {
    // Documenta por que os portes existem: a libc erra o último bit em parte dos casos.
    let v: Value = serde_json::from_str(include_str!("fixtures/js-math.json")).unwrap();
    let difere = v["ln"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| {
            c[1].as_f64()
                .is_some_and(|js| c[0].as_f64().unwrap().ln().to_bits() != js.to_bits())
        })
        .count();
    println!("ln da libc difere do V8 em {difere} casos");
}
