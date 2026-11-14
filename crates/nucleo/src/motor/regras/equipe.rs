//! Regras do motor: frente equipe e rodízio de boss (etapa 4).
//!
//! Porte de `priorizarBosses`, `melhorTimePossivel`, `sugerirBuild`, `combateDe`,
//! `planoRodizio`, `planoEquipe`, `motorRodizio` e `motorEquipe` do `motor.js` (versão do PC),
//! com as dependências privadas que elas puxam (`S_.fight`, `S_.dpsOut`, `buildTeorica`,
//! `melhorCombo`, `aguentaHuntDe`, `huntParaNivelar`, `VER.ivArr`, `VER.potOf`).
//!
//! Tudo é puro: planos e rotas devolvem o mesmo JSON do legado; as regras devolvem
//! [`Decisao`]. Em sombra, a resposta do jogo (`'ok'`, `avaliar`) é sempre assumida como
//! sucesso; os pontos onde o legado ramificaria pela resposta ficam comentados com
//! "RESPOSTA DO JOGO".
//!
//! Diferenças deliberadas em relação ao legado: sem caches (`buildCache`, `melhorTimeCache`:
//! o cache do legado devolve resultado velho quando só mudam slot, qualidade ou constantes);
//! constantes do servidor lidas de `conta.pub` a cada chamada, em vez de globais grudadas.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::LazyLock;

use regex::Regex;
use serde_json::{json, Map, Value};

use super::{alerta, ContaViva, Decisao, EstadoLaco, Registro};
use crate::combate::combatente::{dano, dano_investida, tipo_investida, Combatente, Opcoes};
use crate::combate::especie::{EspecieCombate, Pokedex};
use crate::combate::tipos::{efetividade_bruta, Constantes, Tipo};
use crate::js;
use crate::motor::jogo::{self, norm, norm_texto};
use crate::motor::{caminho, config, Relogio};

/// `JANELA_BOSS`: segundos simulados contra o boss.
const JANELA_BOSS: f64 = 400.0;
/// `CONTAS_BOSS` (cópia: a de `config.rs` é privada).
const CONTAS_BOSS: &[&str] = &["Alllphhha"];
/// `BOSSES_NIVEL`, na ordem do legado.
const BOSSES_NIVEL: [(&str, f64); 25] = [
    ("regirock", 300.0),
    ("regice", 300.0),
    ("registeel", 650.0),
    ("kyogre", 650.0),
    ("groudon", 3100.0),
    ("rayquaza", 3100.0),
    ("uxie", 9750.0),
    ("mesprit", 9750.0),
    ("azelf", 22700.0),
    ("dialga", 22700.0),
    ("palkia", 44050.0),
    ("heatran", 44050.0),
    ("regigigas", 75900.0),
    ("giratina", 75900.0),
    ("cresselia", 120350.0),
    ("cobalion", 120350.0),
    ("terrakion", 179500.0),
    ("virizion", 179500.0),
    ("reshiram", 255450.0),
    ("zekrom", 255450.0),
    ("kyurem", 350300.0),
    ("kartana", 350300.0),
    ("guzzlord", 466150.0),
    ("poipole", 466150.0),
    ("naganadel", 605100.0),
];
const SLOTS_BOSS: usize = 5;

type Re = LazyLock<Option<Regex>>;
static ABATE: Re = LazyLock::new(|| {
    Regex::new(
        r"^(.+?) derrotado! \+([0-9.]+) xp treinador, \+([0-9.]+) xp pokemon, \+([0-9.]+) ouro",
    )
    .ok()
});
static FUGA: Re = LazyLock::new(|| Regex::new(r"(?i)escapou e sumiu").ok());
static CAPTURA: Re = LazyLock::new(|| Regex::new(r"(?i)captur[A-Za-z0-9_]*:").ok());
static POCAO: Re = LazyLock::new(|| Regex::new(r"(?i)po[cç][aã]o usada").ok());
static DESMAIO: Re = LazyLock::new(|| Regex::new(r"(?i)desmai").ok());
static XP_POKE: Re = LazyLock::new(|| Regex::new(r"(?i)^xp pok[eé]mon$").ok());
static POR_HORA: Re = LazyLock::new(|| Regex::new(r"/h$").ok());
static MI: Re = LazyLock::new(|| Regex::new(r"(?i)\bmi\b").ok());
static MIL: Re = LazyLock::new(|| Regex::new(r"(?i)\bmil\b").ok());

fn casa(re: &Re, t: &str) -> bool {
    re.as_ref().is_some_and(|r| r.is_match(t))
}

// ---------------------------------------------------------------------------------------
// Auxiliares de JS
// ---------------------------------------------------------------------------------------

/// `a || b` para números.
fn ou(a: f64, b: f64) -> f64 {
    if a == 0.0 || a.is_nan() {
        b
    } else {
        a
    }
}

/// `+v || 0`.
fn n0(v: Option<&Value>) -> f64 {
    ou(js::mais(v), 0.0)
}

/// `v || padrão` para valores numéricos: o valor convertido se for verdadeiro.
fn ou_val(v: Option<&Value>) -> Option<f64> {
    js::verdadeiro(v).then(|| js::mais(v))
}

fn verd(v: Option<&Value>) -> bool {
    js::verdadeiro(v)
}

fn jn(x: f64) -> Value {
    js::json_num(x)
}

fn jn_ou_nulo(x: Option<f64>) -> Value {
    x.map_or(Value::Null, jn)
}

fn txt(v: Option<&Value>) -> String {
    js::texto(v)
}

fn num_s(x: f64) -> String {
    js::numero(x)
}

/// `a === b` entre valores do JSON (números comparados como `f64`; `undefined` só é igual
/// a `undefined`).
fn igual_estrito(a: Option<&Value>, b: Option<&Value>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(Value::Number(x)), Some(Value::Number(y))) => x.as_f64() == y.as_f64(),
        (Some(x), Some(y)) => x == y,
        _ => false,
    }
}

/// `v == null`.
fn nulo(v: Option<&Value>) -> bool {
    matches!(v, None | Some(Value::Null))
}

/// `Array.prototype.sort` do V8 (inserção binária, estável): cópia de `ordenar_como_js` de
/// `leitura.rs`, que é privada. Aguenta comparador que devolve `NaN`.
fn ordenar<T>(v: &mut Vec<T>, cmp: impl Fn(&T, &T) -> f64) {
    for i in 1..v.len() {
        let (mut esq, mut dir) = (0, i);
        while esq < dir {
            let meio = esq + ((dir - esq) >> 1);
            if cmp(&v[i], &v[meio]) < 0.0 {
                dir = meio;
            } else {
                esq = meio + 1;
            }
        }
        let x = v.remove(i);
        v.insert(esq, x);
    }
}

/// `Math.log10` do V8 (`ieee754::log10`, derivado do fdlibm), sobre o `ln` já portado.
#[allow(clippy::excessive_precision, clippy::approx_constant)]
fn log10(x: f64) -> f64 {
    const TWO54: f64 = 1.801_439_850_948_198_4e16;
    const IVLN10: f64 = 4.342_944_819_032_518_2e-1;
    const LOG10_2HI: f64 = 3.010_299_956_636_117_7e-1;
    const LOG10_2LO: f64 = 3.694_239_077_158_930_8e-13;
    let bits = x.to_bits();
    let mut hx = (bits >> 32) as i32;
    let lx = bits as u32;
    let mut k: i32 = 0;
    let mut x = x;
    if hx < 0x0010_0000 {
        if ((hx & 0x7fff_ffff) as u32 | lx) == 0 {
            return f64::NEG_INFINITY;
        }
        if hx < 0 {
            return f64::NAN;
        }
        k -= 54;
        x *= TWO54;
        hx = (x.to_bits() >> 32) as i32;
    }
    if hx >= 0x7ff0_0000 {
        return x + x;
    }
    k += (hx >> 20) - 1023;
    let i = ((k as u32) & 0x8000_0000) >> 31;
    hx = (hx & 0x000f_ffff) | (((0x3ff - i) << 20) as i32);
    let y = f64::from(k + i as i32);
    let x = f64::from_bits((u64::from(hx as u32) << 32) | (x.to_bits() & 0xffff_ffff));
    let z = y * LOG10_2LO + IVLN10 * js::ln(x);
    z + y * LOG10_2HI
}

fn conta_de_boss(nick: &str) -> bool {
    CONTAS_BOSS
        .iter()
        .any(|n| n.to_lowercase() == nick.to_lowercase())
}

fn bosses_nivel(chave: &str) -> Option<f64> {
    BOSSES_NIVEL
        .iter()
        .find(|(k, _)| *k == chave)
        .map(|(_, n)| *n)
}

/// `cfg(nick).rodizio`.
fn cfg_rodizio(d: &Value, nick: &str) -> Value {
    config::efetiva(d, nick)
        .get("rodizio")
        .cloned()
        .unwrap_or_else(|| json!({}))
}

fn pokemons(conta: &ContaViva) -> &[Value] {
    caminho(&conta.ultimo, &["hdx", "pokemons"])
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

static NULO: Value = Value::Null;

/// `conta.ultimo?.io?.player || {}`.
fn jogador(conta: &ContaViva) -> &Value {
    caminho(&conta.ultimo, &["io", "player"])
        .filter(|v| verd(Some(v)))
        .unwrap_or(&NULO)
}

fn automacao(conta: &ContaViva) -> Option<&Value> {
    caminho(&conta.ultimo, &["hdx", "eu", "automation"]).filter(|v| verd(Some(v)))
}

fn tipos_json(t: &[Tipo]) -> Value {
    Value::Array(t.iter().map(|x| json!(x.nome())).collect())
}

/// `BYN.get(nome.toLowerCase())`.
fn por_nome<'a>(dex: &'a Pokedex, nome: &str) -> Option<&'a EspecieCombate> {
    dex.por_nome(nome)
}

/// `BYD.get(v)`; com `coagir`, `BYD.get(+v)`.
fn por_d<'a>(dex: &'a Pokedex, v: Option<&Value>, coagir: bool) -> Option<&'a EspecieCombate> {
    let n = if coagir {
        js::mais(v)
    } else {
        match v {
            Some(Value::Number(x)) => x.as_f64()?,
            _ => return None,
        }
    };
    (n.is_finite() && n.fract() == 0.0)
        .then(|| dex.por_id(n as i64))
        .flatten()
}

/// `p.level || 1`.
fn lvl_ou_1(v: Option<&Value>) -> f64 {
    if verd(v) {
        js::mais(v)
    } else {
        1.0
    }
}

fn pot_u8(x: f64) -> u8 {
    if x.fract() == 0.0 && (0.0..=255.0).contains(&x) {
        x as u8
    } else {
        0
    }
}

/// `VER.ivArr(v)`.
fn iv_arr(v: Option<&Value>) -> Option<[f64; 6]> {
    match v? {
        Value::Array(a) if a.len() == 6 => {
            let mut r = [0.0; 6];
            for (i, x) in a.iter().enumerate() {
                r[i] = js::mais(Some(x));
            }
            Some(r)
        }
        Value::Object(o) => {
            let g = |ks: &[&str]| ks.iter().find_map(|k| o.get(*k).filter(|x| !x.is_null()));
            let c = [
                g(&["hp"]),
                g(&["atk", "ataque"]),
                g(&["def", "defesa"]),
                g(&["spAtk", "spatk", "spa"]),
                g(&["spDef", "spdef"]),
                g(&["speed", "spe", "spd"]),
            ];
            let mut r = [0.0; 6];
            for (i, x) in c.iter().enumerate() {
                let n = js::mais(Some((*x)?));
                if !n.is_finite() || n < 0.0 {
                    return None;
                }
                r[i] = n;
            }
            Some(r)
        }
        _ => None,
    }
}

/// `VER.potOf(p)`: primeiro dígito de `p.potencia ?? "1"`, ou 1.
fn pot_of(p: &Value) -> f64 {
    let s = match p.get("potencia") {
        None | Some(Value::Null) => "1".to_owned(),
        v => txt(v),
    };
    s.chars()
        .find(char::is_ascii_digit)
        .and_then(|c| c.to_digit(10))
        .map_or(1.0, f64::from)
}

/// IVs do `aguentaHuntDe` e do `huntParaNivelar`: `p.ivs || {}`; lista vira `Number`, objeto
/// vira `[hp, atk, def, spAtk, spDef, speed]` com `+x || 15`.
fn ivs_15(p: &Value) -> [f64; 6] {
    let v = p.get("ivs").filter(|x| verd(Some(x)));
    let mut r = [15.0; 6];
    match v {
        Some(Value::Array(a)) => {
            for (i, slot) in r.iter_mut().enumerate() {
                *slot = a.get(i).map_or(f64::NAN, |x| js::mais(Some(x)));
            }
        }
        other => {
            let ks = ["hp", "atk", "def", "spAtk", "spDef", "speed"];
            for (i, k) in ks.iter().enumerate() {
                let x = other.and_then(|o| o.get(*k));
                r[i] = ou(js::mais(x), 15.0);
            }
        }
    }
    r
}

/// `S_.setServerConstants(conta.pub)` sem estado global.
fn constantes(conta: &ContaViva) -> Constantes {
    let mut k = Constantes::wiki();
    let p = &conta.publico;
    if !verd(Some(p)) {
        return k;
    }
    let h = js::mais(p.get("multHpSelvagem"));
    if h > 0.0 {
        k.hp_selvagem = h;
    }
    let dn = js::mais(p.get("multDanoSelvagem"));
    if dn > 0.0 {
        k.dano_selvagem = dn;
    }
    if let Some(Value::Object(a)) = p.get("ampliacaoHunt") {
        for (ks, v) in a {
            let kk = js::mais(Some(&Value::String(ks.clone())));
            let vv = js::mais(Some(v));
            if kk.is_finite() && vv.is_finite() {
                if let Some(e) = k.ampliacao_hunt.iter_mut().find(|(c, _)| *c == kk) {
                    e.1 = vv;
                } else {
                    k.ampliacao_hunt.push((kk, vv));
                }
            }
        }
    }
    k
}

/// Combatente de um exemplar da coleção (`S_.makeMon(sp, p.level || 1, {...})`).
fn mon_do_poke(sp: &EspecieCombate, p: &Value, k: &Constantes, lvl: Option<f64>) -> Combatente {
    let op = Opcoes {
        qualidade: ou(js::mais(p.get("quality")), 1.1),
        ivs: iv_arr(p.get("ivs")).unwrap_or([16.0; 6]),
        potencia: pot_u8(pot_of(p)),
        shiny: verd(p.get("shiny")),
        ..Opcoes::default()
    };
    Combatente::novo(sp, lvl.unwrap_or_else(|| lvl_ou_1(p.get("level"))), &op, k)
}

fn mon_boss(sp: &EspecieCombate, nivel: f64, k: &Constantes) -> Combatente {
    Combatente::novo(
        sp,
        nivel,
        &Opcoes {
            qualidade: 1.3,
            ivs: [31.0; 6],
            potencia: 3,
            lendario: Some(true),
            ..Opcoes::default()
        },
        k,
    )
}

fn mon_selvagem(sp: &EspecieCombate, nivel: f64, k: &Constantes) -> Combatente {
    Combatente::novo(
        sp,
        nivel,
        &Opcoes {
            qualidade: 1.1,
            selvagem: true,
            ..Opcoes::default()
        },
        k,
    )
}

// ---------------------------------------------------------------------------------------
// Simulador: S_.fight e dpsOut
// ---------------------------------------------------------------------------------------

/// Resultado de `S_.fight`.
#[derive(Debug, Clone)]
struct Luta {
    kph: f64,
    tkill: Option<f64>,
    dps_in: f64,
    burst: f64,
    hp: f64,
    first_kill: Option<f64>,
    bate: f64,
    bate_t: Tipo,
    apanha: f64,
    apanha_t: Tipo,
}

/// `coverage(att, def)`: melhor efetividade e o tipo que a dá.
fn cobertura(att: &Combatente, def: &Combatente, k: &Constantes) -> Option<(f64, Tipo)> {
    let (t, e) = tipo_investida(att, def, k)?;
    let mut best = (e, t);
    for m in &att.golpes {
        let e = k.efetividade_hunt(m.tipo, &def.tipos);
        if e > best.0 {
            best = (e, m.tipo);
        }
    }
    Some(best)
}

/// Janela de golpes de uma luta: escolhe o de maior dano entre os prontos (empate fica com o
/// primeiro, como o `>` do legado).
fn melhor_pronto(dmg: &[f64], pronto: impl Fn(usize) -> bool) -> Option<usize> {
    let mut bi: Option<usize> = None;
    for i in 0..dmg.len() {
        if pronto(i) && bi.is_none_or(|b| dmg[i] > dmg[b]) {
            bi = Some(i);
        }
    }
    bi
}

/// `S_.fight(me, foe, secs, 0.1)`. `None` quando o legado lançaria exceção (combatente sem
/// tipo: `att.types[0]` indefinido).
fn lutar(me: &Combatente, foe: &Combatente, secs: f64, k: &Constantes) -> Option<Luta> {
    let mut ready: HashMap<&str, f64> = me.golpes.iter().map(|m| (m.nome.as_str(), 0.0)).collect();
    let dt = 0.1;
    let (mut t, mut kills, mut taken) = (0.0_f64, 0.0_f64, 0.0_f64);
    // `w`: (hp, ready, nxt, inv, born)
    let mut w: Option<(f64, Vec<f64>, f64, f64, f64)> = None;
    let (mut nxt_me, mut nxt_inv, mut max_burst) = (0.0_f64, 0.0_f64, 0.0_f64);
    let mut first_kill: Option<f64> = None;
    let mut win: VecDeque<(f64, f64)> = VecDeque::new();
    let mut win_sum = 0.0_f64;
    let my_dmg: Vec<f64> = me
        .golpes
        .iter()
        .map(|m| dano(me, foe, m.poder, m.categoria, m.tipo, me.stab(m.tipo), k))
        .collect();
    let foe_dmg: Vec<f64> = foe
        .golpes
        .iter()
        .map(|m| dano(foe, me, m.poder, m.categoria, m.tipo, foe.stab(m.tipo), k))
        .collect();
    tipo_investida(me, foe, k)?;
    tipo_investida(foe, me, k)?;
    let my_inv = dano_investida(me, foe, k);
    let foe_inv = dano_investida(foe, me, k);
    while t < secs {
        let wr = w.get_or_insert_with(|| (foe.hp, vec![t; foe.golpes.len()], t, t, t));
        if t >= nxt_me {
            let bi = melhor_pronto(&my_dmg, |i| {
                ready
                    .get(me.golpes[i].nome.as_str())
                    .copied()
                    .unwrap_or(f64::NAN)
                    <= t
            });
            if let Some(b) = bi {
                wr.0 -= my_dmg[b];
                let m = &me.golpes[b];
                ready.insert(m.nome.as_str(), t + m.recarga * me.mult_recarga);
            }
            nxt_me = t + me.intervalo;
        }
        if t >= nxt_inv {
            wr.0 -= my_inv;
            nxt_inv = t + me.intervalo_investida;
        }
        let mut hit = 0.0_f64;
        if t >= wr.2 {
            let bi = melhor_pronto(&foe_dmg, |i| wr.1[i] <= t);
            if let Some(b) = bi {
                hit += foe_dmg[b];
                wr.1[b] = t + foe.golpes[b].recarga * foe.mult_recarga;
            }
            wr.2 = t + foe.intervalo;
        }
        if t >= wr.3 {
            hit += foe_inv;
            wr.3 = t + foe.intervalo_investida;
        }
        taken += hit;
        win.push_back((t, hit));
        win_sum += hit;
        while win.front().is_some_and(|f| f.0 < t - 5.0) {
            if let Some(f) = win.pop_front() {
                win_sum -= f.1;
            }
        }
        if win_sum > max_burst {
            max_burst = win_sum;
        }
        if wr.0 <= 0.0 {
            kills += 1.0;
            if first_kill.is_none() {
                first_kill = Some(t - wr.4);
            }
            w = None;
        }
        t += dt;
    }
    let off = cobertura(me, foe, k)?;
    let dfn = cobertura(foe, me, k)?;
    Some(Luta {
        kph: kills * 3600.0 / secs,
        tkill: (kills != 0.0).then(|| secs / kills),
        dps_in: taken / secs,
        burst: max_burst,
        hp: me.hp,
        first_kill,
        bate: off.0,
        bate_t: off.1,
        apanha: dfn.0,
        apanha_t: dfn.1,
    })
}

