//! Estado de uma conta do jogo, montado com o `welcome` (login) e os deltas de `estado`
//! (`pkMud`, `pkFora`, `items`, `balls`, `boss`, `huntSlug`, `level`), como o motor do legado.

use std::collections::BTreeMap;

use serde_json::{json, Map, Value};

/// Campos da ficha que o servidor usa para montar equipes (`_EQ_CAMPOS` do relay antigo).
pub const CAMPOS_EQUIPE: [&str; 19] = [
    "id",
    "speciesId",
    "looktype",
    "nome",
    "level",
    "quality",
    "potencia",
    "ivs",
    "ivTotal",
    "nota",
    "stats",
    "maxHp",
    "poder",
    "tipos",
    "shiny",
    "slot",
    "tmElemental",
    "tmAoe",
    "refinoTotal",
];

#[derive(Debug, Default, Clone)]
pub struct Conta {
    /// Nick como o jogo mostra.
    pub nick: String,
    pub level: Option<i64>,
    pub hunt: Option<String>,
    pub vitorias: Vec<String>,
    pub itens: BTreeMap<i64, i64>,
    pub bolas: BTreeMap<i64, i64>,
    pub pokemons: BTreeMap<i64, Value>,
    /// Já recebeu o estado cheio (login)?
    pub pronta: bool,
    pub ouro: Option<f64>,
    /// `estado.automation` (bolas, poções, limiar de HP, travas).
    pub automacao: Value,
    pub ativo: Option<i64>,
    pub bonus_guild: Option<f64>,
    /// Tipo do dia sazonal (`diaSazonal.tipo`).
    pub dia_tipo: Option<String>,
    /// Tier escolhido na Outland (`outlandTier`).
    pub tier_outland: Option<i64>,
    /// XP do treinador: total, início e fim do nível.
    pub xp: (f64, f64, f64),
}

fn soma_ivs(p: &Value) -> Option<i64> {
    let ivs = p.get("ivs")?.as_object()?;
    Some(ivs.values().filter_map(Value::as_i64).sum())
}

fn mesclar(dest: &mut BTreeMap<i64, i64>, v: Option<&Value>) {
    for (k, q) in v.and_then(Value::as_object).into_iter().flatten() {
        if let (Ok(id), Some(q)) = (k.parse::<i64>(), q.as_f64()) {
            dest.insert(id, q.floor() as i64);
        }
    }
}

impl Conta {
    /// Aplica `welcome` ou `estado`. Devolve `true` se mudou algo que o servidor usa.
    pub fn aplicar(&mut self, msg: &Value) -> bool {
        let Some(e) = msg.get("estado").filter(|e| e.is_object()) else {
            return false;
        };
        let cheio = msg.get("t").and_then(Value::as_str) == Some("welcome")
            || e.get("cheio").and_then(Value::as_bool) == Some(true);
        if cheio {
            *self = Self {
                pronta: true,
                ..Self::default()
            };
        }
        if let Some(n) = e.get("nick").and_then(Value::as_str) {
            self.nick = n.to_owned();
        }
        if let Some(l) = e.get("level").and_then(Value::as_i64) {
            self.level = Some(l);
        }
        if let Some(g) = e.get("gold").and_then(Value::as_f64) {
            self.ouro = Some(g);
        }
        if let Some(a) = e.get("automation").filter(|a| a.is_object()) {
            self.automacao = a.clone();
        }
        if let Some(a) = e.get("activeId").and_then(Value::as_i64) {
            self.ativo = Some(a);
        }
        if let Some(b) = e.get("guildBonusPct").and_then(Value::as_f64) {
            self.bonus_guild = Some(b);
        }
        if let Some(t) = e.get("outlandTier").and_then(Value::as_i64) {
            self.tier_outland = Some(t);
        }
        let x = |k: &str| e.get(k).and_then(Value::as_f64);
        if let (Some(a), Some(b), Some(c)) = (x("xp"), x("xpNivel"), x("xpProximo")) {
            self.xp = (a, b, c);
        }
        if let Some(t) = e.pointer("/diaSazonal/tipo").and_then(Value::as_str) {
            self.dia_tipo = Some(t.to_owned());
        }
        if let Some(h) = e.get("huntSlug") {
            self.hunt = h.as_str().filter(|s| !s.is_empty()).map(str::to_owned);
        }
        if let Some(v) = e.pointer("/boss/vitorias").and_then(Value::as_object) {
            self.vitorias = v
                .iter()
                .filter(|(_, x)| x.is_object() || x.as_f64().is_some_and(|n| n != 0.0))
                .map(|(k, _)| k.clone())
                .collect();
        }
        let eu = e.get("eu");
        mesclar(
            &mut self.itens,
            eu.and_then(|x| x.get("items")).or(e.get("items")),
        );
        mesclar(
            &mut self.bolas,
            eu.and_then(|x| x.get("balls")).or(e.get("balls")),
        );
        if let Some(ps) = e.get("pokemons").and_then(Value::as_array) {
            self.pokemons.clear();
            self.guardar(ps);
        }
        if let Some(ps) = e.get("pkMud").and_then(Value::as_array) {
            self.guardar(ps);
        }
        for id in e
            .get("pkFora")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(id) = id.as_i64() {
                self.pokemons.remove(&id);
            }
        }
        true
    }

    fn guardar(&mut self, ps: &[Value]) {
        for p in ps {
            if let Some(id) = p.get("id").and_then(Value::as_i64) {
                self.pokemons.insert(id, p.clone());
            }
        }
    }

    /// Ficha resumida para o servidor (com `ivTotal` calculado se faltar).
    pub fn fichas(&self) -> Vec<Value> {
        self.pokemons
            .values()
            .map(|p| {
                let mut o = Map::new();
                for k in CAMPOS_EQUIPE {
                    if let Some(x) = p.get(k) {
                        o.insert(k.into(), x.clone());
                    }
                }
                if !o.contains_key("ivTotal") {
                    if let Some(t) = soma_ivs(p) {
                        o.insert("ivTotal".into(), t.into());
                    }
                }
                Value::Object(o)
            })
            .collect()
    }

    /// Corpo do `POST /api/equipes/ingest`.
    pub fn equipe(&self, bosses: Option<&Value>) -> Value {
        let mut b = json!({
            "nick": self.nick.to_lowercase(),
            "trLv": self.level,
            "vencidos": self.vitorias,
            "pokemons": self.fichas(),
        });
        if let (Some(bs), Value::Object(o)) = (bosses, &mut b) {
            o.insert("bosses".into(), bs.clone());
        }
        b
    }
}

