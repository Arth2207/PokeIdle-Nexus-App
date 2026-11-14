//! Janela da conta para o motor do Nexus (Fase 4): o mesmo objeto que o motor do legado
//! entrega em `/api/nexus/janela` (`ultimo`, `welcome`, `pub`, `huntKey`, `linhas`...), montado
//! com o que o app lê do WebSocket do jogo em vez da tela. Os textos seguem o formato da tela
//! (`Nv 1017`, `Q 1,600`, `26279 / 26436`) porque as regras do motor leem assim.

use std::collections::VecDeque;

use serde_json::{json, Map, Value};

use super::conta::{bosses_do_welcome, Conta};

/// Linhas do HUD que vão na janela (o legado manda as últimas 500).
pub const LINHAS_JANELA: usize = 500;

/// `1234567` → `1.234.567`.
pub fn milhar(x: f64) -> String {
    let n = x.round() as i64;
    let s = n.unsigned_abs().to_string();
    let mut o = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            o.push('.');
        }
        o.push(c);
    }
    if n < 0 {
        format!("-{o}")
    } else {
        o
    }
}

/// Poções e reanimações básicas: o login não traz, o motor do legado completa com o
/// `/assets/items.json` do site (id, nome, categoria, preço, cura).
const ITENS_BASE: [(i64, &str, &str, i64, i64); 7] = [
    (200, "Small Potion", "heal", 50, 60),
    (201, "Great Potion", "heal", 150, 150),
    (202, "Ultra Potion", "heal", 400, 400),
    (203, "Hyper Potion", "heal", 800, 1000),
    (204, "Ultimate Potion", "heal", 1500, 3000),
    (205, "Revive", "revive", 600, 0),
    (206, "Max Revive", "revive", 2500, 0),
];

/// `a / b xp` com o progresso no nível, como a tela mostra.
fn xp_txt(xp: f64, ini: f64, fim: f64) -> Option<String> {
    (fim > ini).then(|| format!("{} / {} xp", milhar(xp - ini), milhar(fim - ini)))
}

/// `1.6` → `1,600`.
fn decimal3(x: f64) -> String {
    format!("{x:.3}").replace('.', ",")
}

fn n(v: &Value, k: &str) -> f64 {
    v.get(k).and_then(Value::as_f64).unwrap_or(0.0)
}

fn iv_total(p: &Value) -> f64 {
    p.get("ivTotal").and_then(Value::as_f64).unwrap_or_else(|| {
        p.get("ivs")
            .and_then(Value::as_object)
            .map_or(0.0, |m| m.values().filter_map(Value::as_f64).sum())
    })
}

/// Login no formato do motor (`W.__hdxWelcome`).
pub fn welcome_motor(w: &Value) -> Value {
    let lista = |k: &str| {
        w.get(k)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
    };
    let hunts: Vec<Value> = lista("hunts")
        .iter()
        .map(|h| {
            let raw: Vec<Value> = h
                .as_object()
                .map(|m| m.keys().take(12).map(|k| json!(k)).collect())
                .unwrap_or_default();
            json!({
                "slug": h.get("slug"),
                "nome": h.get("nome").or(h.get("name")).cloned().unwrap_or(json!("")),
                "nivel": h.get("nivel").or(h.get("level")),
                "area": h.get("area"),
                "raw": raw,
            })
        })
        .collect();
    let bolas: Vec<Value> = lista("catalogoBolas")
        .iter()
        .map(|b| {
            json!({"id": b.get("id"), "nome": b.get("nome").or(b.get("name")), "preco": b.get("priceGold"),
                   "compravel": b.get("compravel").and_then(Value::as_bool).unwrap_or(false),
                   "inf": b.get("infinite").and_then(Value::as_bool).unwrap_or(false)})
        })
        .collect();
    let mut itens: Vec<Value> = lista("itensNossos")
        .iter()
        .filter(|i| i.get("npcPrice").is_some() && i.get("category").is_some())
        .map(|i| {
            json!({"id": i.get("id"), "nome": i.get("name").or(i.get("nome")), "cat": i.get("category"),
                   "preco": i.get("npcPrice"), "healPct": n(i, "healPct"), "healAmount": n(i, "healAmount")})
        })
        .collect();
    for (id, nome, cat, preco, cura) in ITENS_BASE {
        if !itens
            .iter()
            .any(|i| i.get("id").and_then(Value::as_i64) == Some(id))
        {
            itens.push(json!({"id": id, "nome": nome, "cat": cat, "preco": preco, "healPct": 0, "healAmount": cura}));
        }
    }
    json!({
        "hunts": hunts, "bolas": bolas, "itens": itens,
        "bosses": bosses_do_welcome(w).unwrap_or_else(|| json!([])),
        "pedrasEvolucao": w.get("pedrasEvolucao"), "refino": w.get("refino"),
        "itensTipos": w.get("itensTipos"),
        "chanceShiny": w.get("chanceShiny").and_then(Value::as_f64),
    })
}

