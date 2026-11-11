//! Paridade do otimizador (`selecionar`) com o `bolsa-equipe-selecao.js` do legado.
//!
//! Fixtures de `ferramentas/fixtures/otimizador-legado.cjs`: pools de cartas avaliadas contra
//! bosses reais. Mesma entrada, mesma equipe, números idênticos.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use nucleo::otimizador::{selecionar, Candidato, Opcoes, ResultadoPerfil, Selecao};
use serde_json::{json, Value};

mod comum;
use comum::diferenca;

fn candidato(v: &Value) -> Candidato {
    let f = |k: &str| v[k].as_f64().unwrap();
    Candidato {
        id: v["id"].as_str().unwrap().to_string(),
        sp: v["sp"].as_i64().unwrap(),
        lvl: f("lvl"),
        unit: f("unit"),
        justo: v.get("justo").and_then(Value::as_f64).unwrap_or(f64::NAN),
        hp: f("hp"),
        dps_out: f("dpsOut"),
        dps_in: f("dpsIn"),
    }
}

fn perfil(c: &[Candidato], p: &ResultadoPerfil) -> Value {
    let ids = |pick: &[usize]| pick.iter().map(|&i| c[i].id.clone()).collect::<Vec<_>>();
    let Some(e) = &p.equipe else {
        return json!({
            "indisponivel": true, "inviavel": true, "ids": [], "total": null, "poder": 0, "forca": 0, "mult": 1,
            "danos": [], "D": 0, "margemEstresse": 0, "heroiShare": 0, "abaixo": 0, "piorFaltam": 0,
            "tSobrevive": null, "restr": {}, "tetoUsado": null, "alternativas": [], "reservas": [],
        });
    };
    let r = p.restr.as_ref().unwrap();
    json!({
        "indisponivel": false, "inviavel": p.inviavel, "ids": ids(&e.pick), "total": e.total, "poder": p.poder,
        "forca": e.forca, "mult": e.mult, "danos": e.danos, "D": e.d, "margemEstresse": p.margem_estresse,
        "heroiShare": e.hero, "abaixo": p.abaixo, "piorFaltam": p.pior_faltam, "tSobrevive": p.t_sobrevive,
        "restr": { "piso": r.piso, "tetoNivel": r.teto_nivel, "mult": r.mult, "heroi": r.heroi, "estresse": r.estresse, "relaxadas": r.relaxadas },
        "tetoUsado": p.teto_usado,
        "alternativas": p.alternativas.iter().enumerate().map(|(i, a)| {
            let ae = a.equipe.as_ref().unwrap();
            json!({ "plano": if i == 0 { "B" } else { "C" }, "ids": ids(&ae.pick), "total": ae.total, "poder": a.poder, "mult": ae.mult, "margemEstresse": a.margem_estresse })
        }).collect::<Vec<_>>(),
        "reservas": p.reservas.iter().map(|l| l.iter().map(|x| json!({ "id": c[x.x].id, "margem": x.margem, "total": x.total })).collect::<Vec<_>>()).collect::<Vec<_>>(),
    })
}

fn projeta(c: &[Candidato], s: &Selecao) -> Value {
    json!({
        "perfis": { "eco": perfil(c, &s.eco), "eq": perfil(c, &s.eq), "max": perfil(c, &s.max) },
        "avisos": s.avisos, "elegiveis": s.elegiveis, "excluidasPreco": s.excluidas_preco, "semAvaliacao": s.sem_avaliacao,
    })
}

#[test]
fn otimizador_igual_ao_legado() {
    let f: Value = serde_json::from_str(include_str!("fixtures/otimizador-legado.json")).unwrap();
    let casos = f["casos"].as_array().unwrap();
    let mut erros = Vec::new();
    for (i, caso) in casos.iter().enumerate() {
        let c: Vec<Candidato> = caso["candidatos"]
            .as_array()
            .unwrap()
            .iter()
            .map(candidato)
            .collect();
        let o = &caso["opcoes"];
        let op = Opcoes {
            n: 5,
            nivel_boss: o["nivelBoss"].as_f64().unwrap(),
            referencia: o["referencia"].as_f64().unwrap(),
            teto_justo: o.get("tetoJusto").and_then(Value::as_bool).unwrap_or(true),
            moeda_orb: o.get("moeda").and_then(Value::as_str).unwrap_or("orb") == "orb",
            so_max: o.get("soMax").and_then(Value::as_bool).unwrap_or(false),
            so_eco: o.get("soEco").and_then(Value::as_bool).unwrap_or(false),
        };
        let s = selecionar(&c, &op);
        if let Some(d) = diferenca(&projeta(&c, &s), &caso["saida"], "") {
            erros.push(format!("caso {i} ({} cartas): {d}", c.len()));
        }
    }
    assert!(
        erros.is_empty(),
        "{} de {} divergem:\n{}",
        erros.len(),
        casos.len(),
        erros.join("\n")
    );
}

/// A seleção exata nunca é pior que a heurística: a Máxima tem dano ≥, e a Econômica, quando
/// as duas fecham com as regras completas no teto de 2,5×, custa ≤. Toda equipe exata
/// cumpre a meta de dano.
#[test]
fn selecao_exata_nunca_perde_para_a_heuristica() {
    use nucleo::otimizador::selecionar_exato;
    let f: Value = serde_json::from_str(include_str!("fixtures/otimizador-legado.json")).unwrap();
    for (i, caso) in f["casos"].as_array().unwrap().iter().enumerate() {
        let c: Vec<Candidato> = caso["candidatos"]
            .as_array()
            .unwrap()
            .iter()
            .map(candidato)
            .collect();
        let o = &caso["opcoes"];
        let op = Opcoes {
            n: 5,
            nivel_boss: o["nivelBoss"].as_f64().unwrap(),
            referencia: o["referencia"].as_f64().unwrap(),
            teto_justo: o.get("tetoJusto").and_then(Value::as_bool).unwrap_or(true),
            moeda_orb: o.get("moeda").and_then(Value::as_str).unwrap_or("orb") == "orb",
            so_max: o.get("soMax").and_then(Value::as_bool).unwrap_or(false),
            so_eco: o.get("soEco").and_then(Value::as_bool).unwrap_or(false),
        };
        let h = selecionar(&c, &op);
        let x = selecionar_exato(&c, &op, u64::MAX);
        if let (Some(he), Some(xe)) = (&h.max.equipe, &x.max.equipe) {
            assert!(
                xe.d >= he.d - 1e-6,
                "caso {i}: máxima exata {} < heurística {}",
                xe.d,
                he.d
            );
        }
        for (p, alvo) in [(&x.eco, 1.05), (&x.eq, x.eq.alvo.unwrap_or(1.05))] {
            if let Some(e) = &p.equipe {
                assert!(
                    e.d + 1e-6 >= alvo * op.referencia,
                    "caso {i}: equipe exata abaixo da meta"
                );
            }
        }
        let regras_completas = |p: &ResultadoPerfil| {
            p.teto_usado == Some(2.5) && p.restr.as_ref().is_some_and(|r| r.relaxadas.is_empty())
        };
        if regras_completas(&h.eco) && regras_completas(&x.eco) {
            let (ht, xt) = (
                h.eco.equipe.as_ref().unwrap().total,
                x.eco.equipe.as_ref().unwrap().total,
            );
            assert!(
                xt <= ht + 1e-6,
                "caso {i}: econômica exata {xt} > heurística {ht}"
            );
        }
    }
}
