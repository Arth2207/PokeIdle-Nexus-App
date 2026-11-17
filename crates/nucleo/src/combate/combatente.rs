//! Combatente: os atributos de um pokémon numa luta (`makeMon` do `sim.js`) e o dano entre
//! dois combatentes (`dmg`, `invDmg`).

use serde_json::Value;

use super::especie::{Categoria, EspecieCombate, Pokedex};
use super::tipos::{Constantes, Tipo};
use crate::js;

/// Atributos já calculados.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Atributos {
    pub hp: f64,
    pub atk: f64,
    pub def: f64,
    pub spa: f64,
    pub spd: f64,
    pub spe: f64,
}

/// Golpe que o combatente pode usar.
#[derive(Debug, Clone, PartialEq)]
pub struct Golpe {
    pub nome: String,
    pub poder: f64,
    pub recarga: f64,
    pub categoria: Categoria,
    pub tipo: Tipo,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Combatente {
    pub especie: i64,
    pub nivel: f64,
    pub st: Atributos,
    /// HP de combate.
    pub hp: f64,
    pub selvagem: bool,
    pub tipos: Vec<Tipo>,
    pub golpes: Vec<Golpe>,
    /// Fator das recargas (`cdmul`): 1 menos o desconto pelo speed.
    pub mult_recarga: f64,
    /// Intervalo entre golpes (`gap`).
    pub intervalo: f64,
    /// Intervalo entre investidas (`inv`).
    pub intervalo_investida: f64,
    pub lendario: bool,
}

/// Opções do `makeMon`. O padrão é o do legado: qualidade 1,1, IVs 16, potência 1.
#[derive(Debug, Clone, PartialEq)]
pub struct Opcoes {
    pub qualidade: f64,
    /// HP, ATK, DEF, SpATK, SpDEF, SPD.
    pub ivs: [f64; 6],
    pub potencia: u8,
    pub shiny: bool,
    pub selvagem: bool,
    /// `None` deixa a espécie decidir (`isLegend`).
    pub lendario: Option<bool>,
}

impl Default for Opcoes {
    fn default() -> Self {
        Opcoes {
            qualidade: 1.1,
            ivs: [16.0; 6],
            potencia: 1,
            shiny: false,
            selvagem: false,
            lendario: None,
        }
    }
}

/// `POT` do `sim.js`. Potência fora de 1..=5 vira `NaN`, como o `undefined` do JS.
fn mult_potencia(p: u8) -> f64 {
    match p {
        1 => 1.0,
        2 => 1.05,
        3 => 1.1,
        4 => 1.25,
        5 => 2.0,
        _ => f64::NAN,
    }
}

/// `statCalc`: `round((base + 2·IV) × nível/100 × qualidade^expoente × multiplicador)`.
///
/// Potências usam [`js::pow`], o mesmo algoritmo do `Math.pow` do V8.
pub fn atributo(base: f64, iv: f64, nivel: f64, qualidade: f64, expoente: f64, mult: f64) -> f64 {
    js::round((base + 2.0 * iv) * nivel / 100.0 * js::pow(qualidade, expoente) * mult)
}

impl Combatente {
    /// `makeMon(sp, lvl, opcoes)`.
    pub fn novo(sp: &EspecieCombate, nivel: f64, op: &Opcoes, k: &Constantes) -> Self {
        let lendario = op.lendario.unwrap_or_else(|| sp.lendaria());
        let mult = mult_potencia(op.potencia)
            * if op.shiny { 3.0 } else { 1.0 }
            * if lendario { 4.0 } else { 1.0 };
        let calc = |i: usize, expoente: f64| {
            atributo(sp.base[i], op.ivs[i], nivel, op.qualidade, expoente, mult)
        };
        let st = Atributos {
            hp: calc(0, 0.95),
            atk: calc(1, 0.8),
            def: calc(2, 0.8),
            spa: calc(3, 0.8),
            spd: calc(4, 0.8),
            spe: calc(5, 0.95),
        };
        let hp = js::round(st.hp * 12.0).max(24.0) * if op.selvagem { k.hp_selvagem } else { 1.0 };
        let speed_base = sp.base[5];
        let potencial = if speed_base <= 120.0 {
            speed_base / 20.0
        } else {
            (6.0 + (speed_base - 120.0) / 40.0).min(7.0)
        };
        let iv_speed = op.ivs[5];
        let desconto = potencial.min(7.0) * iv_speed / 32.0 / 100.0;
        let golpes = sp
            .golpes
            .iter()
            .filter(|g| g.poder > 0.0 && g.nivel_min.is_none_or(|n| n <= nivel))
            .map(|g| Golpe {
                nome: g.nome.clone(),
                poder: g.poder,
                recarga: g.recarga,
                categoria: g.categoria,
                tipo: g.tipo,
            })
            .collect();
        Combatente {
            especie: sp.id,
            nivel,
            st,
            hp,
            selvagem: op.selvagem,
            tipos: sp.tipos.clone(),
            golpes,
            mult_recarga: 1.0 - desconto,
            intervalo: if op.selvagem {
                1.4
            } else {
                (0.9 - 0.01 * iv_speed).max(0.4)
            },
            intervalo_investida: (2.0 - 0.01 * iv_speed).max(0.4),
            lendario,
        }
    }

