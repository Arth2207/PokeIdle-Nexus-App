//! Repassa mensagens reais do jogo (JSON por linha: `{"conta", "em", "msg"}`) pelos fluxos do
//! relay e pelos eventos de PvP, e imprime o que sairia para o servidor. Não manda nada.
//!
//! `cargo run -p a2-nucleo --example replay < mensagens.jsonl`

use std::collections::BTreeMap;
use std::io::BufRead;

use a2_nucleo::relay::{Entrega, Relay};
use serde_json::{json, Value};

fn main() {
    let dono = std::env::args().nth(1).unwrap_or_else(|| "sikien".into());
    let mut r = Relay::padrao(true, &dono, false);
    let mut eventos: BTreeMap<String, usize> = BTreeMap::new();
    let mut saidas: Vec<Entrega> = Vec::new();
    let mut ultimo = 0;
    for linha in std::io::stdin().lock().lines().map_while(Result::ok) {
        let Ok(v) = serde_json::from_str::<Value>(&linha) else {
            continue;
        };
        let (conta, em, msg) = (
            v["conta"].as_str().unwrap_or_default().to_owned(),
            v["em"].as_i64().unwrap_or_default(),
            v["msg"].clone(),
        );
        ultimo = ultimo.max(em);
        for ev in a2_nucleo::competitivo::eventos(&msg, &conta, em) {
            *eventos
                .entry(ev["k"].as_str().unwrap_or("?").to_owned())
                .or_default() += 1;
        }
        r.receber_conta(&conta, &msg, em);
        if conta.eq_ignore_ascii_case(&dono) && msg["t"] == "market" {
            r.receber(&msg, em);
        }
        saidas.extend(r.entregar(em));
    }
    saidas.extend(r.entregar(ultimo + 3_600_000));
    let mut resumo: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for s in &saidas {
        let (rota, corpo) = s.requisicao();
        resumo.entry(rota.to_owned()).or_default().push(s.resumo());
        if rota == "/api/equipes/ingest" {
            let p = &corpo["pokemons"];
            println!(
                "equipe {}: {} pokémon, trLv {}, 1º = {}",
                corpo["nick"],
                p.as_array().map_or(0, Vec::len),
                corpo["trLv"],
                p[0]
            );
        }
        if rota == "/api/raros/ingest" {
            println!(
                "raros: {} itens; 1º = {}",
                corpo["raros"]["itens"].as_array().map_or(0, Vec::len),
                corpo["raros"]["itens"][0]
            );
        }
        if rota == "/api/motor/janela" {
            if let Ok(dir) = std::env::var("JANELA_SAIDA") {
                let conta = corpo["conta"].as_str().unwrap_or("x");
                let _ = std::fs::write(
                    format!("{dir}/janela-{conta}.json"),
                    corpo["janela"].to_string(),
                );
            }
        }
        if rota == "/ingest" {
            println!("diário: {}", corpo["eventos"][0]);
        }
    }
    println!(
        "{}",
        json!({"eventos_competitivos": eventos, "saidas": resumo})
    );
}
