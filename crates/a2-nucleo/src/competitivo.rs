//! Eventos de PvP, Área de Treinamento, shiny do servidor e fichas de raid, tirados do WS do
//! jogo para o Nexus aprender o meta (equipes e casas que vencem em cada elo), testar
//! formações e medir o shiny do servidor inteiro. Nicks de outros jogadores não saem do app.

use serde_json::{json, Value};

fn s<'a>(v: &'a Value, k: &str) -> Option<&'a str> {
    v.get(k).and_then(Value::as_str)
}

/// Nick sem maiúsculas e sem letras repetidas em seguida: o rótulo da conta no app pode
/// diferir do nick do jogo só nisso (`Alllphha` × `Alllphhha`).
pub fn nick_simples(n: &str) -> String {
    let mut o = String::new();
    for c in n.to_lowercase().chars() {
        if !o.ends_with(c) {
            o.push(c);
        }
    }
    o
}

/// Pokémon de um ator do replay, sem o dono.
fn ator(a: &Value) -> Value {
    json!({
        "especie": a.get("e"), "nome": a.get("n"), "nivel": a.get("nv"),
        "hp": a.get("mhp"), "alcance": a.get("alc"), "casa": a.get("cs"),
        "shiny": a.get("sh"),
    })
}

/// `pvp` com `partida.replay`: a partida com os dois times e as casas iniciais.
fn partida(p: &Value, nick: &str, ts: i64) -> Option<Value> {
    let r = p.get("replay")?;
    let atores = r.get("atores")?.as_array()?;
    // O lado do dono é o do treinador (ator com `tr`) com o nick da conta.
    let lado = atores
        .iter()
        .find(|a| {
            a.get("tr").is_some()
                && s(a, "n").is_some_and(|n| nick_simples(n) == nick_simples(nick))
        })?
        .get("g")?
        .as_i64()?;
    let (mut meu, mut deles) = (Vec::new(), Vec::new());
    for a in atores
        .iter()
        .filter(|a| a.get("tr").is_none() && a.get("e").is_some())
    {
        if a.get("g").and_then(Value::as_i64) == Some(lado) {
            meu.push(ator(a));
        } else {
            deles.push(ator(a));
        }
    }
    Some(json!({
        "k": "pvp.partida", "ts": ts,
        "id": p.get("id"), "antes": p.get("antes"), "delta": p.get("delta"),
        "venci": p.get("venci"), "motivo": p.get("motivo"),
        "mapa": r.get("mapa"), "arena": r.get("slug"), "dur": r.get("dur"),
        "meu": meu, "oponente": deles,
    }))
}

/// `pvp` com `rank`/`time`/`casas`: o elo e a formação atual da conta.
fn estado_pvp(m: &Value, ts: i64) -> Option<Value> {
    let rank = m.get("rank")?;
    Some(json!({
        "k": "pvp.estado", "ts": ts,
        "tier": rank.get("tierId"), "divisao": rank.get("divisao"), "pontos": rank.get("pontos"),
        "posicao": rank.get("posicao"), "vitorias": rank.get("vitorias"),
        "derrotas": rank.get("derrotas"),
        "time": m.get("time"), "casas": m.get("casas"), "regras": m.get("regras"),
    }))
}

/// `treino` com `resultado`: o placar por lado (A = pokémon teste, B = os do dono).
fn treino(r: &Value, ts: i64) -> Option<Value> {
    let placar: Vec<Value> = r
        .get("placar")?
        .as_array()?
        .iter()
        .map(|l| {
            let pk: Vec<Value> = l
                .get("pokemons")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(|p| {
                    json!({"id": p.get("id"), "nome": p.get("nome"), "casa": p.get("casa"),
                           "alcance": p.get("alcance"), "dano": p.get("dano"),
                           "abates": p.get("abates"), "sobreviveu": p.get("sobreviveu")})
                })
                .collect();
            json!({"lado": l.get("id"), "dano": l.get("dano"), "abates": l.get("abates"),
                   "mortes": l.get("mortes"), "pokemons": pk})
        })
        .collect();
    Some(json!({
        "k": "treino.resultado", "ts": ts,
        "venceuA": r.get("venceuA"), "motivo": r.get("motivo"), "arena": r.get("arena"),
        "placar": placar,
    }))
}