/// Dados públicos do jogo que o motor usa (`conta.pub`).
pub fn publico(w: &Value) -> Value {
    json!({"multHpSelvagem": w.get("multHpSelvagem"), "multDanoSelvagem": w.get("multDanoSelvagem"),
           "ampliacaoHunt": w.get("ampliacaoHunt")})
}

/// Pokémon no formato do motor (`IO.mons`).
fn mon(p: &Value) -> Value {
    let prot = p.get("heldItemId").is_some_and(|h| !h.is_null())
        || p.get("starter").and_then(Value::as_bool) == Some(true)
        || p.get("tmElemental").is_some_and(|t| !t.is_null())
        || p.get("tmAoe").and_then(Value::as_bool) == Some(true)
        || n(p, "refinoTotal") > 0.0;
    json!({
        "id": p.get("id"), "speciesId": p.get("speciesId"), "level": p.get("level"), "ivs": p.get("ivs"),
        "quality": p.get("quality").or(p.get("qualidade")), "potencia": p.get("potencia"),
        "shiny": p.get("shiny"), "refino": p.get("refino"),
        "tmElemental": p.get("tmElemental").cloned().unwrap_or(Value::Null),
        "tmAoe": i32::from(p.get("tmAoe").and_then(Value::as_bool) == Some(true)),
        "heldItemId": p.get("heldItemId").cloned().unwrap_or(Value::Null),
        "slot": p.get("slot").cloned().unwrap_or(Value::Null),
        "starter": i32::from(p.get("starter").and_then(Value::as_bool) == Some(true)),
        "prot": i32::from(prot),
    })
}

/// O que a janela de uma conta precisa além da [`Conta`].
pub struct Contexto<'a> {
    pub id: &'a str,
    pub welcome: &'a Value,
    pub publico: &'a Value,
    pub linhas: &'a VecDeque<Value>,
    pub medias7: Option<&'a Value>,
    pub medias1: Option<&'a Value>,
    pub sessao_inicio: i64,
    pub hunt_desde: i64,
    pub ouro_max: f64,
    pub agora: i64,
}

fn nome_item(welcome: &Value, id: i64) -> String {
    ["itens", "bolas"]
        .iter()
        .filter_map(|k| welcome.get(*k)?.as_array())
        .flatten()
        .find(|i| i.get("id").and_then(Value::as_i64) == Some(id))
        .and_then(|i| i.get("nome")?.as_str())
        .map_or_else(|| format!("#{id}"), str::to_owned)
}

