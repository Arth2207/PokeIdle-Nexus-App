//! Regressão: cada caso em `testes/` é uma resposta gravada do legado (Node) para a entrada
//! `NN.entrada.json`. A saída do Rust tem de ser idêntica byte a byte (`NN.saida.json`,
//! `NN.status`). `catalogo.saida.json` é a resposta do catálogo.

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::*;

fn pasta() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src/dossie/calc/testes")
}

fn dados_teste() -> Dados {
    let pokedex =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../nucleo/tests/fixtures/pokedex.json");
    carregar_de(&pokedex, &pasta().join("evolucoes.json")).expect("dados de teste")
}

fn ler(p: &Path) -> String {
    std::fs::read_to_string(p).expect("fixture")
}

#[test]
fn catalogo_identico_ao_legado() {
    let d = dados_teste();
    let obtido = serde_json::to_string_pretty(&d.catalogo).expect("json");
    assert_eq!(obtido, ler(&pasta().join("catalogo.saida.json")));
}

#[test]
fn dossie_identico_ao_legado() {
    let d = dados_teste();
    let bosses: Vec<Value> =
        serde_json::from_str(&ler(&pasta().join("bosses.json"))).expect("bosses");
    let vivos = move || bosses.clone();
    let mut n = 0;
    for i in 0.. {
        let entrada = pasta().join(format!("{i:02}.entrada.json"));
        if !entrada.exists() {
            break;
        }
        let body: Value = serde_json::from_str(&ler(&entrada)).expect("entrada");
        let status: u16 = ler(&pasta().join(format!("{i:02}.status")))
            .trim()
            .parse()
            .expect("status");
        let esperado = ler(&pasta().join(format!("{i:02}.saida.json")));
        let (codigo, texto) = match calcular_com(&d, &body, &vivos) {
            Ok(v) => (200, serde_json::to_string_pretty(&v).expect("json")),
            Err(e) => (
                e.codigo,
                serde_json::to_string_pretty(&json!({ "erro": e.msg })).expect("json"),
            ),
        };
        assert_eq!(codigo, status, "caso {i}: status");
        assert!(texto == esperado, "caso {i}: corpo diferente do legado");
        n += 1;
    }
    assert!(n >= 40, "poucos casos: {n}");
}