/// `dpsOut(me, foe, secs)`: dano por segundo que `me` causa, sem apanhar.
fn dps_out(me: &Combatente, foe: &Combatente, secs: f64, k: &Constantes) -> Option<f64> {
    let secs = ou(secs, 120.0);
    let dt = 0.1;
    let mut ready: HashMap<&str, f64> = me.golpes.iter().map(|m| (m.nome.as_str(), 0.0)).collect();
    let (mut t, mut dealt, mut nxt_me, mut nxt_inv) = (0.0_f64, 0.0_f64, 0.0_f64, 0.0_f64);
    let my_dmg: Vec<f64> = me
        .golpes
        .iter()
        .map(|m| dano(me, foe, m.poder, m.categoria, m.tipo, me.stab(m.tipo), k))
        .collect();
    tipo_investida(me, foe, k)?;
    let my_inv = dano_investida(me, foe, k);
    while t < secs {
        if t >= nxt_me {
            let bi = melhor_pronto(&my_dmg, |i| {
                ready
                    .get(me.golpes[i].nome.as_str())
                    .copied()
                    .unwrap_or(f64::NAN)
                    <= t
            });
            if let Some(b) = bi {
                dealt += my_dmg[b];
                let m = &me.golpes[b];
                ready.insert(m.nome.as_str(), t + m.recarga * me.mult_recarga);
            }
            nxt_me = t + me.intervalo;
        }
        if t >= nxt_inv {
            dealt += my_inv;
            nxt_inv = t + me.intervalo_investida;
        }
        t += dt;
    }
    Some(dealt / secs)
}

/// `xpTotal(L)`.
fn xp_total(l: f64) -> f64 {
    js::round(50.0 / 3.0 * (js::pow(l, 3.0) - 6.0 * js::pow(l, 2.0) + 17.0 * l - 12.0))
}

/// `pontosBoss(r, superEfetivo)`.
fn pontos_boss(r: &Luta, super_efetivo: bool) -> f64 {
    let Some(fk) = r.first_kill else {
        return 0.0;
    };
    let pior = r.burst / r.hp;
    let sobrevive = 1.0 / (1.0 + (pior - 0.4).max(0.0) * 2.0);
    (3600.0 / fk.max(1.0)) * sobrevive * if super_efetivo { 1.3 } else { 1.0 }
}

/// `temVantagem(sp, bossTipos)`.
fn tem_vantagem(sp: &EspecieCombate, boss_tipos: &[Tipo]) -> bool {
    sp.golpes
        .iter()
        .any(|m| m.poder > 0.0 && efetividade_bruta(m.tipo, boss_tipos) > 1.0)
}

/// Nível do boss no `melhorTimePossivel` e no `sugerirBuild`:
/// `boss?.teamLevel || boss?.nivel || BOSSES_NIVEL[nome.toLowerCase()] || 300`.
fn nivel_do_boss(boss: Option<&Value>, nome: &str) -> f64 {
    ou_val(boss.and_then(|b| b.get("teamLevel")))
        .or_else(|| ou_val(boss.and_then(|b| b.get("nivel"))))
        .or_else(|| bosses_nivel(&nome.to_lowercase()))
        .unwrap_or(300.0)
}

fn boss_da_welcome<'a>(conta: &'a ContaViva, key: &str) -> Option<&'a Value> {
    conta
        .welcome
        .get("bosses")
        .and_then(Value::as_array)?
        .iter()
        .find(|b| b.get("key").and_then(Value::as_str) == Some(key))
}

// ---------------------------------------------------------------------------------------
// priorizarBosses
// ---------------------------------------------------------------------------------------

struct BossPri {
    key: Value,
    nome: Value,
    nivel_boss: f64,
    min_tr: f64,
    i: usize,
    feito: bool,
    score: f64,
    valor: f64,
    tem_token: bool,
    tem_nivel: bool,
    tipos: Value,
}

impl BossPri {
    fn json(&self, proximo: bool) -> Value {
        json!({
            "key": self.key, "nome": self.nome, "nivelBoss": jn(self.nivel_boss),
            "minTr": jn(self.min_tr), "i": self.i, "feito": self.feito, "score": jn(self.score),
            "valorPorVitoria": jn(self.valor), "temToken": self.tem_token,
            "temNivel": self.tem_nivel, "tipos": self.tipos,
            "semDrops": self.valor == 0.0 || self.valor.is_nan(),
            "proximo": proximo
        })
    }
}

/// Lista de bosses quando o painel ainda não chegou.
fn bosses_fallback() -> Vec<Value> {
    BOSSES_NIVEL
        .iter()
        .map(|(k, n)| {
            let mut nome = k.to_string();
            if let Some(p) = nome.get_mut(0..1) {
                p.make_ascii_uppercase();
            }
            json!({"key": k, "nome": nome, "nivel": n, "tipos": [], "drops": [],
                   "entrada": null, "fallback": true})
        })
        .collect()
}

/// `feito = !!(+v || (v && typeof v === 'object'))`.
fn boss_feito(v: Option<&Value>) -> bool {
    let n = js::mais(v);
    (n != 0.0 && !n.is_nan()) || matches!(v, Some(Value::Object(_) | Value::Array(_)))
}

/// `priorizarBosses(conta)`: lista na ordem de prioridade (a rota devolve `{lista}`).
pub fn priorizar_bosses(d: &Value, conta: &ContaViva) -> Value {
    let w = &conta.welcome;
    let tr_lv = ou(jogo::num(conta.ultimo.get("trLv")).unwrap_or(0.0), 1.0);
    let cfg = config::efetiva(d, &conta.nick);
    let g = cfg.get("gemaPrecos").filter(|v| verd(Some(v)));
    let g_ou = |k: &str, padrao: f64| ou_val(g.and_then(|x| x.get(k))).unwrap_or(padrao);
    let itens = caminho(&conta.ultimo, &["hdx", "eu", "items"])
        .filter(|v| verd(Some(v)))
        .unwrap_or(&NULO);
    let lista_w = w
        .get("bosses")
        .and_then(Value::as_array)
        .filter(|a| !a.is_empty());
    let fallback;
    let bosses_lista: &[Value] = match lista_w {
        Some(a) => a,
        None => {
            fallback = bosses_fallback();
            &fallback
        }
    };
    let precos = caminho(&conta.ultimo, &["hdx", "precosMercado"]);
    let preco_gemas = |id: Option<&Value>| -> f64 {
        let chave = txt(id);
        let por_moeda = precos
            .and_then(|p| p.get("itens"))
            .and_then(|i| i.get(&chave))
            .filter(|v| verd(Some(v)));
        if let Some(pm) = por_moeda {
            let em_gema = ["orb", "gema", "orbs"]
                .iter()
                .find_map(|k| pm.get(*k).filter(|v| verd(Some(v))));
            if js::mais(em_gema) > 0.0 {
                return js::mais(em_gema);
            }
        }
        let nome = w
            .get("itens")
            .and_then(Value::as_array)
            .and_then(|a| a.iter().find(|i| igual_estrito(i.get("id"), id)))
            .and_then(|i| i.get("nome"))
            .filter(|v| verd(Some(v)))
            .map(|v| txt(Some(v)))
            .unwrap_or_default();
        let nm = nome.to_lowercase();
        if nm.contains("bronze") || nm.contains("token") {
            return g_ou("bronzeToken", 300.0);
        }
        if nm.contains("chave") || nm.contains("key") {
            return g_ou("fragChave", 5.0);
        }
        if nm.contains("shiny stone") {
            return g_ou("fragShinyStone", 5.0);
        }
        if nm.contains("bicicleta") || nm.contains("bike") {
            return g_ou("fragBicicleta", 5.0);
        }
        10.0
    };
    let vit = jogador(conta)
        .get("bossVit")
        .filter(|v| verd(Some(v)))
        .unwrap_or(&NULO);
    let mut lista: Vec<BossPri> = Vec::new();
    for (i, b) in bosses_lista.iter().enumerate() {
        let nome_b = if verd(b.get("nome")) {
            b.get("nome")
        } else {
            b.get("key")
        };
        let nivel_boss = ou_val(b.get("teamLevel"))
            .or_else(|| ou_val(b.get("nivel")))
            .or_else(|| bosses_nivel(&txt(nome_b).to_lowercase()))
            .unwrap_or(300.0);
        let min_tr = ou_val(b.get("minTr")).unwrap_or(nivel_boss);
        let entrada = b.get("entrada").filter(|v| verd(Some(v)));
        let tem_token = match entrada.filter(|e| verd(e.get("itemId"))) {
            Some(e) => {
                let tem = n0(itens.get(txt(e.get("itemId"))));
                tem >= ou(js::mais(e.get("qtd")), 1.0)
            }
            None => true,
        };
        let tem_nivel = tr_lv >= min_tr;
        let mut valor = 0.0_f64;
        if let Some(Value::Array(drops)) = b.get("drops") {
            for dr in drops {
                valor += n0(dr.get("chance")) * preco_gemas(dr.get("itemId"));
            }
        }
        let piso = log10(10.0_f64.max(nivel_boss)) * 5.0;
        let key_txt = b.get("key").map(|k| txt(Some(k))).unwrap_or_default();
        lista.push(BossPri {
            key: b.get("key").cloned().unwrap_or(Value::Null),
            nome: nome_b.cloned().unwrap_or(Value::Null),
            nivel_boss,
            min_tr,
            i,
            feito: boss_feito(vit.get(&key_txt)),
            score: ou(valor, piso)
                * if tem_nivel { 1.0 } else { 0.1 }
                * if tem_token { 1.0 } else { 0.3 },
            valor,
            tem_token,
            tem_nivel,
            tipos: b
                .get("tipos")
                .filter(|v| verd(Some(v)))
                .cloned()
                .unwrap_or_else(|| json!([])),
        });
    }
    let mut pend: Vec<&BossPri> = lista.iter().filter(|b| !b.feito).collect();
    let mut feitos: Vec<&BossPri> = lista.iter().filter(|b| b.feito).collect();
    ordenar(&mut pend, |a, b| {
        ou(a.nivel_boss - b.nivel_boss, a.i as f64 - b.i as f64)
    });
    ordenar(&mut feitos, |a, b| b.score - a.score);
    let todos: Vec<&BossPri> = pend.into_iter().chain(feitos).collect();
    Value::Array(
        todos
            .iter()
            .enumerate()
            .map(|(n, b)| b.json(n == 0 && !b.feito))
            .collect(),
    )
}

// ---------------------------------------------------------------------------------------
// melhorTimePossivel e sugerirBuild
// ---------------------------------------------------------------------------------------

struct Avaliado<'a> {
    p: &'a Value,
    sp: &'a EspecieCombate,
    pts: f64,
    pior: Option<f64>,
    tempo: Option<f64>,
    local: &'static str,
}

fn local_de(p: &Value, pl: &Value, trav: Option<&HashSet<u64>>) -> &'static str {
    if !nulo(p.get("slot")) || igual_estrito(p.get("id"), pl.get("activeId")) {
        "equipe"
    } else if trav.is_some_and(|t| t.contains(&js::mais(p.get("id")).to_bits())) {
        "colecao"
    } else {
        "depot"
    }
}

fn travados(pl: &Value) -> Option<HashSet<u64>> {
    let t = pl.get("travados").filter(|v| verd(Some(v)))?;
    Some(
        t.as_array()?
            .iter()
            .map(|x| js::mais(Some(x)).to_bits())
            .collect(),
    )
}

/// O `pokes.map(...).filter(pts > 0).sort(...).slice(0, 6)` comum a `melhorTimePossivel` e
/// `sugerirBuild`.
fn avaliar_exemplares<'a>(
    dex: &'a Pokedex,
    pokes: &'a [Value],
    alvo: &Combatente,
    boss_tipos: &[Tipo],
    k: &Constantes,
    local: impl Fn(&Value) -> &'static str,
) -> Vec<Avaliado<'a>> {
    let mut v: Vec<Avaliado<'a>> = Vec::new();
    for p in pokes {
        let Some(sp) = por_d(dex, p.get("speciesId"), false) else {
            continue;
        };
        let me = mon_do_poke(sp, p, k, None);
        let Some(r) = lutar(&me, alvo, JANELA_BOSS, k) else {
            continue;
        };
        let pts = pontos_boss(&r, tem_vantagem(sp, boss_tipos));
        if pts > 0.0 {
            v.push(Avaliado {
                p,
                sp,
                pts,
                pior: Some(r.burst / r.hp),
                tempo: Some(r.first_kill.unwrap_or(1e9)),
                local: local(p),
            });
        }
    }
    ordenar(&mut v, |a, b| b.pts - a.pts);
    v.truncate(6);
    v
}

/// Fallback sem simulação: vantagem de tipo, depois nível.
fn sem_simulacao<'a>(
    dex: &'a Pokedex,
    pokes: &'a [Value],
    boss_tipos: &[Tipo],
    local: impl Fn(&Value) -> &'static str,
) -> Vec<Avaliado<'a>> {
    let mut v: Vec<(Avaliado<'a>, f64)> = pokes
        .iter()
        .filter_map(|p| {
            let sp = por_d(dex, p.get("speciesId"), false)?;
            Some((
                Avaliado {
                    p,
                    sp,
                    pts: 0.0,
                    pior: None,
                    tempo: None,
                    local: local(p),
                },
                f64::from(u8::from(tem_vantagem(sp, boss_tipos))),
            ))
        })
        .collect();
    ordenar(&mut v, |a, b| {
        ou(b.1 - a.1, n0(b.0.p.get("level")) - n0(a.0.p.get("level")))
    });
    v.truncate(6);
    v.into_iter().map(|x| x.0).collect()
}

/// `{id, nome, level}` de um exemplar (`id` e `level` somem quando `undefined`).
fn ficha_basica(p: &Value, sp: &EspecieCombate) -> Map<String, Value> {
    let mut m = Map::new();
    if let Some(i) = p.get("id") {
        m.insert("id".into(), i.clone());
    }
    m.insert("nome".into(), json!(sp.nome));
    if let Some(l) = p.get("level") {
        m.insert("level".into(), l.clone());
    }
    m
}

/// `melhorTimePossivel(conta, bossKey)`: `Value::Null` quando o legado devolve `null`.
pub fn melhor_time_possivel(
    d: &Value,
    conta: &ContaViva,
    dex: &Pokedex,
    boss_key: Option<&str>,
) -> Value {
    let pokes = pokemons(conta);
    if pokes.is_empty() {
        return Value::Null;
    }
    let key: String = match boss_key.filter(|k| !k.is_empty()) {
        Some(k) => k.to_owned(),
        None => {
            let prio = priorizar_bosses(d, conta);
            let Some(arr) = prio.as_array() else {
                return Value::Null;
            };
            let Some(prox) = arr
                .iter()
                .find(|b| b.get("feito") == Some(&Value::Bool(false)))
                .or_else(|| arr.first())
            else {
                return Value::Null;
            };
            txt(prox.get("key"))
        }
    };
    let boss = boss_da_welcome(conta, &key);
    let nome_boss = boss
        .and_then(|b| b.get("nome"))
        .filter(|v| verd(Some(v)))
        .map_or_else(|| key.clone(), |v| txt(Some(v)));
    let nivel_boss = nivel_do_boss(boss, &nome_boss);
    let Some(boss_sp) = por_nome(dex, &nome_boss) else {
        return Value::Null;
    };
    let k = constantes(conta);
    let pl = jogador(conta);
    let trav = travados(pl);
    let local = |p: &Value| local_de(p, pl, trav.as_ref());
    let alvo = mon_boss(boss_sp, nivel_boss, &k);
    let boss_tipos = &boss_sp.tipos;
    let avaliados = avaliar_exemplares(dex, pokes, &alvo, boss_tipos, &k, local);
    let semsim = avaliados.is_empty();
    let lista = if semsim {
        sem_simulacao(dex, pokes, boss_tipos, local)
    } else {
        avaliados
    };
    let mut pts_atual = 0.0_f64;
    for p in pokes.iter().filter(|p| local(p) == "equipe") {
        let Some(sp) = por_d(dex, p.get("speciesId"), false) else {
            continue;
        };
        let me = mon_do_poke(sp, p, &k, None);
        if let Some(r) = lutar(&me, &alvo, JANELA_BOSS, &k) {
            pts_atual += pontos_boss(&r, tem_vantagem(sp, boss_tipos));
        }
    }
    let pts_melhor = lista.iter().fold(0.0, |s, x| s + x.pts);
    let ganho = (pts_atual > 0.0).then(|| js::round((pts_melhor / pts_atual - 1.0) * 100.0));
    let (mut e, mut c, mut dp) = (0, 0, 0);
    for p in pokes {
        match local(p) {
            "equipe" => e += 1,
            "colecao" => c += 1,
            _ => dp += 1,
        }
    }
    let time: Vec<Value> = lista
        .iter()
        .map(|x| {
            let mut m = ficha_basica(x.p, x.sp);
            m.insert("local".into(), json!(x.local));
            let pontos = if x.pts != 0.0 && !x.pts.is_nan() {
                jn(js::round(x.pts))
            } else {
                json!(0)
            };
            m.insert("pontos".into(), pontos);
            let tempo = match x.tempo {
                Some(t) if t < 1e9 => jn(js::round(t)),
                _ => Value::Null,
            };
            m.insert("tempo".into(), tempo);
            m.insert(
                "piorPct".into(),
                jn_ou_nulo(x.pior.map(|p| js::round(p * 100.0))),
            );
            m.insert("tipos".into(), tipos_json(&x.sp.tipos));
            m.insert("shiny".into(), json!(verd(x.p.get("shiny"))));
            Value::Object(m)
        })
        .collect();
    json!({
        "bossKey": key, "nomeBoss": nome_boss, "nivelBoss": jn(nivel_boss),
        "totalPokemon": pokes.len(), "totalPorLocal": {"equipe": e, "colecao": c, "depot": dp},
        "ganhoEstimado": jn_ou_nulo(ganho), "semSimulacao": semsim, "time": time
    })
}