/// A janela da conta, como um item de `contas` do `/api/nexus/janela`.
pub fn montar(c: &Conta, x: &Contexto) -> Value {
    let ativo = c.ativo.and_then(|id| c.pokemons.get(&id));
    let a = &c.automacao;
    let ids = |k: &str| -> Vec<i64> {
        a.get(k)
            .and_then(Value::as_array)
            .map(|v| v.iter().filter_map(Value::as_i64).collect())
            .unwrap_or_default()
    };
    let bolas_on = ids("ballIds");
    let bolas: Vec<Value> = x
        .welcome
        .get("bolas")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|b| {
            let id = b.get("id")?.as_i64()?;
            Some(
                json!({"nome": b.get("nome"), "q": milhar(*c.bolas.get(&id).unwrap_or(&0) as f64),
                        "on": bolas_on.contains(&id)}),
            )
        })
        .collect();
    let pocoes: Vec<Value> = ids("potionIds")
        .into_iter()
        .chain(ids("reviveIds"))
        .map(|id| json!({"nome": nome_item(x.welcome, id), "q": milhar(*c.itens.get(&id).unwrap_or(&0) as f64), "on": true}))
        .collect();
    let mut time: Vec<&Value> = c
        .pokemons
        .values()
        .filter(|p| p.get("slot").is_some_and(|s| !s.is_null()))
        .collect();
    time.sort_by_key(|p| p.get("slot").and_then(Value::as_i64).unwrap_or(99));
    let equipe: Vec<Value> = time
        .iter()
        .map(|p| json!({"nome": p.get("nome"), "lv": format!("Nv{}", n(p, "level")),
                        "q": format!("Q {}", decimal3(n(p, "quality"))), "iv": format!("IV {}", iv_total(p))}))
        .collect();
    let hunt = x
        .welcome
        .get("hunts")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|h| h.get("slug").and_then(Value::as_str) == c.hunt.as_deref());
    let (hunt_txt, hunt_key) = match hunt {
        Some(h) => {
            let mut nome = h
                .get("nome")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned();
            // Na Outland a tela mostra o tier escolhido: `Furious Scizor · Outland 2`.
            if let (Some("outland"), Some(t)) =
                (h.get("area").and_then(Value::as_str), c.tier_outland)
            {
                nome = format!("{nome} · Outland {t}");
            }
            let nivel = n(h, "nivel");
            (format!("{nome} · nível {nivel}"), format!("{nome}|{nivel}"))
        }
        None => (String::new(), String::new()),
    };
    let seq = x
        .linhas
        .back()
        .and_then(|l| l.get(0))
        .cloned()
        .unwrap_or(json!(0));
    let mut player = Map::new();
    player.insert("level".into(), json!(c.level));
    player.insert("activeId".into(), json!(c.ativo));
    player.insert("huntSlug".into(), json!(c.hunt));
    player.insert("guildBonusPct".into(), json!(c.bonus_guild));
    let vit: Map<String, Value> = c.vitorias.iter().map(|k| (k.clone(), json!(1))).collect();
    player.insert("bossVit".into(), Value::Object(vit));
    player.insert("travados".into(), json!(ids("pokemonTravado")));
    let ultimo = json!({
        "t": x.agora, "nick": c.nick, "trLv": c.level.map(|l| l.to_string()),
        "trXp": xp_txt(c.xp.0, c.xp.1, c.xp.2),
        "gold": c.ouro.map(milhar),
        "ativo": ativo.map(|p| json!({
            "nome": p.get("nome"), "lv": format!("Nv {}", n(p, "level")),
            "pot": format!("P{}", n(p, "potencia")), "iv": format!("IV {}", iv_total(p)),
            "q": format!("Q {}", decimal3(n(p, "quality"))), "nota": format!("N {}", decimal3(n(p, "nota"))),
            "hp": format!("{} / {}", n(p, "hp"), n(p, "maxHp")),
            "xp": xp_txt(n(p, "xp"), n(p, "xpNivel"), n(p, "xpProximo")),
        })),
        "hunt": hunt_txt, "dia": c.dia_tipo,
        "limiar": a.get("hpLimiar").and_then(Value::as_f64).map(|l| ((l * 100.0).round() as i64).to_string()),
        "bolas": bolas, "pocoes": pocoes, "equipe": equipe, "eco": "", "ecoTempo": null,
        "io": {"player": player, "active": ativo.map(mon)},
        "sessaoTomada": false, "seq": seq,
        "hdx": {
            "ws": true, "welcome": {"ok": 1},
            "eu": {"balls": c.bolas, "items": c.itens, "automation": a},
            "medias": x.medias7, "medias1": x.medias1,
            "pokemons": c.pokemons.values().map(mon).collect::<Vec<_>>(),
            "mercado": [], "precosMercado": null, "vendas": [], "ofertas": [],
        },
    });
    let estado = json!({"sessaoTomada": false, "huntDesde": x.hunt_desde, "sessaoInicio": x.sessao_inicio,
                        "venderBusy": false, "ouroMax": x.ouro_max, "seq": seq});
    json!({
        "id": x.id, "nome": c.nick, "nick": c.nick, "ultimo": ultimo, "welcome": x.welcome, "pub": x.publico,
        "huntKey": hunt_key, "sessaoInicio": x.sessao_inicio, "linhas": x.linhas,
        "venderBusy": false, "venderUltimo": null,
        "antes": {"estado": estado, "ouroMax": x.ouro_max, "huntDesde": x.hunt_desde, "huntKey": hunt_key},
        "fonte": "a2",
    })
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn textos_da_tela_e_formato_do_motor() {
        assert_eq!(milhar(324_629_612.0), "324.629.612");
        assert_eq!(decimal3(1.6), "1,600");
        let w = json!({"hunts": [{"slug": "furious_scizor", "nome": "Furious Scizor", "nivel": 150, "area": "outland"}],
            "catalogoBolas": [{"id": 1, "name": "Poke Ball", "priceGold": 5, "compravel": true}],
            "itensNossos": [{"id": 70070, "name": "Golden Potion", "category": "heal", "npcPrice": 10}],
            "multHpSelvagem": 5});
        let wm = welcome_motor(&w);
        assert_eq!(wm["hunts"][0]["raw"].as_array().map(Vec::len), Some(4));
        assert_eq!(wm["bolas"][0]["nome"], "Poke Ball");
        let mut c = Conta::default();
        c.aplicar(&json!({"t": "welcome", "estado": {"nick": "Sikien", "level": 1019, "huntSlug": "furious_scizor",
            "outlandTier": 2, "xp": 17_638_563_168u64, "xpNivel": 17_634_916_000u64, "xpProximo": 17_686_885_100u64,
            "gold": 324_629_612, "activeId": 1, "balls": {"1": 4327}, "items": {"70070": 293},
            "automation": {"ballIds": [1], "potionIds": [70070], "hpLimiar": 0.5, "pokemonTravado": [1]},
            "diaSazonal": {"tipo": "GHOST"},
            "pokemons": [{"id": 1, "speciesId": 6, "nome": "Charizard", "level": 1017, "quality": 1.6, "potencia": 2,
                          "nota": 3.704, "hp": 26279, "maxHp": 26436, "slot": 0, "starter": true,
                          "ivs": {"hp": 27, "atk": 27, "def": 27, "spAtk": 27, "spDef": 27, "speed": 27}}]}}));
        let linhas: VecDeque<Value> = VecDeque::from(vec![json!([68156, 5, "x"])]);
        let p = publico(&w);
        let j = montar(
            &c,
            &Contexto {
                id: "sikien",
                welcome: &wm,
                publico: &p,
                linhas: &linhas,
                medias7: None,
                medias1: None,
                sessao_inicio: 1,
                hunt_desde: 2,
                ouro_max: 3.0,
                agora: 9,
            },
        );
        let u = &j["ultimo"];
        assert_eq!(
            u["ativo"],
            json!({"nome": "Charizard", "lv": "Nv 1017", "pot": "P2", "iv": "IV 162",
            "q": "Q 1,600", "nota": "N 3,704", "hp": "26279 / 26436", "xp": null})
        );
        assert_eq!(
            (
                u["trLv"].clone(),
                u["gold"].clone(),
                u["limiar"].clone(),
                u["dia"].clone()
            ),
            (
                json!("1019"),
                json!("324.629.612"),
                json!("50"),
                json!("GHOST")
            )
        );
        assert_eq!(
            u["bolas"][0],
            json!({"nome": "Poke Ball", "q": "4.327", "on": true})
        );
        assert_eq!(
            u["pocoes"][0],
            json!({"nome": "Golden Potion", "q": "293", "on": true})
        );
        assert_eq!(u["equipe"][0]["lv"], "Nv1017");
        assert_eq!(u["hunt"], "Furious Scizor · Outland 2 · nível 150");
        assert_eq!(j["huntKey"], "Furious Scizor · Outland 2|150");
        assert_eq!(u["trXp"], "3.647.168 / 51.969.100 xp");
        assert_eq!(
            wm["itens"].as_array().map(Vec::len),
            Some(8),
            "1 do login + 7 básicos"
        );
        assert_eq!(u["io"]["player"]["travados"], json!([1]));
        assert_eq!(u["hdx"]["pokemons"][0]["prot"], 1);
        assert_eq!(u["seq"], 68156);
        assert_eq!(j["pub"]["multHpSelvagem"], 5);
        // O motor lê a janela com o mesmo parser do legado.
        assert_eq!(j["nick"], "Sikien");
    }
}