    /// STAB do golpe: 1,5 se o tipo do golpe é um dos tipos do combatente.
    pub fn stab(&self, tipo: Tipo) -> f64 {
        if self.tipos.contains(&tipo) {
            1.5
        } else {
            1.0
        }
    }
}

/// `dmg`: dano de um golpe, com a média da variação do jogo (0,925).
pub fn dano(
    att: &Combatente,
    def: &Combatente,
    poder: f64,
    categoria: Categoria,
    tipo: Tipo,
    stab: f64,
    k: &Constantes,
) -> f64 {
    let (a, d) = match categoria {
        Categoria::Fisico => (att.st.atk, def.st.def),
        Categoria::Especial => (att.st.spa, def.st.spd),
    };
    let base = ((2.0 * att.nivel / 5.0 + 2.0) * poder * a / d.max(1.0)) / 50.0 + 2.0;
    base * stab
        * k.efetividade_hunt(tipo, &def.tipos)
        * 0.925
        * if att.selvagem { k.dano_selvagem } else { 1.0 }
}

/// Tipo e efetividade da investida: o 1º tipo, ou o 2º se o 1º não pega no defensor.
pub fn tipo_investida(att: &Combatente, def: &Combatente, k: &Constantes) -> Option<(Tipo, f64)> {
    let primeiro = *att.tipos.first()?;
    let e = k.efetividade_hunt(primeiro, &def.tipos);
    match att.tipos.get(1) {
        Some(&segundo) if e == 0.0 => Some((segundo, k.efetividade_hunt(segundo, &def.tipos))),
        _ => Some((primeiro, e)),
    }
}

/// `invDmg`: a investida é física, poder 30, sempre com STAB.
pub fn dano_investida(att: &Combatente, def: &Combatente, k: &Constantes) -> f64 {
    tipo_investida(att, def, k).map_or(f64::NAN, |(t, _)| {
        dano(att, def, 30.0, Categoria::Fisico, t, 1.5, k)
    })
}

/// Perfil das equipes teóricas: `ideal` (qualidade 1,5, potência 5) ou `realista` (1,3 e 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Perfil {
    Ideal,
    Realista,
}

/// `monTeorico`: a espécie no nível dado, IVs 31.
pub fn teorico(sp: &EspecieCombate, nivel: f64, perfil: Perfil, k: &Constantes) -> Combatente {
    let (qualidade, potencia) = match perfil {
        Perfil::Ideal => (1.5, 5),
        Perfil::Realista => (1.3, 3),
    };
    let op = Opcoes {
        qualidade,
        potencia,
        ivs: [31.0; 6],
        ..Opcoes::default()
    };
    Combatente::novo(sp, nivel, &op, k)
}

