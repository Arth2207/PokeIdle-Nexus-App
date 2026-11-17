//! Replay somente de leitura: hunt_planejar dados.json janelas.json pokedex.json.
//! Não conecta ao jogo, não grava configuração e não enfileira comandos.
use nucleo::combate::especie::Pokedex;
use nucleo::motor::{
    config,
    regras::{
        hunt::{self, ContextoHunt, Precos},
        ContaViva,
    },
    Relogio,
};
use serde_json::{json, Value};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if a.len() != 3 {
        return Err("uso: hunt_planejar dados.json janelas.json pokedex.json".into());
    }
    let d: Value = serde_json::from_slice(&std::fs::read(&a[0])?)?;
    let j: Value = serde_json::from_slice(&std::fs::read(&a[1])?)?;
    let dex = Pokedex::de_json(&std::fs::read_to_string(&a[2])?)?;
    let r = Relogio {
        agora_ms: j["agora"].as_i64().ok_or("falta agora")?,
        fuso_ms: -3 * 3600000,
    };
    let d = config::carregar(&d);
    let mut resultados = Vec::new();
    for v in j["contas"].as_array().ok_or("faltam contas")? {
        let c = ContaViva::de_json(v).ok_or("conta inválida")?;
        let k = hunt::constantes_da_conta(&c);
        let cfg = config::efetiva(&d, &c.nick);
        let pot = cfg["compra"]["pocaoId"].as_i64();
        let ball = cfg["compra"]["bolaId"].as_i64();
        let preco = |lista: &str, id: Option<i64>| {
            c.welcome[lista]
                .as_array()
                .and_then(|a| a.iter().find(|v| v["id"].as_i64() == id))
                .and_then(|v| v["preco"].as_f64().or(v["price"].as_f64()))
                .unwrap_or(0.0)
        };
        let ctx = ContextoHunt {
            dex: &dex,
            k: &k,
            precos: Precos {
                pot: preco("itens", pot),
                ball: preco("bolas", ball),
            },
            limite_rodizio: None,
            equipe_no_comando: false,
            rodizio_volta: false,
            hunt_desde: c.hunt_desde as f64,
        };
        let inicio = std::time::Instant::now();
        let plano = hunt::plano_hunt(&d, &r, &c, &ctx);
        resultados.push(
            json!({"conta":c.nick,"ms":inicio.elapsed().as_millis(),"plano":plano,
            "progressao":nucleo::motor::progressao::prever(&c,r.agora_ms)}),
        );
    }
    println!("{}", serde_json::to_string_pretty(&resultados)?);
    Ok(())
}
