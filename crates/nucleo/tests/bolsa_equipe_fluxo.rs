//! Fluxo completo da equipe da bolsa com o estado real das fixtures e ofertas sintéticas:
//! propriedades que valem para qualquer entrada (equipe válida, busca exata nunca pior que a
//! heurística, orçamento respeitado, híbrida nunca mais cara que comprar tudo).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::{HashMap, HashSet};

use nucleo::bolsa_equipe::{
    self as be, EntradaBolsa, EntradaHibrida, EntradaPropria, Faltas, OfertaEquipe,
};
use nucleo::combate::especie::Pokedex;
use nucleo::equipes::{EstadoEquipes, MotorEquipes};
use serde_json::{json, Value};

fn base() -> (MotorEquipes, EstadoEquipes) {
    let motor =
        MotorEquipes::novo(Pokedex::de_json(include_str!("fixtures/pokedex.json")).unwrap());
    #[derive(serde::Deserialize)]
    struct C {
        estado: EstadoEquipes,
    }
    #[derive(serde::Deserialize)]
    struct F {
        cenarios: Vec<C>,
    }
    let f: F = serde_json::from_str(include_str!("fixtures/equipes-legado.json")).unwrap();
    let estado = f.cenarios.into_iter().next().unwrap().estado;
    (motor, estado)
}

/// Ofertas sintéticas determinísticas: espécies compráveis em níveis de 0,6× a 3× o boss.
fn ofertas(motor: &MotorEquipes, nivel: f64, n: usize) -> Vec<OfertaEquipe> {
    let especies: Vec<i64> = motor
        .dex
        .especies()
        .iter()
        .filter(|s| s.de_pokebola() && !s.lendaria())
        .map(|s| s.id)
        .collect();
    let mut x: u32 = 12345;
    let mut rnd = || {
        x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        f64::from(x) / 4_294_967_296.0
    };
    (0..n)
        .map(|i| {
            let sp = especies[(rnd() * especies.len() as f64) as usize];
            let lvl = (nivel * (0.6 + rnd() * 2.4)).round();
            OfertaEquipe {
                id: 1_000 + i as i64,
                preco: (lvl * (5.0 + rnd() * 30.0)).round(),
                quantidade: 1.0,
                ficha: json!({ "speciesId": sp, "level": lvl, "quality": 1.0 + (rnd() * 8.0).round() / 10.0, "potencia": 1 + (rnd() * 4.0) as i64 }),
            }
        })
        .collect()
}

fn ids_validos(slots: &[Value]) {
    let ids: HashSet<String> = slots.iter().map(|s| s["ofertaId"].to_string()).collect();
    assert_eq!(ids.len(), slots.len(), "ids repetidos");
    let mut por_sp: HashMap<i64, usize> = HashMap::new();
    for s in slots {
        *por_sp.entry(s["speciesId"].as_i64().unwrap()).or_default() += 1;
    }
    assert!(por_sp.values().all(|&n| n <= 2), "mais de 2 por espécie");
}

#[test]
fn equipe_da_bolsa_respeita_as_regras() {
    let (motor, estado) = base();
    let rb = be::resolver_boss(&motor, &estado, &[], Some("articuno"), Some("conta-1")).unwrap();
    let ofs = ofertas(&motor, rb.boss.team_level, 120);
    let preco_de = |_: &Value, _: &Value| (Some(100.0), Some("último"));
    let cot = HashMap::new();
    for (orcamento, melhorias) in [
        (None, false),
        (Some(3_000.0), false),
        (Some(30_000.0), false),
        (None, true),
        (Some(30_000.0), true),
    ] {
        let e = EntradaBolsa {
            motor: &motor,
            boss: &rb,
            moeda: "gold",
            idade_min: 90.0,
            n: 5,
            ofertas: &ofs,
            cotacoes: &cot,
            listas: None,
            preco_de: &preco_de,
            agora: 0,
            orcamento,
            melhorias,
        };
        let r = be::equipe_bolsa(&e, &mut Faltas::default()).unwrap();
        if melhorias {
            let eco = &r["perfis"]["eco"];
            if eco["indisponivel"] == false {
                let ch = eco["chanceVitoria"].as_f64().unwrap();
                assert!((0.0..=1.0).contains(&ch));
                assert_eq!(eco["ordemCompra"].as_array().unwrap().len(), 5);
            }
        }
        for p in ["eco", "eq", "max"] {
            let perfil = &r["perfis"][p];
            if perfil["indisponivel"] == false {
                let slots = perfil["slots"].as_array().unwrap();
                assert_eq!(slots.len(), 5);
                ids_validos(slots);
            }
        }
        // A busca exata nunca perde para a heurística.
        let ex = &r["exata"];
        if !ex.is_null() {
            assert!(ex["D"].as_f64().unwrap() >= ex["heuristica"]["D"].as_f64().unwrap() - 1e-6);
        }
        if let (Some(b), false) = (orcamento, r["orcamento"]["indisponivel"] == true) {
            assert!(r["orcamento"]["total"].as_f64().unwrap() <= b + 1e-9);
            ids_validos(r["orcamento"]["slots"].as_array().unwrap());
        }
        let curva = r["curva"].as_array().unwrap();
        for par in curva.windows(2) {
            assert!(par[1]["poder"].as_f64() > par[0]["poder"].as_f64());
        }
    }
}

#[test]
fn hibrida_nunca_custa_mais_que_comprar_tudo() {
    let (motor, estado) = base();
    let rb = be::resolver_boss(&motor, &estado, &[], Some("articuno"), Some("conta-1")).unwrap();
    let ofs = ofertas(&motor, rb.boss.team_level, 80);
    let r = be::equipe_hibrida(&EntradaHibrida {
        motor: &motor,
        boss: &rb,
        moeda: "gold",
        ofertas: &ofs,
        n: 5,
        melhorias: false,
    })
    .unwrap();
    if r.get("indisponivel").is_none() {
        let compra = r["perfil"]["custoCompra"].as_f64().unwrap();
        if let Some(tudo) = r["custoComprarTudo"].as_f64() {
            assert!(
                compra <= tudo + 1e-6,
                "híbrida {compra} > comprar tudo {tudo}"
            );
        }
        assert_eq!(r["perfil"]["slots"].as_array().unwrap().len(), 5);
    }
}

#[test]
fn equipe_propria_monta_com_a_colecao() {
    let (motor, estado) = base();
    let rb = be::resolver_boss(&motor, &estado, &[], Some("articuno"), Some("conta-1")).unwrap();
    let preco_de = |_: &Value, _: &Value| (None, None);
    let valor = |_: &Value| None;
    let e = EntradaPropria {
        motor: &motor,
        estado: &estado,
        boss: &rb,
        nick: Some("conta-1"),
        valor_ficha: &valor,
        preco_de: &preco_de,
        eco_compra: None,
        agora: 0,
        melhorias: true,
    };
    let r = be::equipe_propria(&e).unwrap();
    assert!(r["perfis"].is_object());
    assert!(r["conta"]["avaliados"].as_u64().unwrap() > 0);
}
