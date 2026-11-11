//! Paridade das peças puras da equipe da bolsa (wiki de bosses, listas de alvos por perfil e
//! lucro) com o legado. Fixtures de `ferramentas/fixtures/bolsa-equipe-legado.cjs`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::HashMap;

use nucleo::bolsa_equipe::{avaliar_especies, listas_do_boss, lucro_perfil, parse_wiki};
use nucleo::combate::especie::Pokedex;
use nucleo::equipes::{BossLista, MotorEquipes};
use serde_json::{json, Value};

mod comum;
use comum::diferenca;

fn fixtures() -> Value {
    serde_json::from_str(include_str!("fixtures/bolsa-equipe-legado.json")).unwrap()
}

#[test]
fn wiki_igual_ao_legado() {
    let f = fixtures();
    let nossos: Vec<Value> = parse_wiki(f["secao"].as_str().unwrap())
        .into_iter()
        .map(|b| json!({ "nome": b.nome, "nivelMin": b.nivel, "nivelEquipe": b.nivel, "drops": b.drops, "entrada": b.entrada }))
        .collect();
    assert_eq!(diferenca(&Value::Array(nossos), &f["wiki"], ""), None);
}

#[test]
fn listas_iguais_ao_legado() {
    let f = fixtures();
    let motor =
        MotorEquipes::novo(Pokedex::de_json(include_str!("fixtures/pokedex.json")).unwrap());
    let mut erros = Vec::new();
    for caso in f["listas"].as_array().unwrap() {
        let b = &caso["boss"];
        let boss = BossLista {
            key: b["key"].as_str().unwrap().into(),
            nome: b["nome"].as_str().unwrap().into(),
            team_level: b["teamLevel"].as_f64().unwrap(),
            min_tr: b["teamLevel"].as_f64().unwrap(),
            regiao: None,
            trinca: None,
            pos: None,
            tipos: b["tipos"]
                .as_array()
                .unwrap()
                .iter()
                .map(|t| t.as_str().unwrap().to_string())
                .collect(),
            entrada: Value::Null,
            drops: Value::Null,
            fonte: "wiki",
        };
        let precos: HashMap<i64, f64> = caso["precos"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| (p[0].as_i64().unwrap(), p[1].as_f64().unwrap()))
            .collect();
        let ex = &caso["extras"];
        let extras =
            ["maxima", "equilibrada", "economica"].map(|k| ex[k].as_u64().unwrap() as usize);
        let a = avaliar_especies(&motor, &boss).unwrap();
        let l = listas_do_boss(&a, &precos, extras);
        let js = |v: &[nucleo::bolsa_equipe::AlvoEspecie]| {
            Value::Array(v.iter().map(|e| json!({ "speciesId": e.species_id, "nome": e.nome, "D": e.d, "preco": e.preco, "extra": e.extra })).collect())
        };
        let nosso =
            json!({ "maxima": js(&l[0]), "equilibrada": js(&l[1]), "economica": js(&l[2]) });
        if let Some(d) = diferenca(&nosso, &caso["saida"], "") {
            erros.push(format!("{}: {d}", boss.nome));
        }
    }
    assert!(erros.is_empty(), "{}", erros.join("\n"));
}

#[test]
fn lucro_igual_ao_legado() {
    let f = fixtures();
    let mut erros = Vec::new();
    for (i, caso) in f["lucros"].as_array().unwrap().iter().enumerate() {
        let tabela = caso["tabela"].clone();
        let preco_de = move |_: &Value, nome: &Value| {
            let t = nome.as_str().and_then(|n| tabela.get(n));
            (
                t.and_then(|t| t["preco"].as_f64()),
                t.and_then(|t| t["fonte"].as_str()).map(|s| match s {
                    "mediana 7d" => "mediana 7d",
                    "último" => "último",
                    _ => "menor anúncio",
                }),
            )
        };
        let b = &caso["boss"];
        let r = lucro_perfil(
            &caso["perfil"],
            caso["bossHp"].as_f64().unwrap(),
            &b["drops"],
            &b["entrada"],
            &preco_de,
        );
        if let Some(d) = diferenca(&r, &caso["saida"], "") {
            erros.push(format!("#{i}: {d}"));
        }
    }
    assert!(erros.is_empty(), "{}", erros.join("\n"));
}