/// `sugerirBuild(conta, bossKey)`: lista (ou `Value::Null`, como o legado sem conta/boss).
pub fn sugerir_build(conta: &ContaViva, dex: &Pokedex, boss_key: Option<&str>) -> Value {
    let Some(key) = boss_key.filter(|k| !k.is_empty()) else {
        return Value::Null;
    };
    let pokes = pokemons(conta);
    if pokes.is_empty() {
        return json!([]);
    }
    let boss = boss_da_welcome(conta, key);
    let nome_boss = boss
        .and_then(|b| b.get("nome"))
        .filter(|v| verd(Some(v)))
        .map_or_else(|| key.to_owned(), |v| txt(Some(v)));
    let nivel_boss = nivel_do_boss(boss, &nome_boss);
    let Some(boss_sp) = por_nome(dex, &nome_boss) else {
        return json!([]);
    };
    let k = constantes(conta);
    let alvo = mon_boss(boss_sp, nivel_boss, &k);
    let boss_tipos = &boss_sp.tipos;
    let avaliados = avaliar_exemplares(dex, pokes, &alvo, boss_tipos, &k, |_| "");
    if avaliados.is_empty() {
        let ordem = sem_simulacao(dex, pokes, boss_tipos, |_| "");
        return Value::Array(
            ordem
                .iter()
                .map(|x| {
                    let mut m = ficha_basica(x.p, x.sp);
                    m.insert("pontos".into(), json!(0));
                    m.insert("tempo".into(), Value::Null);
                    m.insert("piorPct".into(), Value::Null);
                    m.insert("semSimulacao".into(), json!(true));
                    Value::Object(m)
                })
                .collect(),
        );
    }
    Value::Array(
        avaliados
            .iter()
            .map(|x| {
                let mut m = ficha_basica(x.p, x.sp);
                m.insert("pontos".into(), jn(js::round(x.pts)));
                m.insert("tempo".into(), jn_ou_nulo(x.tempo.map(js::round)));
                m.insert(
                    "piorPct".into(),
                    jn_ou_nulo(x.pior.map(|p| js::round(p * 100.0))),
                );
                Value::Object(m)
            })
            .collect(),
    )
}

// ---------------------------------------------------------------------------------------
// combateDe
// ---------------------------------------------------------------------------------------

const STAT_NOMES: [&str; 6] = ["hp", "atk", "def", "spa", "spd", "spe"];

fn atr(a: &crate::combate::combatente::Atributos) -> [f64; 6] {
    [a.hp, a.atk, a.def, a.spa, a.spd, a.spe]
}

/// `ativoDe(conta)`: espécie, nível, qualidade, potência e IVs do pokémon ativo.
struct AtivoDe<'a> {
    sp: &'a EspecieCombate,
    lvl: f64,
    q: f64,
    pot: f64,
    iv: [f64; 6],
}

fn ativo_de<'a>(conta: &ContaViva, dex: &'a Pokedex) -> Option<AtivoDe<'a>> {
    let u = &conta.ultimo;
    let a = u.get("ativo").filter(|v| verd(Some(v)))?;
    let nome = if verd(a.get("nome")) {
        txt(a.get("nome"))
    } else {
        String::new()
    };
    let sp = por_nome(dex, &nome)?;
    let v = caminho(u, &["io", "active", "ivs"]);
    let mut iv: Option<[f64; 6]> = None;
    match v {
        Some(Value::Array(x)) if x.len() == 6 => {
            let mut r = [0.0; 6];
            for (i, e) in x.iter().enumerate() {
                r[i] = js::mais(Some(e));
            }
            iv = Some(r);
        }
        Some(o @ Value::Object(_)) => {
            let p = |ks: &[&str]| -> Option<f64> {
                ks.iter()
                    .find_map(|k| o.get(*k).filter(|x| !x.is_null()))
                    .map(|x| js::mais(Some(x)))
            };
            let r = [
                p(&["hp", "HP"]),
                p(&["atk", "ataque", "attack"]),
                p(&["def", "defesa", "defense"]),
                p(&["spatk", "spa", "ataqueEspecial", "spAtk", "sp_atk"]),
                p(&["spdef", "spd_def", "defesaEspecial", "spDef", "sp_def"]),
                p(&["spe", "speed", "velocidade", "vel"]),
            ];
            if r.iter().all(|x| x.is_some_and(|y| y > 0.0)) {
                let mut f = [0.0; 6];
                for (i, x) in r.iter().enumerate() {
                    f[i] = x.unwrap_or(0.0);
                }
                iv = Some(f);
            }
        }
        _ => {}
    }
    let iv = iv.unwrap_or_else(|| {
        let e = 1.0_f64.max(32.0_f64.min(js::round(
            ou(jogo::num(a.get("iv")).unwrap_or(0.0), 96.0) / 6.0,
        )));
        [e; 6]
    });
    Some(AtivoDe {
        sp,
        lvl: jogo::num(a.get("lv")).unwrap_or(0.0),
        q: ou(jogo::num(a.get("q")).unwrap_or(0.0), 1.1),
        pot: ou(jogo::num(a.get("pot")).unwrap_or(0.0), 1.0),
        iv,
    })
}

/// `par(t)`: `"a/b"` em `{a, b}` (cada um `num(x) || 0`).
fn par(t: Option<&Value>) -> (f64, f64) {
    let s = if verd(t) { txt(t) } else { String::new() };
    let mut it = s.split('/').map(jogo::num_texto);
    let a = it.next().flatten().unwrap_or(0.0);
    let b = it.next().flatten().unwrap_or(0.0);
    (ou(a, 0.0), ou(b, 0.0))
}

struct Aval {
    nome: String,
    r: Luta,
    dps: f64,
    vive: f64,
}

/// `combateDe(conta)`: a tela de combate. `agora` é o `Date.now()`; `sessao_tomada` é
/// `conta.sessaoTomada` (que `ContaViva` não guarda).
pub fn combate_de(
    d: &Value,
    r: &Relogio,
    conta: &ContaViva,
    dex: &Pokedex,
    sessao_tomada: bool,
) -> Value {
    let u = &conta.ultimo;
    if !verd(Some(u)) {
        return json!({"erro": "Ainda sem leitura do jogo."});
    }
    let k = constantes(conta);
    let agora = r.agora_ms as f64;
    let at = ativo_de(conta, dex);
    let h = jogo::hunt_atual(conta);
    let tr_lv = jogo::num(u.get("trLv")).unwrap_or(0.0);
    let tr_lv = ou(tr_lv, 0.0);
    let ativo_js = u.get("ativo").filter(|v| verd(Some(v)));
    let hp = par(ativo_js.and_then(|a| a.get("hp")));
    let xp = par(ativo_js.and_then(|a| a.get("xp")));
    let num_n = |v: Option<&Value>| jn_ou_nulo(jogo::num(v));
    let mut out = Map::new();
    out.insert("nick".into(), json!(conta.nick));
    out.insert("t".into(), jn(agora));
    out.insert("trLv".into(), jn(tr_lv));
    out.insert(
        "hunt".into(),
        h.as_ref().map_or(
            Value::Null,
            |(n, l)| json!({"nome": n, "lvl": jn_ou_nulo(*l)}),
        ),
    );
    out.insert("sessaoTomada".into(), json!(sessao_tomada));
    out.insert(
        "limiar".into(),
        ou_val(Some(&jn(jogo::num(u.get("limiar")).unwrap_or(0.0)))).map_or(Value::Null, jn),
    );
    let mut ativo_m = ativo_js.map(|a| {
        let mut m = Map::new();
        m.insert("nome".into(), a.get("nome").cloned().unwrap_or(Value::Null));
        m.insert("lvl".into(), num_n(a.get("lv")));
        m.insert("q".into(), num_n(a.get("q")));
        m.insert(
            "pot".into(),
            jn(ou(jogo::num(a.get("pot")).unwrap_or(0.0), 1.0)),
        );
        m.insert("ivTotal".into(), num_n(a.get("iv")));
        m.insert("hp".into(), json!({"a": jn(hp.0), "b": jn(hp.1)}));
        m.insert("xp".into(), json!({"a": jn(xp.0), "b": jn(xp.1)}));
        m
    });
    out.insert(
        "ativo".into(),
        ativo_m.clone().map_or(Value::Null, Value::Object),
    );
    // 1) stats do exemplar contra o exemplar médio da espécie no mesmo nível
    if let (Some(at), Some(am)) = (&at, ativo_m.as_mut()) {
        let me = Combatente::novo(
            at.sp,
            at.lvl,
            &Opcoes {
                qualidade: at.q,
                ivs: at.iv,
                potencia: pot_u8(at.pot),
                ..Opcoes::default()
            },
            &k,
        );
        let med = Combatente::novo(
            at.sp,
            at.lvl,
            &Opcoes {
                qualidade: 1.3,
                ivs: [16.0; 6],
                potencia: 1,
                ..Opcoes::default()
            },
            &k,
        );
        am.insert("tipos".into(), tipos_json(&at.sp.tipos));
        am.insert(
            "iv".into(),
            Value::Array(at.iv.iter().map(|x| jn(*x)).collect()),
        );
        am.insert("hpMaxSim".into(), jn(me.hp));
        out.insert("ativo".into(), Value::Object(am.clone()));
        let (sme, smed) = (atr(&me.st), atr(&med.st));
        let stats: Vec<Value> = (0..6)
            .map(|i| {
                json!({
                    "k": STAT_NOMES[i], "base": jn(at.sp.base[i]), "iv": jn(at.iv[i]),
                    "v": jn(sme[i]), "medio": jn(smed[i]),
                    "pct": if smed[i] != 0.0 && !smed[i].is_nan() {
                        jn(js::round((sme[i] / smed[i] - 1.0) * 100.0))
                    } else { Value::Null }
                })
            })
            .collect();
        out.insert("stats".into(), Value::Array(stats));
        let soma = |a: &[f64; 6]| a.iter().fold(0.0, |x, y| x + y);
        out.insert(
            "vsMedio".into(),
            jn(js::round((soma(&sme) / soma(&smed) - 1.0) * 100.0)),
        );
        // 2) a luta de agora
        if let Some((hn, hl)) = &h {
            if let Some(w) = por_nome(dex, hn) {
                let foe = mon_selvagem(w, hl.unwrap_or(0.0), &k);
                if let (Some(rr), Some(dps)) =
                    (lutar(&me, &foe, 150.0, &k), dps_out(&me, &foe, 60.0, &k))
                {
                    out.insert(
                        "luta".into(),
                        json!({
                            "inimigo": w.nome, "tipos": tipos_json(&w.tipos), "lvl": jn_ou_nulo(*hl),
                            "hpInimigo": jn(js::round(foe.hp)), "kphSim": jn(js::round(rr.kph)),
                            "tKill": match rr.tkill { Some(t) if t != 0.0 => jn(js::round(t * 10.0) / 10.0), _ => Value::Null },
                            "pior5s": jn(js::round(rr.burst / rr.hp * 100.0)),
                            "dpsIn": jn(js::round(rr.dps_in)), "dpsOut": jn(js::round(dps)),
                            "bate": jn(rr.bate), "bateT": rr.bate_t.nome(),
                            "apanha": jn(rr.apanha), "apanhaT": rr.apanha_t.nome()
                        }),
                    );
                }
            }
        }
    }
    // 3) sessão
    let corte = (conta.sessao_inicio as f64).max(agora - 6.0 * 3.6e6);
    let linhas: Vec<&(i64, i64, String)> = conta
        .linhas
        .iter()
        .filter(|l| l.1 as f64 >= corte)
        .collect();
    let desde = linhas.first().map_or(agora, |l| corte.max(l.1 as f64));
    let (
        mut kills,
        mut xp_s,
        mut ouro,
        mut pocoes,
        mut bolas,
        mut capturas,
        mut fugiram,
        mut desmaios,
    ) = (0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
    let mut ts: Vec<f64> = Vec::new();
    for (_, t, x) in linhas.iter().copied() {
        if let Some(m) = ABATE.as_ref().and_then(|re| re.captures(x)) {
            kills += 1.0;
            ts.push(*t as f64);
            xp_s += m
                .get(2)
                .and_then(|g| jogo::num_texto(g.as_str()))
                .unwrap_or(0.0);
            ouro += m
                .get(4)
                .and_then(|g| jogo::num_texto(g.as_str()))
                .unwrap_or(0.0);
            continue;
        }
        if casa(&FUGA, x) {
            bolas += 1.0;
            fugiram += 1.0;
        } else if casa(&CAPTURA, x) {
            bolas += 1.0;
            capturas += 1.0;
        } else if casa(&POCAO, x) {
            pocoes += 1.0;
        } else if casa(&DESMAIO, x) {
            desmaios += 1.0;
        }
    }
    let kph = if ts.len() >= 3 && agora - desde >= 60e3 {
        jn(js::round(ts.len() as f64 * 3.6e6 / (agora - desde)))
    } else {
        Value::Null
    };
    let eventos: Vec<Value> = linhas
        .iter()
        .rev()
        .take(14)
        .map(|(_, t, x)| json!([t, x]))
        .collect();
    out.insert(
        "sessao".into(),
        json!({"desde": jn(desde), "kills": jn(kills), "xp": jn(xp_s), "ouro": jn(ouro),
               "pocoes": jn(pocoes), "bolas": jn(bolas), "capturas": jn(capturas),
               "fugiram": jn(fugiram), "desmaios": jn(desmaios), "eventos": eventos, "kph": kph}),
    );
    // o `ses.kph` do legado nasce depois de `eventos`? Não: `kph` entra no literal antes.
    // 3b) ritmo
    let ativo_lvl = ativo_m
        .as_ref()
        .and_then(|a| a.get("lvl"))
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    if ativo_m.is_some() && ativo_lvl != 0.0 {
        let ini = agora - 30.0 * 60e3;
        let (mut xp_pk, mut t0) = (0.0_f64, None::<f64>);
        for (_, t, x) in conta.linhas.iter().filter(|l| l.1 as f64 >= ini.max(corte)) {
            if let Some(m) = ABATE.as_ref().and_then(|re| re.captures(x)) {
                xp_pk += m
                    .get(3)
                    .and_then(|g| jogo::num_texto(g.as_str()))
                    .unwrap_or(0.0);
                t0 = t0.or(Some(*t as f64));
            }
        }
        let primeira = conta.linhas.first().map_or(ini, |l| l.1 as f64);
        let ini_r = ini.max(corte).max(primeira);
        let horas = if t0.is_some() {
            (agora - ini_r) / 3.6e6
        } else {
            0.0
        };
        let mut xp_h: Option<f64> = (horas >= 1.0 / 30.0).then(|| xp_pk / horas);
        let mut origem = "nexus";
        if xp_h.is_none() {
            let eco = if verd(u.get("eco")) {
                txt(u.get("eco"))
            } else {
                String::new()
            };
            let lg: Vec<&str> = eco
                .split('\n')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .collect();
            let i = lg.iter().position(|x| casa(&XP_POKE, x));
            let ph = i.and_then(|i| {
                lg.iter()
                    .skip(i + 1)
                    .take(3)
                    .find(|x| casa(&POR_HORA, x))
                    .copied()
            });
            let v = ph.and_then(|ph| {
                jogo::num_texto(ph).map(|n| {
                    n * if casa(&MI, ph) {
                        1e6
                    } else if casa(&MIL, ph) {
                        1e3
                    } else {
                        1.0
                    }
                })
            });
            if let Some(v) = v.filter(|v| *v > 0.0) {
                xp_h = Some(v);
                origem = "jogo";
            }
        }
        let lv = ativo_lvl;
        let xp_agora = xp_total(lv) + ou(xp.0, 0.0);
        let falta = |alvo: f64| (xp_total(alvo) - xp_agora).max(0.0);
        let eta = |alvo: f64| match xp_h {
            Some(x) if x != 0.0 && !x.is_nan() => jn(falta(alvo) / x),
            _ => Value::Null,
        };
        let mut meta: Option<f64> = None;
        let mut fonte: Option<String> = None;
        if conta_de_boss(&conta.nick) {
            let rr = plano_rodizio(d, conta, dex);
            let e = rr
                .get("equipe")
                .and_then(Value::as_array)
                .and_then(|a| a.iter().find(|x| verd(x.get("ativo"))));
            if let Some(m) = e.and_then(|e| e.get("meta")).filter(|m| verd(Some(m))) {
                meta = Some(js::mais(Some(m)));
                let nome = rr
                    .pointer("/boss/nome")
                    .filter(|v| verd(Some(v)))
                    .map_or_else(|| "boss".to_owned(), |v| txt(Some(v)));
                fonte = Some(format!("meta do rodízio para o {nome}"));
            }
        }
        if meta.is_none() && tr_lv != 0.0 {
            meta = Some(tr_lv + 5.0);
            fonte = Some("nível máximo hoje (treinador + 5)".to_owned());
        }
        let meta_j = match meta {
            Some(m) if m > lv => {
                json!({"nv": jn(m), "fonte": fonte, "falta": jn(falta(m)), "horas": eta(m)})
            }
            Some(m) => json!({"nv": jn(m), "fonte": fonte, "atingida": true}),
            None => Value::Null,
        };
        out.insert(
            "ritmo".into(),
            json!({
                "xpH": jn_ou_nulo(xp_h.map(js::round)), "origem": origem,
                "amostraMin": jn(js::round(horas * 60.0)),
                "proximo": {"nv": jn(lv + 1.0), "falta": jn(falta(lv + 1.0)), "horas": eta(lv + 1.0)},
                "meta": meta_j
            }),
        );
    }
    let chip = |arr: Option<&Value>| -> Value {
        Value::Array(
            arr.and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .map(|c| {
                            json!({"nome": c.get("nome").cloned().unwrap_or(Value::Null),
                                   "q": jn(ou(jogo::num(c.get("q")).unwrap_or(0.0), 0.0)), "on": verd(c.get("on"))})
                        })
                        .collect()
                })
                .unwrap_or_default(),
        )
    };
    out.insert("pocoes".into(), chip(u.get("pocoes")));
    out.insert("bolas".into(), chip(u.get("bolas")));
    let mut precos = Map::new();
    for lista in ["bolas", "itens"] {
        if let Some(a) = conta.welcome.get(lista).and_then(Value::as_array) {
            for x in a {
                if verd(x.get("nome")) && js::mais(x.get("preco")) > 0.0 {
                    precos.insert(
                        txt(x.get("nome")),
                        x.get("preco").cloned().unwrap_or(Value::Null),
                    );
                }
            }
        }
    }
    out.insert("precos".into(), Value::Object(precos));
    // 4) boss
    if !conta_de_boss(&conta.nick) {
        out.insert("boss".into(), json!({"so": "farm"}));
        return Value::Object(out);
    }
    out.insert("boss".into(), boss_teorico(d, conta, dex, &k));
    Value::Object(out)
}

