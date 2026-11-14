//! Diário por conta: abates, bolas, capturas, shiny e drops de cada dia, lidos das linhas do
//! HUD com as mesmas regras do motor do legado (`contarDia`). Também mede o ritmo dos últimos
//! 15 minutos. Vira JSON para sobreviver a reinícios do app.

use std::collections::{BTreeMap, HashMap, VecDeque};

use serde_json::{json, Map, Value};

use crate::relay::tempo::dia_de;

/// Dias guardados por conta (o servidor usa os últimos 45).
const DIAS_GUARDADOS: usize = 45;
/// Janela do ritmo.
pub const JANELA_MS: i64 = 15 * 60_000;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Dia {
    pub kills: u64,
    pub bolas: u64,
    pub capt: u64,
    pub shiny: u64,
    pub pot: u64,
    pub desm: u64,
    pub ouro: f64,
    pub xp: f64,
    pub xp_pk: f64,
    pub drops: BTreeMap<String, f64>,
}

impl Dia {
    fn json(&self) -> Value {
        json!({ "kills": self.kills, "bolas": self.bolas, "capt": self.capt,
                "shiny": self.shiny, "pot": self.pot, "desm": self.desm, "ouro": self.ouro,
                "xp": self.xp, "xpPk": self.xp_pk, "drops": self.drops })
    }

    fn de_json(v: &Value) -> Self {
        let n = |k: &str| v.get(k).and_then(Value::as_u64).unwrap_or(0);
        let f = |k: &str| v.get(k).and_then(Value::as_f64).unwrap_or(0.0);
        let drops = v
            .get("drops")
            .and_then(Value::as_object)
            .map(|m| {
                m.iter()
                    .filter_map(|(k, q)| Some((k.clone(), q.as_f64()?)))
                    .collect()
            })
            .unwrap_or_default();
        Self {
            kills: n("kills"),
            bolas: n("bolas"),
            capt: n("capt"),
            shiny: n("shiny"),
            pot: n("pot"),
            desm: n("desm"),
            ouro: f("ouro"),
            xp: f("xp"),
            xp_pk: f("xpPk"),
            drops,
        }
    }
}

/// O que uma linha do HUD conta.
#[derive(Debug, PartialEq)]
enum Linha {
    /// XP do treinador, XP do pokémon, ouro e drops.
    Kill(f64, f64, f64, Vec<(String, f64)>),
    Pocao,
    Desmaio {
        shiny: bool,
    },
    Captura {
        shiny: bool,
    },
    Fuga {
        shiny: bool,
    },
    Outra {
        shiny: bool,
    },
}

fn num_br(s: &str) -> f64 {
    s.trim()
        .replace('.', "")
        .replace(',', ".")
        .parse()
        .unwrap_or(0.0)
}

/// `<nome> derrotado! +X xp treinador, +Y xp pokemon, +Z ouro[ (+W do bônus de loot)][drops]`.
fn ler_kill(t: &str) -> Option<Linha> {
    let (_, resto) = t.split_once(" derrotado! +")?;
    let (xt, resto) = resto.split_once(" xp treinador, +")?;
    let (xp, resto) = resto.split_once(" xp pokemon, +")?;
    let (ouro, resto) = resto.split_once(" ouro")?;
    let resto = match resto.strip_prefix(" (+") {
        Some(r) => {
            r.split_once(" do b")
                .and_then(|(_, r)| r.split_once(')'))?
                .1
        }
        None => resto,
    };
    let drops = resto
        .split('·')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| match s.rsplit_once('×') {
            Some((n, q))
                if !q.trim().is_empty()
                    && q.trim().chars().all(|c| c.is_ascii_digit() || c == '.') =>
            {
                (n.trim().to_owned(), num_br(q))
            }
            _ => (s.to_owned(), 1.0),
        })
        .collect();
    Some(Linha::Kill(num_br(xt), num_br(xp), num_br(ouro), drops))
}