/// `espDeFicha`: pelo `speciesId`, depois pelo `looktype`, depois pelo nome.
pub fn especie_da_ficha<'a>(dex: &'a Pokedex, f: &Value) -> Option<&'a EspecieCombate> {
    let por_id = |campo: &str| {
        let n = js::mais(f.get(campo));
        // Só inteiros exatos casam com a chave numérica do `Map`.
        (n.fract() == 0.0 && n.is_finite())
            .then(|| dex.por_id(n as i64))
            .flatten()
    };
    por_id("speciesId")
        .or_else(|| por_id("looktype"))
        .or_else(|| {
            let nome = match f.get("nome") {
                Some(v) if js::verdadeiro(Some(v)) => {
                    v.as_str().map_or_else(|| v.to_string(), str::to_string)
                }
                _ => String::new(),
            };
            dex.por_nome(&nome)
        })
}

/// `monDeFicha`: combatente de uma ficha real (mercado ou coleção). Quando a ficha traz os
/// seis atributos positivos, eles substituem os calculados, e o HP vem de `maxHp`.
pub fn de_ficha<'a>(
    dex: &'a Pokedex,
    f: &Value,
    k: &Constantes,
) -> Option<(Combatente, &'a EspecieCombate)> {
    let sp = especie_da_ficha(dex, f)?;
    let nivel = js::mais(f.get("level"));
    if nivel.is_nan() || nivel <= 0.0 {
        return None;
    }
    let ivs = match f.get("ivs") {
        Some(iv) if js::verdadeiro(Some(iv)) => {
            let campo = |a: &str, b: &str| {
                // `a ?? b`: só cai no segundo se o primeiro for null/undefined.
                let v = match iv.get(a) {
                    None | Some(Value::Null) => iv.get(b),
                    outro => outro,
                };
                ou_zero(js::mais(v))
            };
            [
                ou_zero(js::mais(iv.get("hp"))),
                ou_zero(js::mais(iv.get("atk"))),
                ou_zero(js::mais(iv.get("def"))),
                campo("spAtk", "spa"),
                campo("spDef", "spd"),
                campo("speed", "spe"),
            ]
        }
        _ => [16.0; 6],
    };
    let potencia = js::round(ou(js::mais(f.get("potencia")), 1.0)).clamp(1.0, 5.0) as u8;
    let op = Opcoes {
        qualidade: ou(js::mais(f.get("quality")), 1.1),
        ivs,
        potencia,
        shiny: js::verdadeiro(f.get("shiny")),
        ..Opcoes::default()
    };
    let mut refinada = sp.clone();
    for (i, campo) in ["hp", "atk", "def", "spAtk", "spDef", "speed"]
        .iter()
        .enumerate()
    {
        let v = js::mais(f.get("refino").and_then(|r| r.get(*campo)));
        if v.is_finite() && v > 0.0 {
            refinada.base[i] += v;
        }
    }
    let mut mon = Combatente::novo(&refinada, nivel, &op, k);
    aplicar_stats_recebidos(&mut mon, f);
    Some((mon, sp))
}

/// `x || padrao` para números: 0 e `NaN` caem no padrão.
fn ou(x: f64, padrao: f64) -> f64 {
    if x == 0.0 || x.is_nan() {
        padrao
    } else {
        x
    }
}

fn ou_zero(x: f64) -> f64 {
    ou(x, 0.0)
}

/// Usa atributos já calculados pelo jogo (incluindo equipamentos/refinamentos).
pub fn aplicar_stats_recebidos(mon: &mut Combatente, f: &Value) {
    if let Some(s) = f.get("stats").filter(|s| js::verdadeiro(Some(s))) {
        let v = ["hp", "atk", "def", "spAtk", "spDef", "speed"].map(|c| js::mais(s.get(c)));
        if v.iter().all(|&x| x > 0.0) {
            let [hp, atk, def, spa, spd, spe] = v;
            mon.st = Atributos {
                hp,
                atk,
                def,
                spa,
                spd,
                spe,
            };
            let max_hp = js::mais(f.get("maxHp"));
            mon.hp = if max_hp > 0.0 { max_hp } else { hp * 12.0 };
        }
    }
}