/// O bloco `out.boss` de `combateDe` (o `try` inteiro).
fn boss_teorico(d: &Value, conta: &ContaViva, dex: &Pokedex, k: &Constantes) -> Value {
    let u = &conta.ultimo;
    let falhou =
        || json!({"erro": "Falhou o cálculo do boss: Cannot read properties of undefined"});
    let rr = plano_rodizio(d, conta, dex);
    if verd(rr.get("bloqueio")) && !verd(rr.get("boss")) {
        return json!({"erro": rr.get("bloqueio").cloned().unwrap_or(Value::Null)});
    }
    let boss_nome_v = rr.pointer("/boss/nome");
    let boss_nome = txt(boss_nome_v);
    let Some(boss_sp) = por_nome(dex, &boss_nome) else {
        return json!({"erro": "Não achei o boss na Dex."});
    };
    let nivel_boss = ou_val(rr.pointer("/boss/teamLevel"))
        .or_else(|| bosses_nivel(&boss_nome.to_lowercase()))
        .unwrap_or(300.0);
    let alvo = mon_boss(boss_sp, nivel_boss, k);
    let pokes = pokemons(conta);
    let pl = jogador(conta);
    let ativo_id = js::mais(pl.get("activeId"));
    let mut time: Vec<&Value> = pokes
        .iter()
        .filter(|p| !nulo(p.get("slot")) || js::mais(p.get("id")) == ativo_id)
        .collect();
    let slot_de = |p: &Value| match p.get("slot") {
        None | Some(Value::Null) => -1.0,
        s => js::mais(s),
    };
    ordenar(&mut time, |a, b| slot_de(a) - slot_de(b));
    time.truncate(6);
    let avalia = |p: &Value, lvl: Option<f64>| -> Result<Option<Aval>, ()> {
        let Some(sp) = por_d(dex, p.get("speciesId"), true) else {
            return Ok(None);
        };
        let l = ou(lvl.unwrap_or(0.0), ou(js::mais(p.get("level")), 1.0));
        let me = mon_do_poke(sp, p, k, Some(l));
        let r = lutar(&me, &alvo, 120.0, k).ok_or(())?;
        let dps = dps_out(&me, &alvo, 120.0, k).ok_or(())?;
        let vive = if r.dps_in > 0.0 {
            me.hp / r.dps_in
        } else {
            f64::INFINITY
        };
        Ok(Some(Aval {
            nome: sp.nome.clone(),
            r,
            dps,
            vive,
        }))
    };
    let somar = |lista: &[Option<Aval>]| -> (f64, f64) {
        let (mut t, mut dano) = (0.0_f64, 0.0_f64);
        for x in lista.iter().flatten() {
            if t >= JANELA_BOSS {
                continue;
            }
            let dt = x.vive.min(JANELA_BOSS - t);
            dano += x.dps * dt;
            t += dt;
        }
        (dano, t)
    };
    let mut aval: Vec<Option<Aval>> = Vec::new();
    for p in &time {
        match avalia(p, None) {
            Ok(a) => aval.push(a),
            Err(()) => return falhou(),
        }
    }
    let membros: Vec<Value> = time
        .iter()
        .zip(&aval)
        .filter_map(|(p, x)| {
            let x = x.as_ref()?;
            let mut m = Map::new();
            m.insert("id".into(), p.get("id").cloned().unwrap_or(Value::Null));
            m.insert("nome".into(), json!(x.nome));
            m.insert("lvl".into(), jn(n0(p.get("level"))));
            m.insert("ativo".into(), json!(js::mais(p.get("id")) == ativo_id));
            m.insert("dps".into(), jn(js::round(x.dps * 10.0) / 10.0));
            m.insert(
                "vive".into(),
                if x.vive.is_finite() {
                    jn(js::round(x.vive * 10.0) / 10.0)
                } else {
                    Value::Null
                },
            );
            m.insert(
                "dano".into(),
                jn(js::round(x.dps * x.vive.min(JANELA_BOSS))),
            );
            m.insert("pior5s".into(), jn(js::round(x.r.burst / x.r.hp * 100.0)));
            m.insert("bate".into(), jn(x.r.bate));
            Some(Value::Object(m))
        })
        .collect();
    let tot = somar(&aval);
    let pct = if alvo.hp > 0.0 { tot.0 / alvo.hp } else { 0.0 };
    let mut b = Map::new();
    b.insert("nome".into(), boss_nome_v.cloned().unwrap_or(Value::Null));
    b.insert("nivel".into(), jn(nivel_boss));
    b.insert("hp".into(), jn(js::round(alvo.hp)));
    b.insert("janela".into(), jn(JANELA_BOSS));
    b.insert("tipos".into(), tipos_json(&boss_sp.tipos));
    b.insert("dano".into(), jn(js::round(tot.0)));
    b.insert("tempoDePe".into(), jn(js::round(tot.1)));
    b.insert("pctTime".into(), jn(js::round(pct * 100.0)));
    b.insert("membros".into(), Value::Array(membros));
    b.insert(
        "modelo".into(),
        json!("um por vez; cada um causa dano/s × tempo de pé, até a janela acabar"),
    );
    let ia = time.iter().position(|p| js::mais(p.get("id")) == ativo_id);
    if let Some(ia) = ia {
        let pa = time[ia];
        let lv0 = ou(js::mais(pa.get("level")), 1.0);
        let teto = (nivel_boss * 4.0).max(lv0 + 50.0);
        let com_ativo_em = |lvl: f64| -> Result<bool, ()> {
            let mut l: Vec<Option<Aval>> = Vec::new();
            for (i, x) in aval.iter().enumerate() {
                if i == ia {
                    l.push(avalia(pa, Some(lvl))?);
                } else {
                    l.push(x.as_ref().map(|y| Aval {
                        nome: y.nome.clone(),
                        r: y.r.clone(),
                        dps: y.dps,
                        vive: y.vive,
                    }));
                }
            }
            Ok(somar(&l).0 >= alvo.hp)
        };
        let calc = || -> Result<Value, ()> {
            if com_ativo_em(lv0)? {
                return Ok(jn(lv0));
            }
            if com_ativo_em(teto)? {
                let (mut lo, mut hi) = (lv0, teto);
                while hi - lo > 1.0 {
                    let mid = f64::from(((lo + hi) as i32) >> 1);
                    if com_ativo_em(mid)? {
                        hi = mid;
                    } else {
                        lo = mid;
                    }
                }
                return Ok(jn(hi));
            }
            Ok(Value::Null)
        };
        let Ok(lvl) = calc() else {
            return falhou();
        };
        b.insert("ativoPrecisaNv".into(), lvl);
        b.insert("tetoBusca".into(), jn(teto));
        let sozinho = aval[ia]
            .as_ref()
            .map_or(0.0, |x| x.dps * x.vive.min(JANELA_BOSS));
        b.insert(
            "ativoSozinhoPct".into(),
            jn(js::round(sozinho / alvo.hp * 100.0)),
        );
        let mut no_nivel: Vec<Option<Aval>> = Vec::new();
        for p in &time {
            let l = ou(js::mais(p.get("level")), 1.0).max(nivel_boss);
            match avalia(p, Some(l)) {
                Ok(a) => no_nivel.push(a),
                Err(()) => return falhou(),
            }
        }
        b.insert(
            "timeNoNivelBoss".into(),
            jn(js::round(somar(&no_nivel).0 / alvo.hp * 100.0)),
        );
    }
    let _ = u;
    Value::Object(b)
}

// ---------------------------------------------------------------------------------------
// planoRodizio
// ---------------------------------------------------------------------------------------

struct BossR {
    key: String,
    nome: Option<Value>,
    team_level: f64,
    min_tr: Option<f64>,
    i: usize,
}

/// `planoRodizio(conta)`.
pub fn plano_rodizio(d: &Value, conta: &ContaViva, dex: &Pokedex) -> Value {
    let c = cfg_rodizio(d, &conta.nick);
    let pl = jogador(conta);
    let tr_lv = {
        let a = jogo::num(conta.ultimo.get("trLv")).unwrap_or(0.0);
        ou(a, n0(pl.get("level")))
    };
    let mut out = Map::new();
    out.insert(
        "ativo".into(),
        c.get("ativo").cloned().unwrap_or(Value::Null),
    );
    out.insert("bloqueio".into(), Value::Null);
    out.insert("boss".into(), Value::Null);
    out.insert("lista".into(), json!([]));
    out.insert("equipe".into(), json!([]));
    out.insert("forca".into(), json!(0));
    out.insert("mult".into(), json!(1));
    out.insert("trLv".into(), jn(tr_lv));
    out.insert("proximo".into(), Value::Null);
    out.insert("pronta".into(), json!(false));
    out.insert("avisos".into(), json!([]));
    let bloquear = |out: &mut Map<String, Value>, msg: String| {
        out.insert("bloqueio".into(), Value::String(msg));
    };
    if !conta_de_boss(&conta.nick) {
        out.insert("ativo".into(), json!(false));
        bloquear(
            &mut out,
            format!(
                "Esta conta só farma: equipe e rodízio de boss são só da {}.",
                CONTAS_BOSS.join(", ")
            ),
        );
        return Value::Object(out);
    }
    let pokes = pokemons(conta);
    if pokes.is_empty() {
        bloquear(&mut out, "Ainda não recebi a lista de pokemons do jogo. Abra o painel da conta e recarregue uma vez.".into());
        return Value::Object(out);
    }
    let w = conta
        .welcome
        .get("bosses")
        .and_then(Value::as_array)
        .filter(|a| !a.is_empty());
    let mut bosses: Vec<BossR> = match w {
        Some(a) => a
            .iter()
            .map(|b| {
                let tl = js::nulo_ou(b.get("teamLevel"), js::nulo_ou(b.get("level"), None))
                    .map(|v| js::mais(Some(v)))
                    .or_else(|| bosses_nivel(&txt(b.get("key")).to_lowercase()));
                (b, tl)
            })
            .map(|(b, tl)| BossR {
                key: txt(b.get("key")),
                nome: b.get("nome").cloned(),
                team_level: tl.unwrap_or(0.0),
                min_tr: js::nulo_ou(b.get("minTr"), None).map(|v| js::mais(Some(v))),
                i: 0,
            })
            .collect(),
        None => BOSSES_NIVEL
            .iter()
            .map(|(k, n)| {
                let mut nome = k.to_string();
                if let Some(p) = nome.get_mut(0..1) {
                    p.make_ascii_uppercase();
                }
                BossR {
                    key: (*k).to_owned(),
                    nome: Some(Value::String(nome)),
                    team_level: *n,
                    min_tr: Some(*n),
                    i: 0,
                }
            })
            .collect(),
    };
    bosses.retain(|b| b.team_level != 0.0 && !b.team_level.is_nan());
    for (i, b) in bosses.iter_mut().enumerate() {
        b.i = i;
    }
    ordenar(&mut bosses, |a, b| {
        ou(a.team_level - b.team_level, a.i as f64 - b.i as f64)
    });
    let vazio = Value::Null;
    let vit = pl
        .get("bossVit")
        .filter(|v| verd(Some(v)))
        .unwrap_or(&vazio);
    let lista: Vec<(&BossR, bool)> = bosses
        .iter()
        .map(|b| (b, boss_feito(vit.get(&b.key))))
        .collect();
    out.insert(
        "lista".into(),
        Value::Array(
            lista
                .iter()
                .map(|(b, f)| {
                    let mut m = Map::new();
                    m.insert("key".into(), json!(b.key));
                    if let Some(n) = &b.nome {
                        m.insert("nome".into(), n.clone());
                    }
                    m.insert("teamLevel".into(), jn(b.team_level));
                    m.insert("feito".into(), json!(f));
                    Value::Object(m)
                })
                .collect(),
        ),
    );
    let alvo_boss = c.get("alvoBoss").filter(|v| verd(Some(v)));
    let alvo = alvo_boss
        .and_then(|k| {
            lista
                .iter()
                .find(|(b, _)| Some(b.key.as_str()) == k.as_str())
        })
        .or_else(|| lista.iter().find(|(_, f)| !*f))
        .or_else(|| lista.last());
    let Some((alvo, alvo_feito)) = alvo.copied() else {
        bloquear(
            &mut out,
            "Não achei a lista de bosses. Recarregue o painel do jogo uma vez.".into(),
        );
        return Value::Object(out);
    };
    let min_tr = bosses
        .iter()
        .find(|b| b.key == alvo.key)
        .and_then(|b| b.min_tr)
        .unwrap_or(alvo.team_level);
    {
        let mut m = Map::new();
        m.insert("key".into(), json!(alvo.key));
        if let Some(n) = &alvo.nome {
            m.insert("nome".into(), n.clone());
        }
        m.insert("teamLevel".into(), jn(alvo.team_level));
        m.insert("feito".into(), json!(alvo_feito));
        m.insert("minTr".into(), jn(min_tr));
        out.insert("boss".into(), Value::Object(m));
    }
    let nome_alvo = txt(alvo.nome.as_ref());
    let meta0 = alvo.team_level + n0(c.get("extra"));
    if tr_lv != 0.0 && min_tr != 0.0 && !min_tr.is_nan() && tr_lv < min_tr {
        bloquear(
            &mut out,
            format!(
                "Treinador Nv {} ainda não alcança o {} (precisa de Nv {}). Rodízio suspenso.",
                num_s(tr_lv),
                nome_alvo,
                num_s(min_tr)
            ),
        );
        return Value::Object(out);
    }
    let trav = travados(pl);
    let local_de_poke = |p: &Value| -> &'static str {
        if !nulo(p.get("slot")) {
            "equipe"
        } else if trav
            .as_ref()
            .is_some_and(|t| t.contains(&js::mais(p.get("id")).to_bits()))
        {
            "colecao"
        } else {
            "depot"
        }
    };
    let mut time: Vec<&Value> = pokes.iter().filter(|p| !nulo(p.get("slot"))).collect();
    ordenar(&mut time, |a, b| {
        js::mais(a.get("slot")) - js::mais(b.get("slot"))
    });
    if time.is_empty() {
        bloquear(
            &mut out,
            "O jogo não me disse quem está na equipe. Recarregue o painel do jogo uma vez.".into(),
        );
        return Value::Object(out);
    }
    let mut na_colecao: Vec<&Value> = pokes
        .iter()
        .filter(|p| local_de_poke(p) == "colecao")
        .collect();
    ordenar(&mut na_colecao, |a, b| {
        n0(b.get("level")) - n0(a.get("level"))
    });
    let mut completo: Vec<&Value> = time.clone();
    for p in na_colecao {
        if completo.len() >= SLOTS_BOSS {
            break;
        }
        completo.push(p);
    }
    let nome_de = |p: &Value| match por_d(dex, p.get("speciesId"), true) {
        Some(sp) => sp.nome.clone(),
        None => format!("#{}", txt(p.get("speciesId"))),
    };
    let precisa_mover: Vec<Value> = completo
        .iter()
        .filter(|p| nulo(p.get("slot")))
        .map(|p| {
            let mut m = Map::new();
            if let Some(i) = p.get("id") {
                m.insert("id".into(), i.clone());
            }
            m.insert("nome".into(), json!(nome_de(p)));
            m.insert("lvl".into(), jn(n0(p.get("level"))));
            Value::Object(m)
        })
        .collect();
    out.insert("precisaMover".into(), Value::Array(precisa_mover));
    let metas = c.get("metas").filter(|v| verd(Some(v)));
    let active = pl.get("activeId");
    struct Membro {
        json: Value,
        lvl: f64,
        faltam: f64,
        limitada: bool,
    }
    let equipe: Vec<Membro> = completo
        .iter()
        .map(|p| {
            let lvl = n0(p.get("level"));
            let pedida_m = n0(metas.and_then(|m| m.get(txt(p.get("id")))));
            let pedida = ou(pedida_m, meta0);
            let teto = if verd(p.get("starter")) || tr_lv == 0.0 {
                f64::INFINITY
            } else {
                tr_lv + 5.0
            };
            let meta = pedida.min(teto);
            let na_equipe = !nulo(p.get("slot"));
            let ativo = igual_estrito(p.get("id"), active)
                || (igual_estrito(p.get("slot"), Some(&json!(0))) && nulo(active));
            let faltam = (meta - lvl).max(0.0);
            let mut m = Map::new();
            if let Some(i) = p.get("id") {
                m.insert("id".into(), i.clone());
            }
            m.insert("nome".into(), json!(nome_de(p)));
            m.insert("lvl".into(), jn(lvl));
            m.insert("meta".into(), jn(meta));
            m.insert("pedida".into(), jn(pedida));
            m.insert("personalizada".into(), json!(pedida_m != 0.0));
            m.insert("limitada".into(), json!(meta < pedida));
            m.insert("faltam".into(), jn(faltam));
            m.insert("ativo".into(), json!(ativo));
            m.insert("naEquipe".into(), json!(na_equipe));
            Membro {
                json: Value::Object(m),
                lvl,
                faltam,
                limitada: meta < pedida,
            }
        })
        .collect();
    let forca_exata = equipe
        .iter()
        .fold(0.0, |s, e| s + (e.lvl / alvo.team_level).min(1.0));
    out.insert("forca".into(), jn(js::round(forca_exata * 100.0) / 100.0));
    out.insert(
        "mult".into(),
        jn(js::round(js::pow(3.0, (5.0 - forca_exata).max(0.0)) * 10.0) / 10.0),
    );
    let mut pend: Vec<&Membro> = equipe.iter().filter(|e| e.faltam > 0.0).collect();
    out.insert("pronta".into(), json!(pend.is_empty()));
    ordenar(&mut pend, |a, b| a.lvl - b.lvl);
    out.insert(
        "proximo".into(),
        pend.first().map_or(Value::Null, |m| m.json.clone()),
    );
    let mut avisos: Vec<Value> = Vec::new();
    if equipe.iter().any(|e| e.limitada) {
        avisos.push(json!(format!(
            "Pokemon só lutam até 5 níveis acima do treinador (Nv {}); as metas acima disso ficam limitadas até o treinador subir.",
            num_s(tr_lv + 5.0)
        )));
    }
    if time.len() < SLOTS_BOSS {
        avisos.push(json!(format!(
            "A equipe tem só {} pokemon; o boss pede {} no nível dele.",
            time.len(),
            SLOTS_BOSS
        )));
    }
    out.insert("avisos".into(), Value::Array(avisos));
    out.insert(
        "equipe".into(),
        Value::Array(equipe.iter().map(|m| m.json.clone()).collect()),
    );
    Value::Object(out)
}