/// Lista de bosses do `welcome` no formato do motor (`bossesJogaveis`).
pub fn bosses_do_welcome(w: &Value) -> Option<Value> {
    let lista = w.get("bossesJogaveis")?.as_array()?;
    let bs: Vec<Value> = lista
        .iter()
        .map(|b| {
            let tipos: Vec<Value> = match b.get("tipos").or(b.get("types")).and_then(Value::as_array) {
                Some(t) => t.clone(),
                None => [b.get("type1"), b.get("type2")]
                    .into_iter()
                    .flatten()
                    .filter(|t| t.as_str().is_some_and(|s| !s.is_empty()))
                    .cloned()
                    .collect(),
            };
            let drops: Vec<Value> = b
                .get("drops")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(|d| json!({"itemId": d.get("itemId"), "nome": d.get("nome"), "chance": d.get("chance")}))
                .collect();
            json!({
                "key": b.get("key"),
                "nome": b.get("name").or(b.get("nome")).or(b.get("key")),
                "tipos": tipos,
                "entrada": b.get("entrada"),
                "teamLevel": b.get("teamLevel").or(b.get("level")),
                "level": b.get("level"),
                "minTr": b.get("minNivelTreinador"),
                "drops": drops,
            })
        })
        .collect();
    (bs.len() >= 3).then_some(Value::Array(bs))
}

#[cfg(test)]
mod testes {
    use super::*;

    fn poke(id: i64) -> Value {
        json!({"id": id, "speciesId": 6, "nome": "Charizard", "level": 10,
               "ivs": {"hp": 1, "atk": 2}, "xp": 9, "slot": 0})
    }

    #[test]
    fn login_e_deltas() {
        let mut c = Conta::default();
        c.aplicar(&json!({"t": "welcome", "estado": {
            "nick": "Sikien", "level": 1021, "huntSlug": "furious_scizor",
            "items": {"23": 190, "70000": 11}, "balls": {"1": 4},
            "boss": {"vitorias": {"articuno": 2, "moltres": 0}},
            "pokemons": [poke(1), poke(2)]}}));
        assert!(c.pronta);
        assert_eq!(c.hunt.as_deref(), Some("furious_scizor"));
        assert_eq!(c.vitorias, vec!["articuno".to_owned()]);
        c.aplicar(&json!({"t": "estado", "estado": {
            "items": {"23": 180}, "pkMud": [poke(3)], "pkFora": [1], "huntSlug": "caterpie"}}));
        assert_eq!(c.itens.get(&23), Some(&180));
        assert_eq!(c.itens.get(&70000), Some(&11));
        assert_eq!(c.pokemons.keys().copied().collect::<Vec<_>>(), vec![2, 3]);
        assert_eq!(c.hunt.as_deref(), Some("caterpie"));
        let f = &c.fichas()[0];
        assert_eq!(f["ivTotal"], 3);
        assert!(f.get("xp").is_none());
        let e = c.equipe(None);
        assert_eq!(e["nick"], "sikien");
        assert_eq!(e["trLv"], 1021);
    }

    #[test]
    fn bosses_no_formato_do_motor() {
        let b = json!({"key": "articuno", "nome": "Articuno", "type1": "ICE", "type2": "FLYING",
                       "level": 300, "teamLevel": 300, "minNivelTreinador": 300,
                       "drops": [{"itemId": 1, "nome": "x", "chance": 0.5, "qtd": null}]});
        let w = json!({"bossesJogaveis": [b.clone(), b.clone(), b]});
        let bs = bosses_do_welcome(&w).unwrap_or_default();
        assert_eq!(bs[0]["tipos"], json!(["ICE", "FLYING"]));
        assert_eq!(bs[0]["minTr"], 300);
        assert_eq!(
            bs[0]["drops"][0],
            json!({"itemId": 1, "nome": "x", "chance": 0.5})
        );
        assert!(bosses_do_welcome(&json!({"bossesJogaveis": []})).is_none());
    }
}