/// Z-Move da ficha; poder depende da modalidade (hunt/boss 2800, PvP 1800).
/// A área do golpe não é convertida em multiplicador fictício de dano.
pub fn aplicar_z_move(mon: &mut Combatente, f: &Value, poder: f64) {
    let tm = &f["tmElemental"];
    let tipo = tm
        .as_str()
        .or_else(|| tm.get("tipo").and_then(Value::as_str))
        .and_then(|t| t.to_uppercase().parse::<Tipo>().ok());
    if let Some(tipo) = tipo {
        mon.golpes.retain(|g| g.nome != "Z-Move");
        mon.golpes.push(Golpe {
            nome: "Z-Move".into(),
            poder,
            recarga: 60.0,
            categoria: if f["tmFisico"] == true || tm["categoria"] == "F" {
                Categoria::Fisico
            } else {
                Categoria::Especial
            },
            tipo,
        });
    }
}

/// Multiplicador explícito de atributos na hunt; aplicado a ambos os lados no tipo do dia.
pub fn multiplicar_atributos(mon: &mut Combatente, mult: f64) {
    mon.st.hp *= mult;
    mon.st.atk *= mult;
    mon.st.def *= mult;
    mon.st.spa *= mult;
    mon.st.spd *= mult;
    mon.st.spe *= mult;
    mon.hp *= mult;
}

#[cfg(test)]
mod testes_ficha_atual {
    use super::*;
    use serde_json::json;
    #[test]
    fn refino_soma_na_base_e_stats_recebidos_nao_sao_refinados_duas_vezes(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let dex = Pokedex::de_valor(
            json!([{"d":6,"n":"Charizard","t":["FIRE"],"s":[78,84,78,109,85,100],"m":[]}]),
        )?;
        let f = json!({"speciesId":6,"level":100,"quality":1.0,"potencia":1,"ivs":{"hp":27,"atk":27,"def":27,"spAtk":27,"spDef":27,"speed":27},"refino":{"hp":4}});
        let (m, _) = de_ficha(&dex, &f, &Constantes::wiki()).ok_or("ficha")?;
        assert_eq!(m.st.hp, 136.0);
        let mut f = f;
        f["stats"] = json!({"hp":100,"atk":200,"def":300,"spAtk":400,"spDef":500,"speed":600});
        let (m, _) = de_ficha(&dex, &f, &Constantes::wiki()).ok_or("ficha")?;
        assert_eq!(m.st.hp, 100.0);
        Ok(())
    }

    #[test]
    fn atributos_reais_e_z_move_respeitam_ficha_sem_inventar_area(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let dex = Pokedex::de_valor(
            json!([{"d":6,"n":"Charizard","t":["FIRE","FLYING"],"s":[78,84,78,109,85,100],"m":[]}]),
        )?;
        let f = json!({"speciesId":6,"level":100,"shiny":true,"stats":{"hp":100,"atk":200,"def":300,"spAtk":400,"spDef":500,"speed":600},"maxHp":1200,"tmElemental":"GHOST","tmFisico":true});
        let (mut mon, _) = de_ficha(&dex, &f, &Constantes::wiki()).ok_or("ficha")?;
        assert_eq!(
            mon.st.atk, 200.0,
            "stats recebidos não são multiplicados outra vez por shiny"
        );
        aplicar_z_move(&mut mon, &f, 2800.0);
        assert_eq!(mon.golpes[0].categoria, Categoria::Fisico);
        assert_eq!(
            mon.golpes[0].tipo,
            Tipo::Ghost,
            "tipo permanente pode diferir da espécie atual"
        );
        assert_eq!(mon.golpes[0].recarga, 60.0);
        aplicar_z_move(&mut mon, &f, 2800.0);
        assert_eq!(mon.golpes.len(), 1);
        multiplicar_atributos(&mut mon, 2.0);
        assert_eq!(mon.hp, 2400.0);
        assert_eq!(mon.st.atk, 400.0);
        Ok(())
    }
}