// ---------------------------------------------------------------------------------------
// buildTeorica, melhorCombo, aguentaHuntDe, huntParaNivelar
// ---------------------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct Teo {
    sp_id: i64,
    nome: String,
    tipos: Vec<Tipo>,
    nivel_ideal: f64,
    pontos: f64,
}

/// `buildTeorica(bossKey, pub, trLv, modo, opts)` (sem o cache de 300 entradas).
#[allow(clippy::too_many_arguments)]
fn build_teorica(
    dex: &Pokedex,
    k: &Constantes,
    boss_key: &str,
    tr_lv: f64,
    ideal: bool,
    sem_out: bool,
    quantos: usize,
    possui: &HashSet<i64>,
) -> Option<Vec<Teo>> {
    if boss_key.is_empty() {
        return None;
    }
    let boss_sp = por_nome(dex, boss_key)?;
    let nivel_boss = bosses_nivel(&boss_key.to_lowercase()).unwrap_or(300.0);
    let trainer = ou(tr_lv, 1.0);
    let nivel_mon = if ideal {
        nivel_boss
    } else {
        1.0_f64.max((trainer + 5.0).min(nivel_boss))
    };
    let (q, pot) = if ideal { (1.5, 5) } else { (1.3, 3) };
    let alvo = mon_boss(boss_sp, nivel_boss, k);
    let boss_tipos = &boss_sp.tipos;
    let eh_outland = |sp: &EspecieCombate| (2000..3000).contains(&sp.id);
    let tem = |sp: &EspecieCombate| possui.contains(&sp.id);
    let filtra = |sp: &EspecieCombate| -> bool {
        if tem(sp) {
            !(sem_out && eh_outland(sp))
        } else {
            let normal = sp
                .categoria
                .as_deref()
                .is_none_or(|c| c.is_empty() || c == "pokebola");
            normal
                && (ideal || (!sp.lendaria() && sp.nivel_captura.is_some_and(|h| h <= trainer)))
                && !(sem_out && eh_outland(sp))
        }
    };
    let mut cands: Vec<(&EspecieCombate, f64, f64)> = dex
        .especies()
        .iter()
        .filter(|sp| filtra(sp))
        .map(|sp| {
            let st = sp.golpes.iter().fold(0.0, |s, m| {
                s + if m.poder > 0.0 && efetividade_bruta(m.tipo, boss_tipos) > 1.0 {
                    1.0
                } else {
                    0.0
                }
            });
            let bst = sp.base.iter().fold(0.0, |a, b| a + b);
            (sp, st, bst)
        })
        .collect();
    ordenar(&mut cands, |a, b| {
        let pa = f64::from(u8::from(tem(a.0)));
        let pb = f64::from(u8::from(tem(b.0)));
        ou(ou(pb - pa, b.1 - a.1), b.2 - a.2)
    });
    cands.truncate(if ideal { 120 } else { 60 + possui.len() });
    let mut av: Vec<(&EspecieCombate, f64, f64, f64)> = Vec::new();
    for (sp, _, _) in &cands {
        let me = Combatente::novo(
            sp,
            nivel_mon,
            &Opcoes {
                qualidade: q,
                ivs: [31.0; 6],
                potencia: pot,
                ..Opcoes::default()
            },
            k,
        );
        let Some(r) = lutar(&me, &alvo, JANELA_BOSS, k) else {
            continue;
        };
        let pts = pontos_boss(&r, tem_vantagem(sp, boss_tipos));
        if pts > 0.0 {
            av.push((sp, pts, r.burst / r.hp, r.first_kill.unwrap_or(f64::NAN)));
        }
    }
    ordenar(&mut av, |a, b| b.1 - a.1);
    av.truncate(quantos);
    let teo = |sp: &EspecieCombate, pts: f64| Teo {
        sp_id: sp.id,
        nome: sp.nome.clone(),
        tipos: sp.tipos.clone(),
        nivel_ideal: nivel_mon,
        pontos: pts,
    };
    if av.is_empty() {
        return Some(
            cands
                .iter()
                .take(quantos)
                .map(|(sp, _, _)| teo(sp, 0.0))
                .collect(),
        );
    }
    Some(
        av.iter()
            .map(|(sp, pts, _, _)| teo(sp, js::round(*pts)))
            .collect(),
    )
}

struct Combo<'a> {
    membros: Vec<&'a Info>,
    score: f64,
    soma: f64,
    eff: f64,
    tipo: Option<Tipo>,
    fator: f64,
}

/// `riscoEquipe(membros, bossSp)`: `(fator, eff, tipo)`.
fn risco_equipe(membros: &[&Info], boss_sp: Option<&EspecieCombate>) -> (f64, f64, Option<Tipo>) {
    let golpes: Vec<_> = boss_sp
        .map(|b| b.golpes.iter().filter(|m| m.poder > 0.0).collect())
        .unwrap_or_default();
    if golpes.is_empty() || membros.is_empty() {
        return (1.0, 1.0, None);
    }
    let (mut pior, mut tipo) = (0.0_f64, None);
    for mv in golpes {
        let soma = membros
            .iter()
            .fold(0.0, |s, m| s + efetividade_bruta(mv.tipo, &m.teo.tipos));
        let media = soma / membros.len() as f64;
        if media > pior {
            pior = media;
            tipo = Some(mv.tipo);
        }
    }
    (if pior > 0.0 { 1.0 / pior } else { 1.0 }, pior, tipo)
}

/// `melhorCombo(cands, bossSp, SLOTS, repetir)`.
fn melhor_combo<'a>(
    cands: &[&'a Info],
    boss_sp: Option<&EspecieCombate>,
    slots: usize,
    repetir: bool,
) -> Option<Combo<'a>> {
    let top: Vec<&'a Info> = cands.iter().take(10).copied().collect();
    if top.is_empty() {
        return None;
    }
    fn rec<'a>(
        top: &[&'a Info],
        i: usize,
        atual: &mut Vec<&'a Info>,
        slots: usize,
        repetir: bool,
        boss_sp: Option<&EspecieCombate>,
        best: &mut Option<Combo<'a>>,
    ) {
        if atual.len() == slots {
            let (fator, eff, tipo) = risco_equipe(atual, boss_sp);
            let soma = atual.iter().fold(0.0, |s, x| s + x.teo.pontos);
            let score = soma * fator;
            if best.as_ref().is_none_or(|b| score > b.score) {
                *best = Some(Combo {
                    membros: atual.clone(),
                    score,
                    soma,
                    eff,
                    tipo,
                    fator,
                });
            }
            return;
        }
        for j in i..top.len() {
            atual.push(top[j]);
            rec(
                top,
                if repetir { j } else { j + 1 },
                atual,
                slots,
                repetir,
                boss_sp,
                best,
            );
            atual.pop();
        }
    }
    let mut best: Option<Combo<'a>> = None;
    let mut atual: Vec<&'a Info> = Vec::new();
    rec(&top, 0, &mut atual, slots, repetir, boss_sp, &mut best);
    if best.is_none() {
        let m: Vec<&'a Info> = top.iter().take(slots).copied().collect();
        let (fator, eff, tipo) = risco_equipe(&m, boss_sp);
        let soma = m.iter().fold(0.0, |s, x| s + x.teo.pontos);
        best = Some(Combo {
            membros: m,
            score: soma * fator,
            soma,
            eff,
            tipo,
            fator,
        });
    }
    best
}

/// `aguentaHuntDe(conta, p, nomeHunt, nivelHunt)`: o pokémon aguenta a hunt (dano < 60% do HP)?
fn aguenta_hunt_de(
    dex: &Pokedex,
    k: &Constantes,
    p: &Value,
    nome_hunt: &str,
    nivel_hunt: f64,
) -> bool {
    let (Some(sp), Some(w)) = (
        por_d(dex, p.get("speciesId"), true),
        por_nome(dex, nome_hunt),
    ) else {
        return true;
    };
    let me = Combatente::novo(
        sp,
        ou(js::mais(p.get("level")), 1.0),
        &Opcoes {
            qualidade: ou(js::mais(p.get("quality")), 1.1),
            ivs: ivs_15(p),
            potencia: pot_u8(ou(js::mais(p.get("potencia")), 1.0)),
            ..Opcoes::default()
        },
        k,
    );
    match lutar(&me, &mon_selvagem(w, nivel_hunt, k), 150.0, k) {
        Some(r) => r.burst / r.hp < 0.6,
        None => true,
    }
}

/// `huntParaNivelar(conta, p)`: `(nome, nivel)` da hunt de maior nível em que `p` aguenta.
fn hunt_para_nivelar(
    d: &Value,
    conta: &ContaViva,
    dex: &Pokedex,
    p: &Value,
) -> Option<(String, f64)> {
    let sp = por_d(dex, p.get("speciesId"), true)?;
    let tr = jogo::num(conta.ultimo.get("trLv")).unwrap_or(0.0);
    let k = constantes(conta);
    let cfg = config::efetiva(d, &conta.nick);
    let max_pior = match cfg.pointer("/hunt/maxPiorPct") {
        None | Some(Value::Null) => 0.4,
        v => js::mais(v),
    };
    let nivel_p = ou(js::mais(p.get("level")), 1.0);
    let me = Combatente::novo(
        sp,
        nivel_p,
        &Opcoes {
            qualidade: ou(js::mais(p.get("quality")), 1.1),
            ivs: ivs_15(p),
            potencia: pot_u8(ou(js::mais(p.get("potencia")), 1.0)),
            ..Opcoes::default()
        },
        &k,
    );
    let piso = (nivel_p * 0.5).floor();
    let base: Vec<&EspecieCombate> = dex
        .especies()
        .iter()
        .filter(|x| {
            x.categoria.as_deref() == Some("pokebola")
                && !(2000..3000).contains(&x.id)
                && x.nivel_captura.is_some_and(|h| h != 0.0 && h <= tr)
                && jogo::slug_de(conta, &x.nome, x.nivel_captura).is_some_and(|s| !s.is_empty())
        })
        .collect();
    let acima: Vec<&EspecieCombate> = base
        .iter()
        .copied()
        .filter(|x| x.nivel_captura.unwrap_or(0.0) >= piso)
        .collect();
    let mut lista = if acima.is_empty() { base } else { acima };
    let nv = |x: &&EspecieCombate| x.nivel_captura.unwrap_or(0.0);
    ordenar(&mut lista, |a, b| nv(b) - nv(a));
    if lista.len() > 100 {
        let passo = lista.len() as f64 / 100.0;
        lista = (0..100)
            .map(|i| lista[(i as f64 * passo).floor() as usize])
            .collect();
    }
    let mut melhor: Option<(String, f64, f64)> = None;
    for x in lista {
        let h = x.nivel_captura.unwrap_or(0.0);
        let r = lutar(&me, &mon_selvagem(x, h, &k), 150.0, &k)?;
        if (r.kph.is_nan() || r.kph <= 0.0) || r.burst / r.hp >= 0.45 {
            continue;
        }
        let sc = h * r.kph;
        let segura = r.burst / r.hp < max_pior && r.bate >= 2.5;
        let sc_final = if segura { sc * 1.2 } else { sc };
        if melhor.as_ref().is_none_or(|m| sc_final > m.2) {
            melhor = Some((x.nome.clone(), h, sc_final));
        }
    }
    melhor.map(|(n, h, _)| (n, h))
}

// ---------------------------------------------------------------------------------------
// planoEquipe
// ---------------------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct HuntInfo {
    nome: String,
    nivel: f64,
    slug: Option<String>,
}

impl HuntInfo {
    fn json(&self) -> Value {
        json!({"nome": self.nome, "nivel": jn(self.nivel), "slug": self.slug})
    }
}

#[derive(Debug, Clone)]
struct Info {
    teo: Teo,
    hunt: Option<HuntInfo>,
    alcanca: bool,
    seguro: bool,
    viavel: bool,
}

#[derive(Debug, Clone)]
struct Alvo {
    slot: usize,
    teo: Teo,
    id: Option<Value>,
    lvl: Option<f64>,
    qualidade: Option<f64>,
    iv_total: Option<f64>,
    razao: Option<f64>,
    bom: bool,
    hunt: Option<HuntInfo>,
    alcanca: bool,
    seguro: bool,
    atende: bool,
}

impl Alvo {
    fn json(&self) -> Value {
        let nota = self.razao.map(|r| js::round(r * 100.0));
        json!({
            "slot": self.slot, "speciesId": self.teo.sp_id, "nome": self.teo.nome,
            "tipos": tipos_json(&self.teo.tipos), "pontos": jn(self.teo.pontos),
            "nivelIdeal": jn(self.teo.nivel_ideal), "tem": self.atende, "atende": self.atende,
            "id": self.id.clone().unwrap_or(Value::Null), "lvl": jn_ou_nulo(self.lvl),
            "qualidade": jn_ou_nulo(self.qualidade), "ivTotal": jn_ou_nulo(self.iv_total),
            "nota": jn_ou_nulo(nota), "razao": jn_ou_nulo(self.razao.map(|r| js::round(r * 100.0) / 100.0)),
            "bom": self.bom, "hunt": self.hunt.as_ref().map_or(Value::Null, HuntInfo::json),
            "alcanca": self.alcanca, "seguro": self.seguro, "trocadoPor": null, "motivoTroca": null
        })
    }
    fn cacavel(&self, ativo_lvl: f64, desistiu: &HashMap<i64, f64>, agora: f64) -> bool {
        self.hunt
            .as_ref()
            .is_some_and(|h| h.slug.as_deref().is_some_and(|s| !s.is_empty()))
            && self.alcanca
            && self.seguro
            && !self.baixa(ativo_lvl)
            && desistiu
                .get(&self.teo.sp_id)
                .is_none_or(|t| t.is_nan() || *t <= agora)
    }
    fn baixa(&self, ativo_lvl: f64) -> bool {
        let piso = (ativo_lvl * 0.5).floor();
        self.hunt.is_some()
            && ativo_lvl != 0.0
            && self.hunt.as_ref().is_some_and(|h| h.nivel < piso)
    }
}

fn iv_tot(p: &Value) -> f64 {
    iv_arr(p.get("ivs"))
        .unwrap_or_default()
        .iter()
        .fold(0.0, |a, b| a + n0(Some(&jn(*b))))
}

/// Nota do exemplar, de 0 a 1 (qualidade, IVs e potência).
fn nota_de(p: &Value) -> f64 {
    let q = ou(js::mais(p.get("quality")), 1.0);
    let pot = ou(pot_of(p), 1.0);
    let q_pct = 0.0_f64.max(1.0_f64.min((q - 1.0) / 0.6));
    let iv_pct = 0.0_f64.max(1.0_f64.min(iv_tot(p) / 186.0));
    let pot_pct = 0.0_f64.max(1.0_f64.min((pot - 1.0) / 4.0));
    0.5 * q_pct + 0.35 * iv_pct + 0.15 * pot_pct
}