/// Janela a cada 30 s (o ciclo do motor); o login vai junto só na 1ª vez e a cada 30 min.
pub const JANELA_MS: i64 = 30_000;
pub const LOGIN_MS: i64 = 30 * 60_000;

/// O que o app guarda de cada conta para montar a janela do motor.
#[derive(Default)]
pub struct Janelas {
    login: std::collections::HashMap<String, Value>,
    publico: Value,
    linhas: std::collections::HashMap<String, VecDeque<Value>>,
    /// conta → (hunt, desde quando).
    hunt: std::collections::HashMap<String, (Option<String>, i64)>,
    sessao: std::collections::HashMap<String, i64>,
    ouro_max: std::collections::HashMap<String, f64>,
    enviada: std::collections::HashMap<String, i64>,
    login_enviado: std::collections::HashMap<String, i64>,
    medias7: Option<Value>,
    medias1: Option<Value>,
}

impl Janelas {
    pub fn login(&mut self, conta: &str, w: &Value, agora: i64) {
        self.login.insert(conta.to_owned(), welcome_motor(w));
        self.publico = publico(w);
        self.sessao.insert(conta.to_owned(), agora);
        self.login_enviado.remove(conta);
    }

    /// Depois de aplicar `welcome`/`estado`: marca troca de hunt e o maior ouro visto.
    pub fn estado(&mut self, conta: &str, c: &Conta, agora: i64) {
        let h = self
            .hunt
            .entry(conta.to_owned())
            .or_insert((c.hunt.clone(), agora));
        if h.0 != c.hunt {
            *h = (c.hunt.clone(), agora);
            self.sessao.insert(conta.to_owned(), agora);
        }
        if let Some(g) = c.ouro {
            let m = self.ouro_max.entry(conta.to_owned()).or_insert(g);
            *m = m.max(g);
        }
    }