fn ler_linha(t: &str) -> Linha {
    if let Some(k) = ler_kill(t) {
        return k;
    }
    let baixo = t.to_lowercase();
    let shiny = baixo.contains("shiny");
    // Formatos do HUD: `Capturado: X Nv 5 (q 1,5)` (legado) e `Capturou X Nv.5! Qualidade: 1,5`.
    let captura = baixo.find("captur").is_some_and(|i| {
        let r = &baixo[i..];
        r.contains(" nv") && (r.contains("(q") || r.contains("qualidade"))
    });
    if captura {
        Linha::Captura { shiny }
    } else if baixo.contains("escapou e sumiu") {
        Linha::Fuga { shiny }
    } else if ["poção usada", "pocao usada", "poçao usada", "pocão usada"]
        .iter()
        .any(|p| baixo.contains(p))
    {
        Linha::Pocao
    } else if baixo.contains("desmai") {
        Linha::Desmaio { shiny }
    } else {
        Linha::Outra { shiny }
    }
}

#[derive(Debug, Default)]
pub struct Diario {
    /// nick → dia → contagens.
    dias: BTreeMap<String, BTreeMap<String, Dia>>,
    /// nick → (instante, é abate?) dos últimos 15 min.
    janela: HashMap<String, VecDeque<(i64, bool)>>,
}

impl Diario {
    /// Linhas novas do HUD (`[seq, ts, texto]`) de uma conta.
    pub fn linhas(&mut self, nick: &str, linhas: &[Value]) {
        for l in linhas {
            let (Some(ts), Some(t)) = (
                l.get(1).and_then(Value::as_i64),
                l.get(2).and_then(Value::as_str),
            ) else {
                continue;
            };
            self.linha(nick, ts, t);
        }
    }

    fn linha(&mut self, nick: &str, ts: i64, texto: &str) {
        let d = self
            .dias
            .entry(nick.to_owned())
            .or_default()
            .entry(dia_de(ts))
            .or_default();
        let (bola, kill) = match ler_linha(texto) {
            Linha::Kill(xt, xp, ouro, drops) => {
                d.kills += 1;
                d.xp += xt;
                d.xp_pk += xp;
                d.ouro += ouro;
                for (n, q) in drops {
                    *d.drops.entry(n).or_default() += q;
                }
                (false, true)
            }
            Linha::Captura { shiny } => {
                d.shiny += u64::from(shiny);
                d.capt += 1;
                d.bolas += 1;
                (true, false)
            }
            Linha::Fuga { shiny } => {
                d.shiny += u64::from(shiny);
                d.bolas += 1;
                (true, false)
            }
            Linha::Pocao => {
                d.pot += 1;
                (false, false)
            }
            Linha::Desmaio { shiny } => {
                d.shiny += u64::from(shiny);
                d.desm += 1;
                (false, false)
            }
            Linha::Outra { shiny } => {
                d.shiny += u64::from(shiny);
                (false, false)
            }
        };
        if bola || kill {
            let j = self.janela.entry(nick.to_owned()).or_default();
            j.push_back((ts, kill));
            while j.front().is_some_and(|(t, _)| *t < ts - JANELA_MS) {
                j.pop_front();
            }
        }
        if let Some(por) = self.dias.get_mut(nick) {
            while por.len() > DIAS_GUARDADOS {
                por.pop_first();
            }
        }
    }

    /// Ritmo dos últimos 15 min: abates por hora e bolas por abate.
    pub fn ritmo(&self, nick: &str, agora: i64) -> (f64, f64) {
        let Some(j) = self.janela.get(nick) else {
            return (0.0, 0.0);
        };
        let (mut k, mut b) = (0.0, 0.0);
        for (t, kill) in j {
            if *t >= agora - JANELA_MS {
                if *kill {
                    k += 1.0;
                } else {
                    b += 1.0;
                }
            }
        }
        let kph = k * 3_600_000.0 / JANELA_MS as f64;
        (kph, if k > 0.0 { b / k } else { 0.0 })
    }

    pub fn nicks(&self) -> impl Iterator<Item = &String> {
        self.dias.keys()
    }

    pub fn dias(&self, nick: &str) -> Option<&BTreeMap<String, Dia>> {
        self.dias.get(nick)
    }

    /// Registro de um dia no formato do motor (`D.dia[nick][dia]`).
    pub fn dia_json(&self, nick: &str, dia: &str) -> Option<Value> {
        self.dias.get(nick)?.get(dia).map(Dia::json)
    }