/// `planoEquipe(conta)`. `desistiu` é `conta.equipeDesistiu` (espécie → até quando); o
/// relógio vem de `r`.
pub fn plano_equipe(
    d: &Value,
    r: &Relogio,
    conta: &ContaViva,
    dex: &Pokedex,
    desistiu: &HashMap<i64, f64>,
) -> Value {
    let c = cfg_rodizio(d, &conta.nick);
    let agora = r.agora_ms as f64;
    let min_q = n0(c.get("minQualidade"));
    let min_iv = n0(c.get("minIvTotal"));
    let mut out = Map::new();
    out.insert("ativo".into(), json!(verd(c.get("ativo"))));
    out.insert(
        "capturar".into(),
        json!(c.get("capturar") != Some(&json!(false))),
    );
    out.insert("bloqueio".into(), Value::Null);
    out.insert("boss".into(), Value::Null);
    out.insert("alvos".into(), json!([]));
    out.insert("faltam".into(), json!([]));
    out.insert("fase".into(), Value::Null);
    out.insert("alvoAgora".into(), Value::Null);
    out.insert("minQualidade".into(), jn(min_q));
    out.insert("minIvTotal".into(), jn(min_iv));
    let bloq = |out: &mut Map<String, Value>, m: &str| {
        out.insert("bloqueio".into(), json!(m));
    };
    let pokes = pokemons(conta);
    if pokes.is_empty() {
        bloq(&mut out, "Ainda não recebi a lista de pokemons do jogo. Abra o painel da conta e recarregue uma vez.");
        return Value::Object(out);
    }
    let rod = plano_rodizio(d, conta, dex);
    if verd(rod.get("bloqueio")) {
        out.insert(
            "bloqueio".into(),
            rod.get("bloqueio").cloned().unwrap_or(Value::Null),
        );
        return Value::Object(out);
    }
    let boss = rod.get("boss").cloned().unwrap_or(Value::Null);
    out.insert("boss".into(), boss.clone());
    let tr_lv = n0(Some(
        &jn(jogo::num(conta.ultimo.get("trLv")).unwrap_or(0.0)),
    ));
    let possui: HashSet<i64> = pokes
        .iter()
        .filter_map(|p| {
            let n = js::mais(p.get("speciesId"));
            (n.is_finite() && n.fract() == 0.0).then_some(n as i64)
        })
        .collect();
    let k = constantes(conta);
    let boss_key = txt(boss.get("key"));
    let teo = build_teorica(
        dex,
        &k,
        &boss_key,
        tr_lv,
        false,
        c.get("outland") != Some(&json!(true)),
        12,
        &possui,
    )
    .unwrap_or_default();
    if teo.is_empty() {
        bloq(
            &mut out,
            "Ainda não consegui calcular a equipe teórica deste boss.",
        );
        return Value::Object(out);
    }
    let pl = jogador(conta);
    let ativo_id = js::mais(pl.get("activeId"));
    let ativo_p = pokes
        .iter()
        .find(|p| js::mais(p.get("id")) == ativo_id)
        .or_else(|| {
            pokes
                .iter()
                .find(|p| igual_estrito(p.get("slot"), Some(&json!(0))))
        });
    let info: Vec<Info> = teo
        .iter()
        .map(|t| {
            let sp = dex.por_id(t.sp_id);
            let hunt = sp.and_then(|s| {
                s.nivel_captura.map(|h| HuntInfo {
                    nome: s.nome.clone(),
                    nivel: h,
                    slug: jogo::slug_de(conta, &s.nome, Some(h)),
                })
            });
            let alcanca = hunt.as_ref().is_some_and(|h| {
                h.slug.as_deref().is_some_and(|s| !s.is_empty()) && h.nivel <= tr_lv
            });
            let seguro = alcanca
                && match (ativo_p, &hunt) {
                    (Some(a), Some(h)) => aguenta_hunt_de(dex, &k, a, &h.nome, h.nivel),
                    _ => true,
                };
            let tem = possui.contains(&t.sp_id);
            Info {
                teo: t.clone(),
                hunt,
                alcanca,
                seguro,
                viavel: tem || (alcanca && seguro),
            }
        })
        .collect();
    let viaveis: Vec<&Info> = info.iter().filter(|x| x.viavel).collect();
    if viaveis.is_empty() {
        bloq(&mut out, "Nenhuma das espécies boas para este boss está em hunt que a conta alcança com segurança.");
        return Value::Object(out);
    }
    let boss_sp = por_nome(dex, &txt(boss.get("nome")));
    let repetir = c.get("repetirEspecie") == Some(&json!("sim"));
    let Some(escolhido) = melhor_combo(&viaveis, boss_sp, SLOTS_BOSS, repetir) else {
        bloq(
            &mut out,
            "Não consegui montar a equipe com as espécies disponíveis.",
        );
        return Value::Object(out);
    };
    let alternativo = boss_sp.and_then(|b| melhor_combo(&viaveis, Some(b), SLOTS_BOSS, !repetir));
    out.insert(
        "comparacao".into(),
        json!({
            "escolhido": if repetir { "repetir" } else { "variar" },
            "forcado": c.get("repetirEspecie") != Some(&json!("auto")),
            "pontos": jn(js::round(escolhido.score)), "soma": jn(js::round(escolhido.soma)),
            "alternativa": jn_ou_nulo(alternativo.as_ref().map(|a| js::round(a.score))),
            "sinergia": {"eff": jn(js::round(escolhido.eff * 100.0) / 100.0),
                         "tipo": escolhido.tipo.map(Tipo::nome),
                         "fator": jn(js::round(escolhido.fator * 100.0) / 100.0)}
        }),
    );
    let alvo_pct = if nulo(c.get("alvoPct")) {
        0.5
    } else {
        js::mais(c.get("alvoPct"))
    };
    out.insert("alvoPct".into(), jn(alvo_pct));
    let mut usados: HashSet<String> = HashSet::new();
    let mut alvos: Vec<Alvo> = Vec::new();
    for (i, t) in escolhido.membros.iter().enumerate() {
        let mut melhor: Option<(&Value, f64)> = None;
        for x in pokes {
            if js::mais(x.get("speciesId")) != t.teo.sp_id as f64
                || usados.contains(&txt(x.get("id")))
                || n0(x.get("quality")) < min_q
                || iv_tot(x) < min_iv
            {
                continue;
            }
            let n = nota_de(x);
            if melhor.is_none_or(|m| n > m.1) {
                melhor = Some((x, n));
            }
        }
        if let Some((p, _)) = melhor {
            usados.insert(txt(p.get("id")));
        }
        let razao = melhor.map(|m| m.1);
        alvos.push(Alvo {
            slot: i + 1,
            teo: t.teo.clone(),
            id: melhor.map(|(p, _)| p.get("id").cloned().unwrap_or(Value::Null)),
            lvl: melhor.map(|(p, _)| n0(p.get("level"))),
            qualidade: melhor.map(|(p, _)| n0(p.get("quality"))),
            iv_total: melhor.map(|(p, _)| iv_tot(p)),
            razao,
            bom: razao.map_or(melhor.is_some(), |r| r >= alvo_pct),
            hunt: t.hunt.clone(),
            alcanca: t.alcanca,
            seguro: t.seguro,
            atende: melhor.is_some(),
        });
    }
    let faltam: Vec<&Alvo> = alvos.iter().filter(|a| !a.atende).collect();
    out.insert(
        "alvos".into(),
        Value::Array(alvos.iter().map(Alvo::json).collect()),
    );
    out.insert(
        "faltam".into(),
        Value::Array(faltam.iter().map(|a| a.json()).collect()),
    );
    let adiado = |a: &Alvo| desistiu.get(&a.teo.sp_id).copied().unwrap_or(f64::NAN) > agora;
    let fracos: Vec<&Alvo> = if c.get("melhorar") == Some(&json!(false)) {
        Vec::new()
    } else {
        alvos
            .iter()
            .filter(|a| a.atende && !a.bom && !adiado(a))
            .collect()
    };
    let adiados: Vec<&str> = alvos
        .iter()
        .filter(|a| a.atende && !a.bom && adiado(a))
        .map(|a| a.teo.nome.as_str())
        .collect();
    let slot_de = |id: Option<&Value>| -> Option<f64> {
        let alvo_id = id.map_or(0.0, |v| js::mais(Some(v)));
        let p = pokes.iter().find(|x| js::mais(x.get("id")) == alvo_id)?;
        (!nulo(p.get("slot"))).then(|| js::mais(p.get("slot")))
    };
    let montagem: Vec<Value> = if faltam.is_empty() {
        let ocupados: Vec<f64> = alvos
            .iter()
            .filter_map(|x| slot_de(x.id.as_ref()))
            .collect();
        let livre = (0..5)
            .find(|i| !ocupados.contains(&f64::from(*i)))
            .map(f64::from);
        alvos
            .iter()
            .enumerate()
            .filter(|(_, a)| a.id.as_ref().is_some_and(|i| verd(Some(i))) && slot_de(a.id.as_ref()).is_none())
            .map(|(idx, a)| {
                json!({"id": a.id, "nome": a.teo.nome, "slot": jn(livre.unwrap_or(idx as f64)), "slotAtual": null})
            })
            .collect()
    } else {
        Vec::new()
    };
    let fase = if !faltam.is_empty() {
        "capturar"
    } else if !montagem.is_empty() {
        "montar"
    } else if !fracos.is_empty() {
        "melhorar"
    } else {
        "nivelar"
    };
    out.insert(
        "fracos".into(),
        Value::Array(fracos.iter().map(|a| a.json()).collect()),
    );
    out.insert("adiados".into(), json!(adiados));
    out.insert("montagem".into(), Value::Array(montagem));
    out.insert("fase".into(), json!(fase));
    let ativo_lv = ativo_p.map_or(0.0, |p| n0(p.get("level")));
    let cac = |a: &&Alvo| a.cacavel(ativo_lv, desistiu, agora);
    let baixas: Vec<String> = faltam
        .iter()
        .chain(fracos.iter())
        .filter(|a| a.baixa(ativo_lv))
        .map(|a| {
            format!(
                "{} (hunt Nv {}, ativo Nv {})",
                a.teo.nome,
                a.hunt.as_ref().map_or_else(String::new, |h| num_s(h.nivel)),
                num_s(ativo_lv)
            )
        })
        .collect();
    out.insert("baixas".into(), json!(baixas));
    let dist = |a: &&Alvo| (a.hunt.as_ref().map_or(f64::NAN, |h| h.nivel) - ativo_lv).abs();
    let mut cand_f: Vec<&Alvo> = faltam.iter().copied().filter(|a| cac(a)).collect();
    ordenar(&mut cand_f, |x, y| dist(x) - dist(y));
    let mut alvo_agora: Option<&Alvo> = cand_f.first().copied();
    if alvo_agora.is_none() && !fracos.is_empty() {
        let mut fr: Vec<&Alvo> = fracos.clone();
        ordenar(&mut fr, |x, y| {
            ou(x.razao.unwrap_or(0.0), 0.0) - ou(y.razao.unwrap_or(0.0), 0.0)
        });
        alvo_agora = fr.into_iter().find(|a| cac(a));
    }
    out.insert(
        "alvoAgora".into(),
        alvo_agora.map_or(Value::Null, Alvo::json),
    );
    if alvo_agora.is_none() && (!faltam.is_empty() || !fracos.is_empty()) {
        let mut nomes: Vec<&str> = Vec::new();
        for a in faltam.iter().chain(fracos.iter()).filter(|a| !cac(a)) {
            if !nomes.contains(&a.teo.nome.as_str()) {
                nomes.push(a.teo.nome.as_str());
            }
        }
        out.insert("semHunt".into(), json!(nomes));
    }
    Value::Object(out)
}

// ---------------------------------------------------------------------------------------
// Estado e regras do laço
// ---------------------------------------------------------------------------------------

/// Campos de `conta.*` que `motorEquipe` e `motorRodizio` gravam e que não são números
/// soltos (os numéricos ficam em [`EstadoLaco::marcas`]: `equipeEm`, `equipePausaAte`,
/// `equipeSaiuHunt`, `equipeMontou`, `equipeTroca`, `limpouBoss`, `rodizioEm`,
/// `rodizioTroca`, `rodizioPulou`).
#[derive(Debug, Default, Clone)]
pub struct EstadoEquipe {
    /// `conta.equipePlano`.
    pub equipe_plano: Option<Value>,
    /// `conta.equipeBola`.
    pub equipe_bola: bool,
    /// `conta.equipeDiag`.
    pub equipe_diag: Option<Value>,
    /// `conta.equipeCapturando`: espécie → desde quando (ms).
    pub equipe_capturando: HashMap<i64, f64>,
    /// `conta.equipeMelhorando`.
    pub equipe_melhorando: HashMap<i64, f64>,
    /// `conta.equipeDesistiu`: espécie → até quando (ms).
    pub equipe_desistiu: HashMap<i64, f64>,
    /// `conta.rodizioBloq`: id do pokémon (texto) → até quando (ms).
    pub rodizio_bloq: HashMap<String, f64>,
    /// `conta.rodizioTrocas`: `{id, de, em}`.
    pub rodizio_trocas: Vec<(Value, Value, f64)>,
    /// `conta.rodizioVolta.forte`: o pokémon principal a devolver ao fim do rodízio.
    pub rodizio_volta: Option<Value>,
}

impl EstadoEquipe {
    /// O estado de equipe e rodízio que o legado guarda na conta (`objetos` da rota
    /// `janela`). Em sombra, vale o do legado a cada ciclo.
    pub fn do_legado(o: &Value) -> Self {
        let mapa_i = |k: &str| -> HashMap<i64, f64> {
            o.get(k)
                .and_then(Value::as_object)
                .map(|m| {
                    m.iter()
                        .filter_map(|(id, t)| Some((id.parse().ok()?, t.as_f64()?)))
                        .collect()
                })
                .unwrap_or_default()
        };
        let valor = |k: &str| o.get(k).filter(|v| !v.is_null()).cloned();
        Self {
            equipe_plano: valor("equipePlano"),
            equipe_bola: false,
            equipe_diag: valor("equipeDiag"),
            equipe_capturando: mapa_i("equipeCapturando"),
            equipe_melhorando: mapa_i("equipeMelhorando"),
            equipe_desistiu: mapa_i("equipeDesistiu"),
            rodizio_bloq: o
                .get("rodizioBloq")
                .and_then(Value::as_object)
                .map(|m| {
                    m.iter()
                        .filter_map(|(id, t)| Some((id.clone(), t.as_f64()?)))
                        .collect()
                })
                .unwrap_or_default(),
            rodizio_trocas: o
                .get("rodizioTrocas")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|t| {
                            Some((
                                t.get("id").cloned().unwrap_or(Value::Null),
                                t.get("de").cloned().unwrap_or(Value::Null),
                                t.get("em")?.as_f64()?,
                            ))
                        })
                        .collect()
                })
                .unwrap_or_default(),
            rodizio_volta: o
                .get("rodizioVolta")
                .and_then(|v| v.get("forte"))
                .filter(|v| !v.is_null())
                .cloned(),
        }
    }
}

/// `listaDesejos(conta)`: as espécies das builds teóricas dos 10 bosses prioritários (a venda
/// nunca dispensa essas). O legado guarda por 10 min; aqui é calculada a cada chamada, o que dá
/// o mesmo resultado enquanto a prioridade não muda.
pub fn lista_desejos(d: &Value, conta: &ContaViva, dex: &Pokedex) -> Vec<i64> {
    let pri = priorizar_bosses(d, conta);
    let tr_lv = ou(jogo::num(conta.ultimo.get("trLv")).unwrap_or(0.0), 1.0);
    let k = constantes(conta);
    let mut ids: Vec<i64> = Vec::new();
    for b in pri
        .as_array()
        .map_or(&[][..], Vec::as_slice)
        .iter()
        .take(10)
    {
        let chave = txt(b.get("key"));
        let lista = build_teorica(dex, &k, &chave, tr_lv, false, false, 6, &HashSet::new())
            .unwrap_or_default();
        for x in lista {
            if !ids.contains(&x.sp_id) {
                ids.push(x.sp_id);
            }
        }
    }
    ids
}

/// `limiteRodizio` do `motorHunt`: nível do mais fraco que o rodízio ainda vai subir, quando o
/// ativo já bateu a meta e a equipe não está sendo montada.
pub fn limite_rodizio(
    d: &Value,
    laco: &EstadoLaco,
    r: &Relogio,
    conta: &ContaViva,
    dex: &Pokedex,
    st: &EstadoEquipe,
) -> Option<f64> {
    let cfg = crate::motor::config::efetiva(d, &conta.nick);
    if !verd(cfg.pointer("/rodizio/ativo")) {
        return None;
    }
    let p = plano_rodizio(d, conta, dex);
    let equipe = p.get("equipe").and_then(Value::as_array)?;
    if verd(p.get("bloqueio")) || equipe.is_empty() {
        return None;
    }
    let faltam = |e: &Value| js::mais(e.get("faltam"));
    if equipe
        .iter()
        .find(|e| verd(e.get("ativo")))
        .is_some_and(|a| faltam(a) > 0.0)
    {
        return None;
    }
    if equipe_no_comando(laco, r, &conta.nick, st) {
        return None;
    }
    let agora = r.agora_ms as f64;
    let mut fracos: Vec<&Value> = equipe
        .iter()
        .filter(|e| {
            let id = js::texto(e.get("id"));
            faltam(e) > 0.0 && !st.rodizio_bloq.get(&id).is_some_and(|b| *b > agora)
        })
        .collect();
    crate::motor::veredito::ordenar_como_js(&mut fracos, |a, b| {
        js::mais(a.get("lvl")) - js::mais(b.get("lvl"))
    });
    fracos.first().map(|f| js::mais(f.get("lvl")))
}

fn comando(nick: &str, obj: Value, registro: Option<Registro>) -> Decisao {
    Decisao::Comando {
        nick: nick.to_owned(),
        comando: obj,
        registro,
    }
}

fn registro(tipo: &str, ok: String, falha: String) -> Registro {
    Registro {
        tipo: tipo.to_owned(),
        texto_ok: ok,
        texto_falha: falha,
        custo: None,
    }
}

fn registrar(nick: &str, tipo: &str, texto: String, ok: bool) -> Decisao {
    Decisao::Registro {
        nick: nick.to_owned(),
        tipo: tipo.to_owned(),
        texto,
        ok,
    }
}

/// `{ t: 'auto.set', ...a, ...patch }`.
fn auto_set(a: Option<&Value>, patch: &[(&str, Value)]) -> Value {
    let mut m = Map::new();
    m.insert("t".into(), json!("auto.set"));
    if let Some(Value::Object(o)) = a {
        for (k, v) in o {
            m.insert(k.clone(), v.clone());
        }
    }
    for (k, v) in patch {
        m.insert((*k).to_owned(), v.clone());
    }
    Value::Object(m)
}

/// `equipeNoComando(conta)`.
/// `equipeNoComando(conta)`: a equipe está sendo montada (o rodízio e a hunt esperam).
pub fn equipe_no_comando(laco: &EstadoLaco, r: &Relogio, nick: &str, st: &EstadoEquipe) -> bool {
    let Some(e) = st.equipe_plano.as_ref() else {
        return false;
    };
    if verd(e.get("bloqueio")) {
        return false;
    }
    let agora = r.agora_ms as f64;
    let fase = e.get("fase").and_then(Value::as_str).unwrap_or("");
    let pausa = laco.marca(nick, "equipePausaAte");
    if fase == "montar" {
        return !(pausa != 0.0 && agora < pausa)
            && e.get("montagem")
                .and_then(Value::as_array)
                .is_some_and(|m| !m.is_empty());
    }
    (fase == "capturar" || fase == "melhorar") && verd(e.get("alvoAgora"))
}

fn nome_da_especie(dex: &Pokedex, p: &Value) -> String {
    match por_d(dex, p.get("speciesId"), true) {
        Some(sp) => sp.nome.clone(),
        None => format!("#{}", txt(p.get("speciesId"))),
    }
}

/// `voltarHunt(motivo)` do `motorEquipe`.
fn voltar_hunt(
    out: &mut Vec<Decisao>,
    laco: &mut EstadoLaco,
    conta: &ContaViva,
    au: Option<&Value>,
    motivo: &str,
) {
    let nick = conta.nick.as_str();
    if laco.marca(nick, "equipeSaiuHunt") == 0.0 {
        return;
    }
    laco.marcar(nick, "equipeSaiuHunt", 0.0);
    let a = automacao(conta).or(au);
    if verd(a.and_then(|x| x.get("desistiuHunt"))) {
        out.push(comando(
            nick,
            auto_set(a, &[("desistiuHunt", json!(false))]),
            None,
        ));
        out.push(registrar(
            nick,
            "equipe",
            format!("Voltando para a hunt: {motivo}."),
            true,
        ));
    }
}