    pub fn hud(&mut self, conta: &str, linhas: &[Value]) {
        let l = self.linhas.entry(conta.to_owned()).or_default();
        l.extend(linhas.iter().cloned());
        while l.len() > LINHAS_JANELA {
            l.pop_front();
        }
    }

    /// Resposta `market` aba `medias` (como o motor guarda: `{t, dias, itens}`).
    pub fn medias(&mut self, msg: &Value, agora: i64) {
        let dias = msg.get("dias").and_then(Value::as_i64).unwrap_or(7);
        let v = json!({"t": agora, "dias": dias, "itens": msg.get("medias")});
        if dias == 1 {
            self.medias1 = Some(v);
        } else {
            self.medias7 = Some(v);
        }
    }

    /// Janelas prontas para enviar: `(conta, corpo)`.
    pub fn entregar(
        &mut self,
        contas: &std::collections::BTreeMap<String, Conta>,
        agora: i64,
    ) -> Vec<(String, Value)> {
        let mut s = Vec::new();
        for (conta, c) in contas {
            let Some(login) = self.login.get(conta) else {
                continue;
            };
            if !c.pronta
                || self
                    .enviada
                    .get(conta)
                    .is_some_and(|t| agora - t < JANELA_MS)
            {
                continue;
            }
            self.enviada.insert(conta.clone(), agora);
            let vazio = VecDeque::new();
            let (_, desde) = self.hunt.get(conta).cloned().unwrap_or((None, agora));
            let mut j = montar(
                c,
                &Contexto {
                    id: conta,
                    welcome: login,
                    publico: &self.publico,
                    linhas: self.linhas.get(conta).unwrap_or(&vazio),
                    medias7: self.medias7.as_ref(),
                    medias1: self.medias1.as_ref(),
                    sessao_inicio: self.sessao.get(conta).copied().unwrap_or(agora),
                    hunt_desde: desde,
                    ouro_max: self.ouro_max.get(conta).copied().unwrap_or(0.0),
                    agora,
                },
            );
            // O login (pesado) só vai de vez em quando; o servidor guarda o último.
            if self
                .login_enviado
                .get(conta)
                .is_some_and(|t| agora - t < LOGIN_MS)
            {
                if let Value::Object(m) = &mut j {
                    m.remove("welcome");
                }
            } else {
                self.login_enviado.insert(conta.clone(), agora);
            }
            s.push((conta.to_lowercase(), j));
        }
        s
    }
}
