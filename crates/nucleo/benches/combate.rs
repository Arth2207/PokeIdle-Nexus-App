//! Benchmarks do motor de combate: `cargo bench -p nucleo` (na VPS, com
//! `CARGO_BUILD_JOBS=1 nice -n 19`). Usam as fixtures dos testes de paridade.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::hint::black_box;

use criterion::{criterion_group, criterion_main, Criterion};
use nucleo::combate::combatente::{self, Perfil};
use nucleo::combate::especie::Pokedex;
use nucleo::equipes::MotorEquipes;
use nucleo::otimizador::{selecionar, Candidato, Opcoes};
use serde_json::Value;

fn motor() -> MotorEquipes {
    MotorEquipes::novo(Pokedex::de_json(include_str!("../tests/fixtures/pokedex.json")).unwrap())
}

/// O maior pool das fixtures do otimizador.
fn pool() -> (Vec<Candidato>, Opcoes) {
    let f: Value =
        serde_json::from_str(include_str!("../tests/fixtures/otimizador-legado.json")).unwrap();
    let caso = f["casos"]
        .as_array()
        .unwrap()
        .iter()
        .max_by_key(|c| c["candidatos"].as_array().unwrap().len())
        .unwrap();
    let c = caso["candidatos"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| Candidato {
            id: v["id"].as_str().unwrap().into(),
            sp: v["sp"].as_i64().unwrap(),
            lvl: v["lvl"].as_f64().unwrap(),
            unit: v["unit"].as_f64().unwrap(),
            justo: v.get("justo").and_then(Value::as_f64).unwrap_or(f64::NAN),
            hp: v["hp"].as_f64().unwrap(),
            dps_out: v["dpsOut"].as_f64().unwrap(),
            dps_in: v["dpsIn"].as_f64().unwrap(),
        })
        .collect();
    let o = &caso["opcoes"];
    let op = Opcoes {
        n: 5,
        nivel_boss: o["nivelBoss"].as_f64().unwrap(),
        referencia: o["referencia"].as_f64().unwrap(),
        teto_justo: true,
        moeda_orb: true,
        so_max: false,
        so_eco: false,
    };
    (c, op)
}

fn benches(cr: &mut Criterion) {
    let m = motor();
    let lista = m.lista_bosses(&[]);
    let boss = lista.iter().find(|b| b.key == "groudon").unwrap();
    let (foe, _) = m.modelo.boss(&boss.para_combate(), &m.dex).unwrap();
    let sp = m.dex.por_nome("Blastoise").unwrap();
    let carta = combatente::teorico(sp, boss.team_level, Perfil::Realista, m.modelo.constantes());
    cr.bench_function("duelo (uma carta × boss, 40 s simulados)", |b| {
        b.iter(|| m.modelo.avaliar(black_box(&carta), black_box(&foe), 1.0));
    });
    cr.bench_function("equipeIdeal (140 candidatas × boss)", |b| {
        b.iter(|| m.equipe_ideal(black_box(boss), Perfil::Realista, 6, None));
    });
    let (c, op) = pool();
    let nome = format!("otimizador ({} ofertas, 3 perfis)", c.len());
    cr.bench_function(&nome, |b| {
        b.iter(|| selecionar(black_box(&c), black_box(&op)))
    });
    let nome = format!("busca exata ({} ofertas)", c.len());
    cr.bench_function(&nome, |b| {
        b.iter(|| {
            nucleo::busca_exata::melhor_equipe(
                black_box(&c),
                5,
                op.nivel_boss,
                None,
                None,
                u64::MAX,
            )
        });
    });
}

criterion_group! {
    name = combate;
    config = Criterion::default().sample_size(20);
    targets = benches
}
criterion_main!(combate);
