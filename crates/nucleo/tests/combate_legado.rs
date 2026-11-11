//! Paridade do modelo de combate `Legado` com o JS do legado.
//!
//! As fixtures saem de `ferramentas/fixtures/combate-legado.cjs`, que roda o próprio
//! `sim.js`/`equipes-core.js`. Os floats têm que sair idênticos bit a bit.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use nucleo::combate::combatente::{self, Combatente, Opcoes, Perfil};
use nucleo::combate::duelo::{avaliar, penalidade, penalidade_individual, Eficacia};
use nucleo::combate::especie::{Categoria, Pokedex};
use nucleo::combate::modelo::{Boss, Legado, ModeloCombate};
use nucleo::combate::tipos::{efetividade_bruta, Constantes, Tipo};
use serde_json::Value;

const FIXTURES: &str = include_str!("fixtures/combate-legado.json");

struct Ctx {
    f: Value,
    dex: Pokedex,
    k: Constantes,
}

fn ctx() -> Ctx {
    let f: Value = serde_json::from_str(FIXTURES).unwrap();
    let dex = Pokedex::de_valor(f["especies"].clone()).unwrap();
    Ctx {
        f,
        dex,
        k: Constantes::wiki(),
    }
}

/// Acumula divergências para mostrar todas de uma vez.
#[derive(Default)]
struct Diffs(Vec<String>);

impl Diffs {
    fn num(&mut self, onde: &str, nosso: f64, js: &Value) {
        let esperado = js.as_f64().unwrap_or(f64::NAN);
        let ok = nosso.to_bits() == esperado.to_bits() || (nosso == 0.0 && esperado == 0.0);
        if !ok {
            self.0
                .push(format!("{onde}: nexus={nosso:?} js={esperado:?}"));
        }
    }
    fn igual<T: PartialEq + std::fmt::Debug>(&mut self, onde: &str, nosso: T, js: T) {
        if nosso != js {
            self.0.push(format!("{onde}: nexus={nosso:?} js={js:?}"));
        }
    }
    fn conferir(self, total: usize) {
        assert!(
            self.0.is_empty(),
            "{} divergências em {total} casos:\n{}",
            self.0.len(),
            self.0
                .iter()
                .take(20)
                .cloned()
                .collect::<Vec<_>>()
                .join("\n")
        );
    }
}

fn tipos(v: &Value) -> Vec<Tipo> {
    v.as_array()
        .map(|a| a.iter().filter_map(|t| t.as_str()?.parse().ok()).collect())
        .unwrap_or_default()
}

