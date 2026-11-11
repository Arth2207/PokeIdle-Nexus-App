//! Paridade de `/api/equipes/bosses` e `/api/equipes/plano` com as rotas do legado.
//!
//! Fixtures de `ferramentas/fixtures/equipes-legado.cjs`, que chama as rotas do próprio
//! legado em memória. Números têm que sair idênticos; campo ausente equivale a `null`
//! (o `JSON.stringify` omite `undefined`).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use nucleo::combate::especie::Pokedex;
use nucleo::equipes::{resposta_bosses, resposta_plano, EstadoEquipes, MotorEquipes};
use serde_json::Value;

fn motor() -> MotorEquipes {
    MotorEquipes::novo(Pokedex::de_json(include_str!("fixtures/pokedex.json")).unwrap())
}

/// Primeira diferença entre dois JSONs.
fn diferenca(a: &Value, b: &Value, caminho: &str) -> Option<String> {
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

#[test]
fn rotas_de_equipes_iguais_ao_legado() {
    let m = motor();
    #[derive(serde::Deserialize)]
    struct Cenario {
        variante: String,
        estado: EstadoEquipes,
        chamadas: Vec<Value>,
    }
    #[derive(serde::Deserialize)]
    struct Fixtures {
        cenarios: Vec<Cenario>,
    }
    let f: Fixtures = serde_json::from_str(include_str!("fixtures/equipes-legado.json")).unwrap();
    let mut erros = Vec::new();
    let mut total = 0;
    for cenario in &f.cenarios {
        let estado = &cenario.estado;
        for c in &cenario.chamadas {
            total += 1;
            let q = &c["query"];
            let s = |k: &str| q.get(k).and_then(Value::as_str);
            let (status, resposta) = match c["rota"].as_str().unwrap() {
                "/api/equipes/bosses" => (200, resposta_bosses(&m, estado, s("nick"))),
                _ => match resposta_plano(&m, estado, s("nick"), s("boss"), s("perfil")) {
                    Ok(v) => (200, v),
                    Err(e) => (e.status, serde_json::json!({ "erro": e.erro })),
                },
            };
            let onde = format!("{} {} {}", cenario.variante, c["rota"], q);
            if u64::from(status) != c["status"].as_u64().unwrap() {
                erros.push(format!("{onde}: status {status} ≠ {}", c["status"]));
            } else if let Some(d) = diferenca(&resposta, &c["resposta"], "") {
                erros.push(format!("{onde}: {d}"));
            }
        }
    }
    assert!(
        erros.is_empty(),
        "{} de {total} divergem:\n{}",
        erros.len(),
        erros[..erros.len().min(15)].join("\n")
    );
}

#[test]
fn ingest_como_o_legado() {
    let mut e = EstadoEquipes::default();
    assert_eq!(
        e.ingerir(&serde_json::json!({ "nick": "" }), 1),
        Err("nick obrigatório")
    );
    let corpo = serde_json::json!({
        "nick": "Fulano", "trLv": "910", "vencidos": ["articuno", 5],
        "bosses": [{"key": "a"}, {"key": "b"}, {"key": "c"}],
        "pokemons": [{"id": 1, "level": 5, "extra": true}, null, 3, [1]],
    });
    let (nick, r) = e.ingerir(&corpo, 1000).unwrap();
    assert_eq!(nick, "fulano");
    assert_eq!(
        r,
        serde_json::json!({ "ok": true, "bosses": 3, "pokemons": 2 })
    );
    let c = e.conta(Some("FULANO")).unwrap();
    assert_eq!(c["trLv"], 910);
    assert_eq!(c["vencidos"], serde_json::json!(["articuno", "5"]));
    assert_eq!(
        c["pokemons"],
        serde_json::json!([{ "id": 1, "level": 5 }, {}])
    );
    assert_eq!(e.bosses_em, 1000);
    // Sem nick, a primeira conta.
    assert!(e.conta(None).is_some());
}