/// `motorEquipe(conta)`. `st` guarda o resto do estado da conta; `sessao_tomada` é
/// `conta.sessaoTomada`.
///
/// RESPOSTA DO JOGO: todo `comando(...)` é assumido `'ok'`; `avaliar(...)` assume que o
/// pokémon entrou no slot pedido, sem erro (`window.__hdxErro` nulo) e que a saída da
/// equipe funcionou. Os ramos de falha (`rr !== 'ok'`, `saiu === false`, contador
/// `equipeMontou` chegando a 4 ou 7) não são alcançados em sombra. As esperas
/// (`setTimeout`) não existem aqui.
pub fn motor_equipe(
    d: &Value,
    laco: &mut EstadoLaco,
    r: &Relogio,
    conta: &ContaViva,
    dex: &Pokedex,
    st: &mut EstadoEquipe,
    sessao_tomada: bool,
) -> Vec<Decisao> {
    let mut out: Vec<Decisao> = Vec::new();
    let nick = conta.nick.as_str();
    let agora = r.agora_ms as f64;
    let c = cfg_rodizio(d, nick);
    if !conta_de_boss(nick) {
        // conta só de farm: desfaz uma vez o que a captura de equipe pode ter deixado ligado
        if laco.marca(nick, "limpouBoss") != 0.0 || nick.is_empty() || sessao_tomada {
            return out;
        }
        let Some(a) = automacao(conta) else {
            return out;
        };
        laco.marcar(nick, "limpouBoss", 1.0);
        st.equipe_plano = None;
        st.equipe_bola = false;
        laco.marcar(nick, "equipeSaiuHunt", 0.0);
        if verd(a.get("desistiuHunt")) {
            out.push(comando(
                nick,
                auto_set(Some(a), &[("desistiuHunt", json!(false))]),
                None,
            ));
            out.push(registrar(
                nick,
                "equipe",
                "Esta conta só farma: voltei para a hunt (a troca de equipe tinha mandado para a cidade).".into(),
                true,
            ));
        }
        return out;
    }
    if !verd(c.get("ativo")) || c.get("capturar") == Some(&json!(false)) {
        return out;
    }
    let em = laco.marca(nick, "equipeEm");
    if em != 0.0 && agora - em < 60e3 {
        return out;
    }
    if sessao_tomada {
        return out;
    }
    laco.marcar(nick, "equipeEm", agora);
    let e = plano_equipe(d, r, conta, dex, &st.equipe_desistiu);
    st.equipe_plano = Some(e.clone());
    let au = automacao(conta);
    let pausa_ate = laco.marca(nick, "equipePausaAte");
    let pausada = pausa_ate != 0.0 && agora < pausa_ate;
    let fase = e
        .get("fase")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    if fase != "montar" || pausada {
        voltar_hunt(
            &mut out,
            laco,
            conta,
            au,
            if pausada {
                "troca de equipe pausada por 30 min depois de várias falhas"
            } else {
                "não há troca de equipe pendente"
            },
        );
    }
    let boss_nome = txt(e.pointer("/boss/nome"));
    let montagem: &[Value] = e
        .get("montagem")
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice);
    let pokes = pokemons(conta);
    // Equipe escolhida em mãos: põe os cinco nos slots, um comando por ciclo.
    if !verd(e.get("bloqueio")) && fase == "montar" && !montagem.is_empty() && !pausada {
        let m = &montagem[0];
        // `slotAtual` é sempre null no plano: só o ramo "veio da coleção" existe.
        let alvo_ids: HashSet<u64> = e
            .get("alvos")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .map(|x| js::mais(x.get("id")))
                    .filter(|n| *n != 0.0 && !n.is_nan())
                    .map(f64::to_bits)
                    .collect()
            })
            .unwrap_or_default();
        let sobrando: Vec<&Value> = pokes
            .iter()
            .filter(|p| {
                !nulo(p.get("slot"))
                    && !alvo_ids.contains(&js::mais(p.get("id")).to_bits())
                    && !verd(p.get("starter"))
            })
            .collect();
        let na_equipe = pokes.iter().filter(|p| !nulo(p.get("slot"))).count();
        if !sobrando.is_empty() && na_equipe >= 5 {
            let em_combate = caminho(&conta.ultimo, &["io", "player", "chavesEstado"])
                .and_then(Value::as_array)
                .is_some_and(|a| a.iter().any(|x| x.as_str() == Some("selvagem")));
            if em_combate {
                let au_atual = automacao(conta);
                if !verd(au_atual.and_then(|a| a.get("desistiuHunt"))) {
                    out.push(comando(
                        nick,
                        auto_set(au_atual, &[("desistiuHunt", json!(true))]),
                        None,
                    ));
                    let saiu = laco.marca(nick, "equipeSaiuHunt");
                    laco.marcar(nick, "equipeSaiuHunt", ou(saiu, agora));
                    out.push(registrar(
                        nick,
                        "equipe",
                        format!("Indo à cidade para trocar a equipe do {boss_nome} (espero sair do combate)."),
                        true,
                    ));
                } else {
                    let saiu = laco.marca(nick, "equipeSaiuHunt");
                    if saiu != 0.0 && agora - saiu > 5.0 * 60e3 {
                        laco.marcar(nick, "equipePausaAte", agora + 30.0 * 60e3);
                        voltar_hunt(
                            &mut out,
                            laco,
                            conta,
                            au,
                            "o jogo não saiu do combate em 5 min",
                        );
                        let pedido = Decisao::Alerta {
                            nick: nick.to_owned(),
                            chave: "equipe-combate".into(),
                            titulo: "Troca de equipe não andou".into(),
                            texto: format!("Tentei sair do combate por 5 min para trocar a equipe do {boss_nome} e o jogo não saiu. Volto a tentar em 30 min."),
                            minutos: 60,
                        };
                        out.extend(alerta(d, laco, r, pedido));
                    }
                }
                // não segura o loop: confere de novo no próximo ciclo
                laco.marcar(nick, "equipeEm", agora - 45e3);
                return out;
            }
            let mut ordem = sobrando.clone();
            ordenar(&mut ordem, |a, b| n0(a.get("level")) - n0(b.get("level")));
            let fora = ordem[0];
            // pokemonTravado: destrava antes de mover
            let ptrav: Vec<Value> = au
                .and_then(|a| a.get("pokemonTravado"))
                .filter(|v| verd(Some(v)))
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let fora_id = js::mais(fora.get("id"));
            let nome_fora = nome_da_especie(dex, fora);
            if ptrav.iter().any(|x| js::mais(Some(x)) == fora_id) {
                let nova: Vec<Value> = ptrav
                    .iter()
                    .filter(|x| js::mais(Some(x)) != fora_id)
                    .cloned()
                    .collect();
                out.push(comando(
                    nick,
                    auto_set(au, &[("pokemonTravado", Value::Array(nova))]),
                    None,
                ));
                out.push(registrar(
                    nick,
                    "equipe",
                    format!("Destravei {nome_fora} (estava em pokemonTravado) para poder tirar da equipe."),
                    true,
                ));
            }
            out.push(comando(
                nick,
                json!({"t": "colecao.mover", "pokemonId": fora.get("id").cloned().unwrap_or(Value::Null), "para": "colecao"}),
                Some(registro(
                    "equipe",
                    format!(
                        "Tirei {nome_fora} (Nv {}) da equipe: ele não entra na equipe escolhida para o {boss_nome}.",
                        txt(fora.get("level"))
                    ),
                    format!("Não consegui tirar {nome_fora} da equipe ({{r}})."),
                )),
            ));
            // RESPOSTA DO JOGO: `colecao.mover` ok e `saiu !== false`.
        }
        let m_id = m.get("id").cloned().unwrap_or(Value::Null);
        let m_slot = js::mais(m.get("slot"));
        out.push(comando(
            nick,
            json!({"t": "team.move", "pokemonId": m_id, "slot": jn(m_slot)}),
            None,
        ));
        out.push(comando(
            nick,
            json!({"t": "team.add", "pokemonId": m_id}),
            None,
        ));
        // RESPOSTA DO JOGO: `entrou === 'slot' + m.slot`, sem erro.
        let tentativas = laco.marca(nick, "equipeMontou") + 1.0;
        st.equipe_diag = Some(json!({
            "id": m_id, "nome": m.get("nome").cloned().unwrap_or(Value::Null),
            "pediu": "move+add", "slot": jn(m_slot), "entrou": format!("slot{}", num_s(m_slot)),
            "erro": null, "tentativas": jn(tentativas), "em": jn(agora)
        }));
        out.push(registrar(
            nick,
            "equipe",
            format!(
                "Equipe do {boss_nome}: {} no slot {} (veio da coleção). Faltam {}.",
                txt(m.get("nome")),
                num_s(m_slot + 1.0),
                montagem.len() - 1
            ),
            true,
        ));
        laco.marcar(nick, "equipeMontou", 0.0);
        return out;
    }
    if fase == "nivelar" {
        laco.marcar(nick, "equipeMontou", 0.0);
        laco.marcar(nick, "equipePausaAte", 0.0);
        if verd(au.and_then(|a| a.get("desistiuHunt"))) {
            laco.marcar(nick, "equipeSaiuHunt", 0.0);
            out.push(comando(
                nick,
                auto_set(au, &[("desistiuHunt", json!(false))]),
                None,
            ));
            out.push(registrar(
                nick,
                "equipe",
                format!("Equipe do {boss_nome} montada! Voltando para a hunt."),
                true,
            ));
        }
    }
    let a_agora = e.get("alvoAgora").filter(|v| verd(Some(v)));
    if verd(e.get("bloqueio")) || !(fase == "capturar" || fase == "melhorar") || a_agora.is_none() {
        st.equipe_bola = false;
        return out;
    }
    let Some(a) = a_agora else {
        return out;
    };
    let a_nome = txt(a.get("nome"));
    let a_sp = js::mais(a.get("speciesId")) as i64;
    let h_nome = txt(a.pointer("/hunt/nome"));
    let h_nivel = a.pointer("/hunt/nivel").cloned().unwrap_or(Value::Null);
    let h_slug = txt(a.pointer("/hunt/slug"));
    let hunt_agora = jogo::hunt_atual(conta);
    let na_hunt = hunt_agora.as_ref().is_some_and(|(n, l)| {
        norm_texto(n) == norm(Some(&json!(h_nome))) && l.unwrap_or(0.0) == js::mais(Some(&h_nivel))
    });
    if !na_hunt {
        if agora - laco.marca(nick, "equipeTroca") < 3.0 * 60e3 {
            return out;
        }
        let bola_estava = verd(au.and_then(|x| x.get("autoBallSemParar")));
        if bola_estava {
            out.push(comando(
                nick,
                auto_set(au, &[("autoBallSemParar", json!(false))]),
                None,
            ));
        }
        let porque = if fase == "melhorar" {
            format!(
                "O {a_nome} que tenho rende só {}% do que a espécie dá (alvo {}%)",
                num_s(js::round(ou(js::mais(a.get("razao")), 0.0) * 100.0)),
                num_s(js::round(js::mais(e.get("alvoPct")) * 100.0))
            )
        } else {
            format!(
                "Falta {a_nome} para a equipe do {boss_nome} ({} pts)",
                txt(a.get("pontos"))
            )
        };
        let nivel_txt = num_s(js::mais(Some(&h_nivel)));
        out.push(Decisao::TrocarHunt {
            nick: nick.to_owned(),
            slug: h_slug,
            nome: h_nome.clone(),
            nivel: h_nivel.clone(),
            registro: Some(registro(
                "equipe",
                format!(
                    "{porque}: fui para {h_nome} Nv {nivel_txt} para {}.",
                    if fase == "melhorar" {
                        "tentar um melhor"
                    } else {
                        "capturar"
                    }
                ),
                format!(
                    "Não consegui ir para {h_nome} Nv {nivel_txt} ({{r}}); tento de novo depois."
                ),
            )),
        });
        laco.marcar(nick, "equipeTroca", agora);
        if bola_estava {
            let a_novo = automacao(conta).or(au);
            out.push(comando(
                nick,
                auto_set(a_novo, &[("autoBallSemParar", json!(true))]),
                None,
            ));
        }
        return out;
    }
    if au.is_some() && !verd(au.and_then(|x| x.get("autoBallSemParar"))) {
        out.push(comando(
            nick,
            auto_set(au, &[("autoBallSemParar", json!(true))]),
            Some(registro(
                "equipe",
                format!("Liguei a bola automática para capturar {a_nome}."),
                "Não consegui ligar a bola automática ({r}).".into(),
            )),
        ));
        // RESPOSTA DO JOGO: ok.
        st.equipe_bola = true;
    }
    let h_txt = num_s(js::mais(Some(&h_nivel)));
    if fase == "capturar" {
        let t0 = *st.equipe_capturando.entry(a_sp).or_insert(agora);
        let max_min = ou(js::mais(c.get("capturarMaxMin")), 25.0);
        if agora - t0 > max_min * 60e3 {
            st.equipe_desistiu.insert(a_sp, agora + 6.0 * 3.6e6);
            st.equipe_capturando.remove(&a_sp);
            out.push(registrar(
                nick,
                "equipe",
                format!(
                    "Passei {} min em {h_nome} Nv {h_txt} sem capturar {a_nome}. Desisto dele por 6 h e volto a farmar (a bola continua ligada).",
                    num_s(max_min)
                ),
                true,
            ));
        }
    }
    if fase == "melhorar" {
        let t0 = *st.equipe_melhorando.entry(a_sp).or_insert(agora);
        let limite = n0(c.get("melhorarMaxMin")) * 60e3;
        if limite > 0.0 && agora - t0 > limite {
            st.equipe_desistiu.insert(a_sp, agora + 6.0 * 3.6e6);
            st.equipe_melhorando.remove(&a_sp);
            out.push(registrar(
                nick,
                "equipe",
                format!(
                    "Passei {} min tentando um {a_nome} melhor e não veio nenhum acima de {}%. Fico com o que tenho e sigo; tento de novo daqui a 6 h.",
                    num_s(js::round(limite / 60e3)),
                    num_s(js::round(js::mais(e.get("alvoPct")) * 100.0))
                ),
                true,
            ));
        }
    }
    out
}