fn de_spec(c: &Ctx, s: &Value) -> Combatente {
    let sp = c.dex.por_id(s["d"].as_i64().unwrap()).unwrap();
    let iv: Vec<f64> = s["iv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_f64().unwrap())
        .collect();
    let op = Opcoes {
        qualidade: s["q"].as_f64().unwrap(),
        ivs: iv.try_into().unwrap(),
        potencia: u8::try_from(s["pot"].as_u64().unwrap()).unwrap(),
        shiny: s["shiny"].as_bool().unwrap(),
        selvagem: s["wild"].as_bool().unwrap(),
        lendario: s["legend"].as_bool(),
    };
    Combatente::novo(sp, s["lvl"].as_f64().unwrap(), &op, &c.k)
}

fn boss_de(v: &Value) -> Boss {
    Boss {
        key: v["key"].as_str().unwrap_or_default().to_string(),
        nome: v["nome"].as_str().unwrap().to_string(),
        nivel_equipe: v["teamLevel"].as_f64().unwrap(),
        tipos: tipos(&v["tipos"]),
    }
}

/// Compara um combatente com o `descreve` do gerador.
fn compara_mon(d: &mut Diffs, onde: &str, m: &Combatente, js: &Value) {
    let st = &js["st"];
    for (nome, v) in [
        ("hp", m.st.hp),
        ("atk", m.st.atk),
        ("def", m.st.def),
        ("spa", m.st.spa),
        ("spd", m.st.spd),
        ("spe", m.st.spe),
    ] {
        d.num(&format!("{onde}.st.{nome}"), v, &st[nome]);
    }
    d.num(&format!("{onde}.hp"), m.hp, &js["hp"]);
    d.num(&format!("{onde}.cdmul"), m.mult_recarga, &js["cdmul"]);
    d.num(&format!("{onde}.gap"), m.intervalo, &js["gap"]);
    d.num(&format!("{onde}.inv"), m.intervalo_investida, &js["inv"]);
    d.igual(
        &format!("{onde}.leg"),
        Some(m.lendario),
        js["leg"].as_bool(),
    );
    d.igual(
        &format!("{onde}.types"),
        m.tipos.clone(),
        tipos(&js["types"]),
    );
    let golpes: Vec<_> = m
        .golpes
        .iter()
        .map(|g| {
            (
                g.nome.clone(),
                g.poder,
                g.recarga,
                g.categoria,
                g.tipo.to_string(),
            )
        })
        .collect();
    let esperados: Vec<_> = js["moves"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| {
            let cat = if g[3] == "F" {
                Categoria::Fisico
            } else {
                Categoria::Especial
            };
            (
                g[0].as_str().unwrap().to_string(),
                g[1].as_f64().unwrap(),
                g[2].as_f64().unwrap(),
                cat,
                g[4].as_str().unwrap().to_string(),
            )
        })
        .collect();
    d.igual(&format!("{onde}.moves"), golpes, esperados);
}

fn compara_eficacia(d: &mut Diffs, onde: &str, e: Eficacia, js: &Value) {
    d.num(&format!("{onde}.e"), e.mult, &js["e"]);
    d.igual(
        &format!("{onde}.t"),
        e.tipo.map(|t| t.to_string()),
        js["t"].as_str().map(str::to_string),
    );
}

#[test]
fn efetividade_igual_ao_js() {
    let c = ctx();
    let casos = c.f["efetividade"].as_array().unwrap();
    let mut d = Diffs::default();
    for (i, x) in casos.iter().enumerate() {
        let mt: Tipo = x[0].as_str().unwrap().parse().unwrap();
        let def = tipos(&x[1]);
        d.num(&format!("#{i} raw"), efetividade_bruta(mt, &def), &x[2]);
        d.num(&format!("#{i} hunt"), c.k.efetividade_hunt(mt, &def), &x[3]);
    }
    d.conferir(casos.len());
}

#[test]
fn combatentes_iguais_ao_js() {
    let c = ctx();
    let casos = c.f["combatentes"].as_array().unwrap();
    let mut d = Diffs::default();
    for (i, x) in casos.iter().enumerate() {
        compara_mon(
            &mut d,
            &format!("#{i}"),
            &de_spec(&c, &x["spec"]),
            &x["mon"],
        );
    }
    d.conferir(casos.len());
}

#[test]
fn danos_iguais_ao_js() {
    let c = ctx();
    let casos = c.f["danos"].as_array().unwrap();
    let mut d = Diffs::default();
    for (i, x) in casos.iter().enumerate() {
        let (a, b) = (de_spec(&c, &x["att"]), de_spec(&c, &x["def"]));
        let esperados = x["golpes"].as_array().unwrap();
        d.igual(&format!("#{i} qtd golpes"), a.golpes.len(), esperados.len());
        for (j, (g, e)) in a.golpes.iter().zip(esperados).enumerate() {
            let v = combatente::dano(&a, &b, g.poder, g.categoria, g.tipo, a.stab(g.tipo), &c.k);
            d.num(&format!("#{i} golpe {j}"), v, e);
        }
        d.num(
            &format!("#{i} investida"),
            combatente::dano_investida(&a, &b, &c.k),
            &x["investida"],
        );
    }
    d.conferir(casos.len());
}

#[test]
fn bosses_iguais_ao_js() {
    let c = ctx();
    let modelo = Legado::novo();
    let casos = c.f["bosses"].as_array().unwrap();
    let mut d = Diffs::default();
    for x in casos {
        let b = boss_de(&x["boss"]);
        match (modelo.boss(&b, &c.dex), x["mon"].is_null()) {
            (Some((m, _)), false) => compara_mon(&mut d, &b.nome, &m, &x["mon"]),
            (None, true) => {}
            (nosso, _) => d.0.push(format!(
                "{}: nexus={} js={}",
                b.nome,
                nosso.is_some(),
                x["mon"]
            )),
        }
    }
    d.conferir(casos.len());
}

#[test]
fn fichas_iguais_ao_js() {
    let c = ctx();
    let casos = c.f["fichas"].as_array().unwrap();
    let mut d = Diffs::default();
    for (i, x) in casos.iter().enumerate() {
        let r = combatente::de_ficha(&c.dex, &x["ficha"], &c.k);
        d.igual(
            &format!("#{i} espécie"),
            r.as_ref().map(|(_, sp)| sp.id),
            x["especie"].as_i64(),
        );
        if let (Some((m, _)), false) = (&r, x["mon"].is_null()) {
            compara_mon(&mut d, &format!("#{i}"), m, &x["mon"]);
        }
    }
    d.conferir(casos.len());
}

#[test]
fn duelos_iguais_ao_js() {
    let c = ctx();
    let modelo = Legado::novo();
    let casos = c.f["duelos"].as_array().unwrap();
    let mut d = Diffs::default();
    for (i, x) in casos.iter().enumerate() {
        let (boss, _) = modelo.boss(&boss_de(&x["boss"]), &c.dex).unwrap();
        let me = &x["me"];
        let carta = match me["tipo"].as_str().unwrap() {
            "teorico" => {
                let sp = c.dex.por_id(me["d"].as_i64().unwrap()).unwrap();
                let perfil = if me["perfil"] == "ideal" {
                    Perfil::Ideal
                } else {
                    Perfil::Realista
                };
                combatente::teorico(sp, me["lvl"].as_f64().unwrap(), perfil, &c.k)
            }
            "spec" => de_spec(&c, &me["spec"]),
            _ => combatente::de_ficha(&c.dex, &me["ficha"], &c.k).unwrap().0,
        };
        let r = modelo.avaliar(&carta, &boss, x["mult"].as_f64().unwrap());
        let s = &x["saida"];
        let onde = format!("#{i} {}", x["boss"]["nome"]);
        d.num(&format!("{onde} D"), r.d, &s["D"]);
        d.num(&format!("{onde} dpsOut"), r.dps_causado, &s["dpsOut"]);
        d.num(&format!("{onde} dpsIn"), r.dps_recebido, &s["dpsIn"]);
        d.num(&format!("{onde} tDie"), r.tempo_ate_cair, &s["tDie"]);
        compara_eficacia(&mut d, &format!("{onde} bate"), r.bate, &s["bate"]);
        compara_eficacia(&mut d, &format!("{onde} apanha"), r.apanha, &s["apanha"]);
        d.igual(
            &format!("{onde} imune"),
            Some(r.imune),
            s["imune"].as_bool(),
        );
        // A função livre e a do trait são a mesma coisa no modelo legado.
        assert_eq!(r, avaliar(&carta, &boss, x["mult"].as_f64().unwrap(), &c.k));
    }
    d.conferir(casos.len());
}

#[test]
fn penalidades_iguais_ao_js() {
    let c = ctx();
    let casos = c.f["penalidades"].as_array().unwrap();
    let mut d = Diffs::default();
    for (i, x) in casos.iter().enumerate() {
        let niveis: Vec<f64> = x["niveis"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n.as_f64().unwrap())
            .collect();
        let team = x["team"].as_f64().unwrap();
        let (forca, mult) = penalidade(&niveis, team);
        d.num(&format!("#{i} força"), forca, &x["forca"]);
        d.num(&format!("#{i} mult"), mult, &x["mult"]);
        d.num(
            &format!("#{i} individual"),
            penalidade_individual(x["lvl"].as_f64().unwrap(), team),
            &x["individual"],
        );
    }
    d.conferir(casos.len());
}

#[test]
fn pow_igual_ao_v8() {
    let c = ctx();
    let casos = c.f["potencias"].as_array().unwrap();
    let mut d = Diffs::default();
    for (i, x) in casos.iter().enumerate() {
        let (b, e) = (x[0].as_f64().unwrap(), x[1].as_f64().unwrap());
        let nosso = nucleo::js::pow(b, e);
        // O JSON.stringify grava Infinity/NaN como null.
        match x[2].as_f64() {
            Some(_) => d.num(&format!("#{i} {b}^{e}"), nosso, &x[2]),
            None => d.igual(&format!("#{i} {b}^{e} finito"), nosso.is_finite(), false),
        }
    }
    d.conferir(casos.len());
}