    /// Drops por dia (média dos últimos 7 dias completos), somando as contas: nome → qtd/dia.
    pub fn drops_por_dia(&self, hoje: &str) -> HashMap<String, f64> {
        let mut r: HashMap<String, f64> = HashMap::new();
        for por in self.dias.values() {
            let completos: Vec<&Dia> = por
                .iter()
                .rev()
                .take(7)
                .filter(|(k, _)| k.as_str() < hoje)
                .map(|(_, d)| d)
                .collect();
            if completos.is_empty() {
                continue;
            }
            let n = completos.len() as f64;
            for d in completos {
                for (nome, q) in &d.drops {
                    *r.entry(nome.clone()).or_default() += q / n;
                }
            }
        }
        r
    }

    pub fn para_json(&self) -> Value {
        let dias: Map<String, Value> = self
            .dias
            .iter()
            .map(|(n, por)| {
                let m: Map<String, Value> =
                    por.iter().map(|(k, d)| (k.clone(), d.json())).collect();
                (n.clone(), Value::Object(m))
            })
            .collect();
        json!({ "v": 1, "dias": dias })
    }

    pub fn de_json(v: &Value) -> Self {
        let dias = v
            .get("dias")
            .and_then(Value::as_object)
            .map(|m| {
                m.iter()
                    .filter_map(|(n, por)| {
                        let por = por.as_object()?;
                        Some((
                            n.clone(),
                            por.iter()
                                .map(|(k, d)| (k.clone(), Dia::de_json(d)))
                                .collect(),
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default();
        Self {
            dias,
            janela: HashMap::new(),
        }
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    const T: i64 = 1_794_632_400_000;

    #[test]
    fn le_as_linhas_como_o_legado() {
        assert_eq!(
            ler_linha("Scizor derrotado! +1.200 xp treinador, +1.200 xp pokemon, +850 ouro (+85 do bônus de loot) · Metal Coat × 2 · Rare Candy"),
            Linha::Kill(
                1200.0,
                1200.0,
                850.0,
                vec![("Metal Coat".into(), 2.0), ("Rare Candy".into(), 1.0)]
            )
        );
        assert_eq!(
            ler_linha("Pidgey derrotado! +10 xp treinador, +10 xp pokemon, +5 ouro"),
            Linha::Kill(10.0, 10.0, 5.0, vec![])
        );
        assert_eq!(
            ler_linha("Capturado: Pikachu Nv 12 (q 1,5) shiny!"),
            Linha::Captura { shiny: true }
        );
        assert_eq!(
            ler_linha("Capturou Abra Nv.5! Qualidade: 1,2"),
            Linha::Captura { shiny: false }
        );
        assert_eq!(
            ler_linha("Abra escapou e sumiu"),
            Linha::Fuga { shiny: false }
        );
        assert_eq!(ler_linha("Um Shiny apareceu"), Linha::Outra { shiny: true });
        assert_eq!(ler_linha("Poção usada em Charizard"), Linha::Pocao);
        assert_eq!(
            ler_linha("Charizard desmaiou"),
            Linha::Desmaio { shiny: false }
        );
    }

    #[test]
    fn conta_por_dia_ritmo_e_volta_do_json() {
        let mut d = Diario::default();
        let l = |s: i64, t: &str| json!([s, T + s * 60_000, t]);
        d.linhas(
            "Sikien",
            &[
                l(
                    0,
                    "A derrotado! +1 xp treinador, +1 xp pokemon, +1 ouro · X × 3",
                ),
                l(1, "B derrotado! +1 xp treinador, +1 xp pokemon, +1 ouro"),
                l(2, "Capturado: C Nv 5 (q 1,0)"),
                l(3, "D escapou e sumiu"),
            ],
        );
        let dia = &d.dias("Sikien").unwrap()["2026-11-14"];
        assert_eq!((dia.kills, dia.bolas, dia.capt, dia.shiny), (2, 2, 1, 0));
        assert_eq!(dia.drops["X"], 3.0);
        assert_eq!((dia.ouro, dia.xp), (2.0, 2.0));
        assert_eq!(d.ritmo("Sikien", T + 4 * 60_000), (8.0, 1.0));
        assert_eq!(d.ritmo("Sikien", T + 60 * 60_000), (0.0, 0.0));
        let volta = Diario::de_json(&d.para_json());
        assert_eq!(volta.dias("Sikien"), d.dias("Sikien"));
        // Hoje não entra na média de drops; amanhã entra.
        assert!(d.drops_por_dia("2026-11-14").is_empty());
        assert_eq!(d.drops_por_dia("2026-11-15")["X"], 3.0);
    }
}