/// `motorRodizio(conta)`.
///
/// RESPOSTA DO JOGO: `comando(...)` e `trocarHunt(...)` assumidos `'ok'`. Os ramos de falha
/// (`r1 !== 'ok'` e o texto de falha da troca) ficam no `registro` das decisões. O legado
/// espera 4 s entre a troca de hunt e o `team.active`; aqui as duas decisões saem juntas.
/// Na troca de hunt seguida de `team.active`, o `texto_ok` do `TrocarHunt` fica vazio (o
/// legado só registra a falha dela; o texto de sucesso vem do `team.active`).
pub fn motor_rodizio(
    d: &Value,
    laco: &mut EstadoLaco,
    r: &Relogio,
    conta: &ContaViva,
    dex: &Pokedex,
    st: &mut EstadoEquipe,
) -> Vec<Decisao> {
    let mut out: Vec<Decisao> = Vec::new();
    let nick = conta.nick.as_str();
    let agora = r.agora_ms as f64;
    let c = cfg_rodizio(d, nick);
    if !verd(c.get("ativo")) || !conta_de_boss(nick) {
        return out;
    }
    if equipe_no_comando(laco, r, nick, st) {
        return out;
    }
    let em = laco.marca(nick, "rodizioEm");
    if em != 0.0 && agora - em < js::mais(c.get("intervaloMin")) * 60e3 {
        return out;
    }
    laco.marcar(nick, "rodizioEm", agora);
    let p = plano_rodizio(d, conta, dex);
    let equipe: &[Value] = p
        .get("equipe")
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice);
    if verd(p.get("bloqueio")) || equipe.is_empty() {
        return out;
    }
    if let Some(Value::Array(mover)) = p.get("precisaMover") {
        if !mover.is_empty() {
            for pm in mover {
                let nome = txt(pm.get("nome"));
                out.push(comando(
                    nick,
                    json!({"t": "team.add", "pokemonId": pm.get("id").cloned().unwrap_or(Value::Null)}),
                    Some(registro(
                        "rodizio",
                        format!(
                            "Adicionei {nome} (Nv {}) da coleção para a equipe para subir de nível no rodízio.",
                            txt(pm.get("lvl"))
                        ),
                        format!("Não consegui mover {nome} para a equipe ({{r}}). Talvez a equipe esteja cheia ou o pokemon não existe."),
                    )),
                ));
            }
            return out;
        }
    }
    let ativo = equipe.iter().find(|e| verd(e.get("ativo")));
    let bloq = |st: &EstadoEquipe, id: Option<&Value>| {
        st.rodizio_bloq.get(&txt(id)).copied().unwrap_or(f64::NAN) > agora
    };
    st.rodizio_trocas.retain(|t| agora - t.2 < 20.0 * 60e3);
    let troca_ativa = st
        .rodizio_trocas
        .iter()
        .find(|t| ativo.is_some_and(|a| igual_estrito(Some(&t.0), a.get("id"))))
        .cloned();
    if let (Some(ta), Some(a)) = (troca_ativa, ativo) {
        let dd = jogo::janela(conta, agora - ta.2, agora, agora).desm;
        if dd >= 1 {
            st.rodizio_bloq
                .insert(txt(Some(&ta.0)), agora + 2.0 * 3.6e6);
            st.rodizio_trocas
                .retain(|t| !igual_estrito(Some(&t.0), Some(&ta.0)));
            laco.marcar(nick, "rodizioTroca", 0.0);
            out.push(registrar(
                nick,
                "rodizio",
                format!(
                    "{} desmaiou {dd}× nesta hunt (nível alto demais). Ele fica 2 h fora do rodízio; próximo da fila entra.",
                    txt(a.get("nome"))
                ),
                true,
            ));
        }
    }
    let faltam_de = |e: &Value| js::mais(e.get("faltam"));
    if let Some(a) = ativo {
        if faltam_de(a) > 0.0 && !bloq(st, a.get("id")) {
            return out;
        }
    }
    let alguem_falta = equipe.iter().any(|e| faltam_de(e) > 0.0);
    let pronta = verd(p.get("pronta"));
    if st.rodizio_volta.is_some() && (pronta || !alguem_falta) {
        let forte = st.rodizio_volta.take().unwrap_or(Value::Null);
        let texto = "Equipe toda na meta: devolvi o comando ao pokemon principal e soltei a hunt automática.".to_owned();
        if verd(Some(&forte)) {
            out.push(comando(
                nick,
                json!({"t": "team.active", "pokemonId": forte}),
                Some(registro("rodizio", texto.clone(), texto)),
            ));
        } else {
            out.push(registrar(nick, "rodizio", texto, true));
        }
        return out;
    }
    if pronta {
        let pedido = Decisao::Alerta {
            nick: nick.to_owned(),
            chave: "equipe".into(),
            titulo: "Equipe pronta".into(),
            texto: format!(
                "Os {} pokemon chegaram na meta para o {} (Nv {}).",
                equipe.len(),
                txt(p.pointer("/boss/nome")),
                txt(p.pointer("/boss/teamLevel"))
            ),
            minutos: 240,
        };
        out.extend(alerta(d, laco, r, pedido));
        return out;
    }
    let pokes = pokemons(conta);
    let por_id = |id: Option<&Value>| pokes.iter().find(|x| igual_estrito(x.get("id"), id));
    // PRIORIDADE ABSOLUTA: o mais fraco da equipe que ainda falta e não está de castigo.
    let mut candidatos: Vec<&Value> = equipe
        .iter()
        .filter(|e| faltam_de(e) > 0.0 && !bloq(st, e.get("id")))
        .collect();
    ordenar(&mut candidatos, |x, y| {
        js::mais(x.get("lvl")) - js::mais(y.get("lvl"))
    });
    let Some(alvo) = candidatos.first().copied() else {
        let pulou = laco.marca(nick, "rodizioPulou") + 1.0;
        laco.marcar(nick, "rodizioPulou", pulou);
        if alguem_falta && pulou % 30.0 == 1.0 {
            out.push(registrar(
                nick,
                "rodizio",
                "Todos os pokemon que faltam subir estão de castigo por desmaio; fiquei com o ativo até liberar.".into(),
                true,
            ));
        }
        return out;
    };
    let h = jogo::hunt_atual(conta);
    let vazio = json!({});
    let poke_alvo = por_id(alvo.get("id")).unwrap_or(&vazio);
    let alvo_h = hunt_para_nivelar(d, conta, dex, poke_alvo);
    let slug = alvo_h
        .as_ref()
        .and_then(|(n, v)| jogo::slug_de(conta, n, Some(*v)))
        .filter(|s| !s.is_empty());
    let k = constantes(conta);
    let hunt_serve = match (&alvo_h, &h) {
        (Some((an, av)), Some((hn, hl))) => {
            let hl = hl.unwrap_or(0.0);
            (norm_texto(an) == norm_texto(hn) && *av == hl)
                || (*av < hl * 1.15 && aguenta_hunt_de(dex, &k, poke_alvo, hn, hl))
        }
        _ => false,
    };
    let ja_ativo = ativo.is_some_and(|a| igual_estrito(alvo.get("id"), a.get("id")));
    if ja_ativo && hunt_serve {
        return out;
    }
    let ativo_na_meta = ativo.is_none_or(|a| faltam_de(a) <= 0.0 || bloq(st, a.get("id")));
    let troca_em = laco.marca(nick, "rodizioTroca");
    if !ativo_na_meta && agora - troca_em < 5.0 * 60e3 {
        return out;
    }
    if ativo_na_meta && agora - troca_em < 60e3 {
        return out;
    }
    let alvo_id = alvo.get("id").cloned().unwrap_or(Value::Null);
    let de = ativo
        .and_then(|a| a.get("id"))
        .cloned()
        .unwrap_or(Value::Null);
    if let (Some((an, av)), Some(slug)) = (&alvo_h, slug) {
        if !hunt_serve {
            // troca hunt E pokemon juntos
            if st.rodizio_volta.is_none() && ativo.is_some_and(|a| faltam_de(a) <= 0.0) && !ja_ativo
            {
                st.rodizio_volta = ativo.and_then(|a| a.get("id")).cloned();
            }
            let falha = format!(
                "Não consegui levar a conta para {an} Nv {} ({{r}}); mantive o ativo.",
                num_s(*av)
            );
            let texto = format!(
                "{} (Nv {} → {}) é o mais fraco da equipe: levei a conta para {an} Nv {}, a hunt certa para ele{}.",
                txt(alvo.get("nome")),
                txt(alvo.get("lvl")),
                txt(alvo.get("meta")),
                num_s(*av),
                if ja_ativo { "" } else { ", e ele entrou como ativo" }
            );
            out.push(Decisao::TrocarHunt {
                nick: nick.to_owned(),
                slug,
                nome: an.clone(),
                nivel: jn(*av),
                registro: Some(registro(
                    "rodizio",
                    if ja_ativo {
                        texto.clone()
                    } else {
                        String::new()
                    },
                    falha,
                )),
            });
            laco.marcar(nick, "rodizioTroca", agora);
            if !ja_ativo {
                out.push(comando(
                    nick,
                    json!({"t": "team.active", "pokemonId": alvo_id}),
                    Some(registro("rodizio", texto.clone(), texto)),
                ));
                st.rodizio_trocas.push((alvo_id, de, agora));
            }
            return out;
        }
    }
    // Sem hunt melhor para ele: ao menos põe ele como ativo onde estamos.
    if ja_ativo {
        return out;
    }
    laco.marcar(nick, "rodizioTroca", agora);
    st.rodizio_trocas.push((alvo_id.clone(), de, agora));
    let quem = match ativo {
        Some(a) => format!(
            "{} chegou na meta (Nv {})",
            txt(a.get("nome")),
            txt(a.get("lvl"))
        ),
        None => "Sem ativo na meta".to_owned(),
    };
    let texto = format!(
        "{quem}: {} (Nv {}, meta {}) agora sobe para o {}.",
        txt(alvo.get("nome")),
        txt(alvo.get("lvl")),
        txt(alvo.get("meta")),
        txt(p.pointer("/boss/nome"))
    );
    out.push(comando(
        nick,
        json!({"t": "team.active", "pokemonId": alvo_id}),
        Some(registro("rodizio", texto.clone(), texto)),
    ));
    out
}

#[cfg(test)]
mod testes {
    use super::*;

    fn dex() -> Pokedex {
        let esp = |d: i64, n: &str, t: &str, h: Option<i64>| {
            json!({"d": d, "n": n, "t": [t], "s": [80, 80, 80, 80, 80, 80], "h": h,
                   "c": "pokebola", "b": [0, 0, 0, 0, 1.88],
                   "m": [["Tackle", 40, 2, "F", "NORMAL"], ["Ember", 40, 2, "S", "FIRE"]]})
        };
        Pokedex::de_valor(json!([
            esp(1, "Alfa", "FIRE", Some(10)),
            esp(2, "Regirock", "ROCK", None),
            esp(3, "Beta", "WATER", Some(20))
        ]))
        .unwrap_or_default()
    }

    fn conta(nick: &str) -> ContaViva {
        ContaViva {
            nick: nick.into(),
            ultimo: json!({"trLv": 400, "hdx": {"pokemons": [
                {"id": 10, "speciesId": 1, "level": 50, "slot": 0, "quality": 1.2},
                {"id": 11, "speciesId": 3, "level": 30, "quality": 1.1}]},
                "io": {"player": {"activeId": 10, "travados": [11]}}}),
            ..Default::default()
        }
    }

    #[test]
    fn log10_bate_com_o_v8() {
        assert_eq!(log10(1000.0), 3.0);
        assert_eq!(log10(300.0), 2.477_121_254_719_662_4);
        assert_eq!(log10(10.0), 1.0);
    }

    #[test]
    fn xp_total_como_o_legado() {
        assert_eq!(
            xp_total(1.0),
            50.0 / 3.0 * 0.0_f64 + js::round(50.0 / 3.0 * (1.0 - 6.0 + 17.0 - 12.0))
        );
        assert_eq!(
            xp_total(100.0),
            js::round(50.0 / 3.0 * (1e6 - 6e4 + 1700.0 - 12.0))
        );
    }

    #[test]
    fn iv_e_potencia_como_o_ver() {
        assert_eq!(
            iv_arr(Some(&json!([1, 2, 3, 4, 5, 6]))),
            Some([1.0, 2.0, 3.0, 4.0, 5.0, 6.0])
        );
        assert_eq!(iv_arr(Some(&json!([1, 2]))), None);
        let o = json!({"hp": 1, "atk": 2, "def": 3, "spa": 4, "spdef": 5, "spe": 6});
        assert_eq!(iv_arr(Some(&o)), Some([1.0, 2.0, 3.0, 4.0, 5.0, 6.0]));
        assert_eq!(iv_arr(Some(&json!({"hp": 1}))), None);
        assert_eq!(pot_of(&json!({"potencia": "x3"})), 3.0);
        assert_eq!(pot_of(&json!({"potencia": null})), 1.0);
        assert_eq!(pot_of(&json!({})), 1.0);
    }

    #[test]
    fn conta_de_farm_so_farma() {
        let d = config::carregar(&json!({}));
        let p = plano_rodizio(&d, &conta("Ana"), &dex());
        assert_eq!(p["ativo"], json!(false));
        assert_eq!(
            p["bloqueio"],
            json!("Esta conta só farma: equipe e rodízio de boss são só da Alllphhha.")
        );
        let r = Relogio {
            agora_ms: 1_000_000,
            fuso_ms: 0,
        };
        let e = plano_equipe(&d, &r, &conta("Ana"), &dex(), &HashMap::new());
        assert_eq!(e["bloqueio"], p["bloqueio"]);
    }

    #[test]
    fn plano_rodizio_da_conta_de_boss() {
        let d = config::carregar(&json!({}));
        let c = conta("Alllphhha");
        let p = plano_rodizio(&d, &c, &dex());
        assert_eq!(p["boss"]["key"], json!("regirock"));
        assert_eq!(p["boss"]["teamLevel"], json!(300));
        let eq = p["equipe"].as_array().map(Vec::len);
        assert_eq!(eq, Some(2));
        assert_eq!(p["equipe"][0]["meta"], json!(300));
        assert_eq!(p["equipe"][0]["ativo"], json!(true));
        assert_eq!(p["equipe"][0]["faltam"], json!(250));
        assert_eq!(p["pronta"], json!(false));
        // forca = 50/300 + 30/300; mult = 3^(5 - forca)
        assert_eq!(p["forca"], json!(0.27));
    }

    #[test]
    fn priorizar_usa_a_lista_conhecida_sem_painel() {
        let d = config::carregar(&json!({}));
        let l = priorizar_bosses(&d, &conta("Alllphhha"));
        let a = l.as_array().map_or(0, Vec::len);
        assert_eq!(a, 25);
        assert_eq!(l[0]["key"], json!("regirock"));
        assert_eq!(l[0]["proximo"], json!(true));
        assert_eq!(l[0]["semDrops"], json!(true));
        assert_eq!(l[0]["temNivel"], json!(true));
        assert_eq!(l[0]["nome"], json!("Regirock"));
    }

    #[test]
    fn luta_simples_e_determinista() {
        let dx = dex();
        let k = Constantes::wiki();
        let (Some(a), Some(b)) = (dx.por_id(1), dx.por_id(2)) else {
            panic!("dex de teste");
        };
        let me = Combatente::novo(a, 100.0, &Opcoes::default(), &k);
        let foe = mon_selvagem(b, 10.0, &k);
        let (Some(r1), Some(r2)) = (lutar(&me, &foe, 60.0, &k), lutar(&me, &foe, 60.0, &k)) else {
            panic!("luta");
        };
        assert!(r1.kph > 0.0);
        assert_eq!(r1.kph, r2.kph);
        assert_eq!(r1.burst, r2.burst);
        assert!(dps_out(&me, &foe, 60.0, &k).is_some_and(|x| x > 0.0));
    }

    #[test]
    fn rodizio_traz_o_mais_fraco_para_ativo() {
        let d = config::carregar(
            &json!({"config": {"Alllphhha": {"rodizio": {"ativo": true, "intervaloMin": 1}}}}),
        );
        let dx = dex();
        let mut c = conta("Alllphhha");
        c.ultimo["hdx"]["pokemons"][1]["slot"] = json!(1);
        c.ultimo["hdx"]["pokemons"][0]["level"] = json!(300);
        let mut laco = EstadoLaco::default();
        let mut st = EstadoEquipe::default();
        let r = Relogio {
            agora_ms: 10_000_000,
            fuso_ms: 0,
        };
        let v = motor_rodizio(&d, &mut laco, &r, &c, &dx, &mut st);
        // o ativo já está na meta: o mais fraco (id 11) passa a ativo
        assert!(
            v.iter()
                .any(|x| matches!(x, Decisao::Comando { comando, .. }
                if comando["t"] == json!("team.active") && comando["pokemonId"] == json!(11))
                    || matches!(x, Decisao::TrocarHunt { .. })),
            "{v:?}"
        );
        // dentro do intervalo, nada
        assert!(motor_rodizio(&d, &mut laco, &r, &c, &dx, &mut st).is_empty());
    }

    #[test]
    fn motor_equipe_de_conta_farm_so_limpa_uma_vez() {
        let d = config::carregar(&json!({}));
        let mut c = conta("Ana");
        c.ultimo["hdx"]["eu"] = json!({"automation": {"desistiuHunt": true, "x": 1}});
        let mut laco = EstadoLaco::default();
        let mut st = EstadoEquipe::default();
        let r = Relogio {
            agora_ms: 10_000_000,
            fuso_ms: 0,
        };
        let v = motor_equipe(&d, &mut laco, &r, &c, &dex(), &mut st, false);
        assert_eq!(v.len(), 2);
        assert!(matches!(&v[0], Decisao::Comando { comando, .. }
            if comando == &json!({"t": "auto.set", "desistiuHunt": false, "x": 1})));
        assert!(motor_equipe(&d, &mut laco, &r, &c, &dex(), &mut st, false).is_empty());
    }

    /// Compara com o legado nas fixtures reais (`NEXUS_FIXTURES_MOTOR` = pasta da captura).
    #[test]
    #[ignore = "precisa das fixtures do PC (NEXUS_FIXTURES_MOTOR)"]
    fn fixtures_reais_do_legado() {
        let Ok(pasta) = std::env::var("NEXUS_FIXTURES_MOTOR") else {
            return;
        };
        let ler = |nome: &str| -> Option<Value> {
            serde_json::from_str(&std::fs::read_to_string(format!("{pasta}/{nome}")).ok()?).ok()
        };
        let pok = std::env::var("NEXUS_POKEDEX")
            .unwrap_or_else(|_| "/opt/holodex-nexus/web/pokedex.json".into());
        let dx = Pokedex::de_json(&std::fs::read_to_string(pok).expect("pokedex")).expect("dex");
        let d = config::carregar(&ler("nexus-dados.json").expect("dados"));
        let janela = ler("nexus-janela.json").expect("janela");
        let estado = ler("estado.json").expect("estado");
        let agora = janela["agora"].as_i64().expect("agora");
        let r = Relogio {
            agora_ms: agora,
            fuso_ms: -3 * 3_600_000,
        };
        let mut falhas = 0;
        for cj in janela["contas"].as_array().expect("contas") {
            let Some(c) = ContaViva::de_json(cj) else {
                continue;
            };
            let nick = c.nick.clone();
            let leg = estado["contas"]
                .as_array()
                .and_then(|a| a.iter().find(|x| x["nick"] == json!(nick)))
                .cloned()
                .unwrap_or(Value::Null);
            let mut confere = |rotulo: &str, esperado: Option<Value>, obtido: Value| {
                let Some(esp) = esperado else {
                    println!("{nick} {rotulo}: sem fixture");
                    return;
                };
                match primeira_diferenca(&esp, &obtido, "") {
                    None => println!("{nick} {rotulo}: IGUAL"),
                    Some(m) => {
                        falhas += 1;
                        println!("{nick} {rotulo}: DIFERE em {m}");
                    }
                }
            };
            confere(
                "rodizio",
                leg.get("rodizio").cloned(),
                plano_rodizio(&d, &c, &dx),
            );
            confere(
                "equipe",
                leg.get("equipe").cloned(),
                plano_equipe(&d, &r, &c, &dx, &HashMap::new()),
            );
            // a captura do `equipe-melhor` é de outro instante (a lista de pokémons mudou)
            let mut obt = melhor_time_possivel(&d, &c, &dx, None);
            let mut esp = ler(&format!("equipe-melhor-{nick}.json"));
            for v in [&mut obt].into_iter().chain(esp.iter_mut()) {
                if let Some(o) = v.as_object_mut() {
                    o.remove("totalPokemon");
                    o.remove("totalPorLocal");
                }
            }
            confere("equipe-melhor (sem totais)", esp, obt);
            confere(
                "builds-prioridade",
                ler(&format!("builds-prioridade-{nick}.json")),
                json!({"lista": priorizar_bosses(&d, &c)}),
            );
            let mut esp = ler(&format!("combate-{nick}.json"));
            // o instante do legado é o `t` da própria captura
            let t_cap = esp.as_ref().and_then(|e| e["t"].as_i64()).unwrap_or(agora);
            let rc = Relogio {
                agora_ms: t_cap,
                fuso_ms: r.fuso_ms,
            };
            let mut co = combate_de(&d, &rc, &c, &dx, false);
            if let Some(o) = esp.as_mut().and_then(Value::as_object_mut) {
                o.remove("contas");
            }
            // o estado ao vivo anda entre a janela e a captura: HP, XP, estoques, sessão e ritmo
            // (calculados do HUD) ficam de fora; o resto (stats, luta, boss) tem de bater.
            for v in [&mut co].into_iter().chain(esp.iter_mut()) {
                if let Some(o) = v.as_object_mut() {
                    for k in ["t", "sessao", "ritmo", "pocoes", "bolas"] {
                        o.remove(k);
                    }
                    for k in ["hp", "xp"] {
                        if let Some(a) = o.get_mut("ativo").and_then(Value::as_object_mut) {
                            a.remove(k);
                        }
                    }
                }
            }
            confere("combate (sem hp, xp, sessão, ritmo, estoques)", esp, co);
        }
        assert_eq!(falhas, 0, "{falhas} diferenças");
    }

    fn primeira_diferenca(a: &Value, b: &Value, caminho: &str) -> Option<String> {
        match (a, b) {
            (Value::Object(x), Value::Object(y)) => {
                for (k, v) in x {
                    match y.get(k) {
                        None => return Some(format!("{caminho}/{k}: falta no Rust")),
                        Some(w) => {
                            if let Some(m) = primeira_diferenca(v, w, &format!("{caminho}/{k}")) {
                                return Some(m);
                            }
                        }
                    }
                }
                y.keys()
                    .find(|k| !x.contains_key(*k))
                    .map(|k| format!("{caminho}/{k}: sobra no Rust"))
            }
            (Value::Array(x), Value::Array(y)) => {
                if x.len() != y.len() {
                    return Some(format!("{caminho}: tamanho {} x {}", x.len(), y.len()));
                }
                x.iter()
                    .zip(y)
                    .enumerate()
                    .find_map(|(i, (v, w))| primeira_diferenca(v, w, &format!("{caminho}[{i}]")))
            }
            (Value::Number(x), Value::Number(y)) if x.as_f64() == y.as_f64() => None,
            _ if a == b => None,
            _ => Some(format!("{caminho}: legado {a} x Rust {b}")),
        }
    }
}