/// Eventos destas telas a partir de uma mensagem do WS do jogo. `nick`: o da conta.
pub fn eventos(msg: &Value, nick: &str, ts: i64) -> Vec<Value> {
    let mut v = Vec::new();
    match s(msg, "t") {
        Some("pvp") => {
            v.extend(msg.get("partida").and_then(|p| partida(p, nick, ts)));
            v.extend(estado_pvp(msg, ts));
        }
        Some("treino") => v.extend(msg.get("resultado").and_then(|r| treino(r, ts))),
        Some("shiny.captura") => {
            if let Some(p) = msg.get("pokemon") {
                v.push(json!({
                    "k": "shiny.servidor", "ts": ts,
                    "pid": p.get("id"), "especie": p.get("speciesId"),
                    "nome": p.get("nome"), "nivel": p.get("level"),
                }));
            }
        }
        Some("raidFichas") => {
            if let Some(f) = msg.get("fichas").filter(|f| f.is_array()) {
                v.push(json!({"k": "boss.fichas", "ts": ts, "fichas": f}));
            }
        }
        _ => {}
    }
    v
}

/// Tipos que o coletor precisa repassar para estes eventos.
pub const TIPOS_WS: [&str; 4] = ["pvp", "treino", "shiny.captura", "raidFichas"];

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn partida_separa_os_lados_sem_nicks() {
        let m = json!({"t": "pvp", "partida": {"id": 7, "antes": 461, "delta": 16, "venci": true,
            "motivo": "wipe", "replay": {"mapa": "cerulean", "slug": "duelo", "dur": 66000, "atores": [
                {"e": 6, "n": "Charizard", "nv": 362, "mhp": 14112, "alc": 3, "cs": 1, "g": 0, "dono": "Outro"},
                {"e": 127, "n": "Pinsir", "nv": 100, "mhp": 4050, "alc": 1, "cs": 2, "g": 1, "dono": "Sikien"},
                {"g": 0, "n": "Outro", "tr": 1}, {"g": 1, "n": "Sikien", "tr": 1}]}}});
        let e = eventos(&m, "sikien", 5);
        assert_eq!(e.len(), 1);
        assert_eq!(e[0]["meu"][0]["nome"], "Pinsir");
        assert_eq!(e[0]["oponente"][0]["casa"], 1);
        assert!(!e[0].to_string().contains("Outro"));
        assert!(eventos(&m, "ninguem", 5).is_empty());
        assert_eq!(
            eventos(&m, "Siikien", 5).len(),
            1,
            "rótulo do app com letra repetida"
        );
    }

    #[test]
    fn estado_treino_shiny_e_fichas() {
        let pvp = json!({"t": "pvp", "rank": {"tierId": "prata", "pontos": 498}, "time": [1], "casas": {"1": 17}});
        assert_eq!(eventos(&pvp, "x", 1)[0]["tier"], "prata");
        let t = json!({"t": "treino", "resultado": {"venceuA": false, "motivo": "wipe",
            "placar": [{"id": 1, "dano": 9, "pokemons": [{"nome": "Ivysaur", "casa": 9}]}]}});
        assert_eq!(
            eventos(&t, "x", 1)[0]["placar"][0]["pokemons"][0]["casa"],
            9
        );
        let sh = json!({"t": "shiny.captura", "nick": "Bries", "pokemon": {"id": 3, "speciesId": 397, "nome": "Staravia", "level": 40}});
        let e = eventos(&sh, "x", 1);
        assert_eq!(e[0]["especie"], 397);
        assert!(!e[0].to_string().contains("Bries"));
        let f = json!({"t": "raidFichas", "fichas": [{"nome": "Melmetal"}]});
        assert_eq!(eventos(&f, "x", 1)[0]["k"], "boss.fichas");
    }
}
