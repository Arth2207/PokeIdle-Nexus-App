//! Roda um ciclo das regras do motor com arquivos: `D` (motor_estado), janela (do app ou do
//! legado) e pokédex. Imprime as decisões. Serve para validar a janela montada pelo app.
//!
//! `cargo run -p nucleo --example motor_janela -- dados.json janela.json pokedex.json`

use nucleo::combate::especie::Pokedex;
use nucleo::motor::laco::{self, ContextoCiclo};
use nucleo::motor::regras::{ContaViva, EstadoLaco};
use nucleo::motor::veredito::Especies;
use nucleo::motor::{config, Relogio};
use serde_json::Value;

fn ler(caminho: &str) -> Value {
    std::fs::read_to_string(caminho)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or(Value::Null)
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (Some(d), Some(j), Some(p)) = (a.first(), a.get(1), a.get(2)) else {
        eprintln!("uso: motor_janela dados.json janela.json pokedex.json");
        return;
    };
    let dados = config::carregar(&ler(d));
    let janela = ler(j);
    let bruta = ler(p);
    let precos = Especies::precos_de_json(&bruta);
    let Ok(dex) = Pokedex::de_valor(bruta) else {
        eprintln!("pokédex inválida");
        return;
    };
    let contas: Vec<ContaViva> = janela["contas"]
        .as_array()
        .map(|v| v.iter().filter_map(ContaViva::de_json).collect())
        .unwrap_or_default();
    let agora = janela["ciclo"]["t"].as_i64().unwrap_or(0);
    let r = Relogio {
        agora_ms: agora,
        fuso_ms: -3 * 3_600_000,
    };
    let mut laco_estado = EstadoLaco::default();
    let ctx = ContextoCiclo {
        dex: &dex,
        precos_npc: &precos,
    };
    let decisoes = laco::ciclo(&dados, &mut laco_estado, &r, &contas, &ctx);
    println!(
        "contas lidas: {} ({})",
        contas.len(),
        contas
            .iter()
            .map(|c| c.nick.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );
    for x in &decisoes {
        println!(
            "{} | {} | {} | {}",
            x.nick,
            x.regra,
            x.decisao.tipo(),
            x.dados().to_string().chars().take(160).collect::<String>()
        );
    }
    println!("decisões: {}", decisoes.len());
}
