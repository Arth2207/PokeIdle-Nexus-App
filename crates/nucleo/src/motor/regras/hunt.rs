//! Regras do motor: frente hunt (etapa 4). `planoHunt` e `motorHunt` do legado.
//!
//! Tudo é puro. A pokédex e as constantes do servidor chegam por parâmetro
//! ([`ContextoHunt`]), assim como o que vem de outras frentes (preço da poção e da bola do
//! `planoCompra`, limite do rodízio do `planoRodizio`, `equipeNoComando`).
//!
//! Em sombra o Nexus não grava o `D`: o que o legado gravaria (medidas, bloqueios, campos da
//! ação `hunt`, calibração) sai em [`SaidaHunt::gravacoes`] como [`GravacaoD`]. Dependências do
//! jogo: toda troca de hunt assume a resposta `"ok"`; a lista de linhas do HUD (`conta.linhas`)
//! precisa cobrir 60 min (a janela lenta) e 12 h de `D.medidas` já vêm do `D`.

use std::collections::{HashMap, VecDeque};

use serde::Serialize;
use serde_json::{json, Map, Value};

use super::{ContaViva, Decisao, EstadoLaco, Registro};
use crate::combate::combatente::{dano, dano_investida, tipo_investida, Combatente, Opcoes};
use crate::combate::especie::{EspecieCombate, Pokedex};
use crate::combate::tipos::Constantes;
use crate::js;
use crate::motor::config;
use crate::motor::jogo::{hunt_atual, janela, num, slug_de, Janela};
use crate::motor::Relogio;

const TESTE_MIN: f64 = 5.0;
const CALIB_PADRAO: f64 = 0.75;
/// `OBJETIVOS`: chave, nome, unidade.
const OBJETIVOS: [(&str, &str, &str); 5] = [
    ("xp", "XP", "XP/h"),
    ("lucro", "lucro", "ouro/h"),
    ("equilibrio", "XP sem prejuízo", "XP/h"),
    ("gemas", "gemas", "gemas/h"),
    ("shiny", "chance de shiny", "shiny/dia"),
];
const RAROS_OUT: [(&str, f64); 4] = [
    ("bronzeToken", 0.0005),
    ("fragChave", 0.000005),
    ("fragShinyStone", 0.000005),
    ("fragBicicleta", 0.000005),
];
const OUTLANDS: [(f64, f64); 8] = [
    (150.0, 1.0),
    (1000.0, 1.25),
    (2000.0, 1.5),
    (4000.0, 2.0),
    (8000.0, 3.0),
    (16000.0, 4.0),
    (24000.0, 6.0),
    (36000.0, 8.0),
];
const CURVA_OURO: [(f64, f64); 6] = [
    (1.0, 41.0),
    (100.0, 658.0),
    (1000.0, 2624.0),
    (5000.0, 6900.0),
    (25000.0, 18142.0),
    (160300.0, 31681.0),
];
const HORA: f64 = 3.6e6;

fn objetivo(chave: &str) -> Option<(&'static str, &'static str, &'static str)> {
    OBJETIVOS.iter().copied().find(|o| o.0 == chave)
}

/// Preço da poção e da bola em uso (`precosEmUso`, do `planoCompra`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Precos {
    pub pot: f64,
    pub ball: f64,
}

/// O que as regras de hunt precisam além da conta e do `D`.
pub struct ContextoHunt<'a> {
    pub dex: &'a Pokedex,
    /// Constantes do servidor da conta (veja [`constantes_da_conta`]).
    pub k: &'a Constantes,
    pub precos: Precos,
    /// `limiteRodizio`: nível do mais fraco que o rodízio ainda vai subir (`None` sem rodízio).
    pub limite_rodizio: Option<f64>,
    /// `equipeNoComando(conta)`.
    pub equipe_no_comando: bool,
    /// `conta.rodizioVolta` truthy.
    pub rodizio_volta: bool,
    /// `conta.huntDesde` (0 = ausente).
    pub hunt_desde: f64,
}

/// Gravação no `D` que o legado faz e a sombra não executa. `alvo`:
/// `medida` (`{chave, medida}` em `D.medidas[nick]`), `medHist` (`{chave, ponto: [t, xpH]}`),
/// `bloqueio` (`{chave, ate}` em `D.bloqueio`), `acao` (campos a mesclar na última ação `hunt`
/// ok da conta), `acaoNova` (campos extras da ação criada pela troca que veio logo antes em
/// `decisoes`) e `calibracao` (entrada a empilhar em `D.calib2[nick]`, máx. 40).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GravacaoD {
    pub nick: String,
    pub alvo: String,
    pub dados: Value,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct SaidaHunt {
    pub decisoes: Vec<Decisao>,
    pub gravacoes: Vec<GravacaoD>,
}

// ---------------------------------------------------------------- helpers do JS

fn verd(x: f64) -> bool {
    x != 0.0 && !x.is_nan()
}
fn ou(x: Option<f64>, padrao: f64) -> f64 {
    x.filter(|v| verd(*v)).unwrap_or(padrao)
}
fn min_js(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else {
        a.min(b)
    }
}
fn max_js(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else {
        a.max(b)
    }
}
fn jn(x: f64) -> Value {
    js::json_num(x)
}
fn jo(x: Option<f64>) -> Value {
    x.map_or(Value::Null, jn)
}
fn nm(v: &Value, k: &str) -> f64 {
    js::mais(v.get(k))
}
fn ver(v: &Value, k: &str) -> bool {
    js::verdadeiro(v.get(k))
}
fn txt(v: &Value, k: &str) -> String {
    js::texto(v.get(k))
}
fn nv(x: f64) -> String {
    js::numero(x)
}

/// `Math.round(v).toLocaleString('pt-BR')`.
fn fmt_br(v: f64) -> String {
    let r = js::round(v);
    if r.is_nan() {
        return "NaN".into();
    }
    if r.is_infinite() {
        return if r > 0.0 { "∞".into() } else { "-∞".into() };
    }
    let digitos = format!("{:.0}", r.abs());
    let n = digitos.len();
    let mut s = String::new();
    for (i, c) in digitos.chars().enumerate() {
        if i > 0 && (n - i) % 3 == 0 {
            s.push('.');
        }
        s.push(c);
    }
    if r.is_sign_negative() || (r == 0.0 && v < 0.0) {
        format!("-{s}")
    } else {
        s
    }
}
/// `Math.round(v || 0).toLocaleString('pt-BR')`.
fn fmt0(v: f64) -> String {
    fmt_br(if verd(v) { v } else { 0.0 })
}

/// `Array.prototype.sort` do V8 (inserção binária, estável) com comparador numérico.
fn ordenar_como_js<T>(v: &mut Vec<T>, cmp: impl Fn(&T, &T) -> f64) {
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

// ---------------------------------------------------------------- simulador (fight)

/// Resultado do `S_.fight` que a hunt usa.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Luta {
    pub kph: f64,
    pub tkill: Option<f64>,
    pub dps_in: f64,
    pub burst: f64,
    pub hp: f64,
    /// `bate`: melhor efetividade do atacante.
    pub bate: f64,
    /// `pior`: `burst / hp`.
    pub pior: f64,
}

struct Selvagem {
    hp: f64,
    pronto: Vec<f64>,
    prox: f64,
    inv: f64,
}

/// `S_.fight(me, foe, secs)` (dt = 0,1), com `pior = burst / hp`.
pub fn luta(me: &Combatente, foe: &Combatente, secs: f64, k: &Constantes) -> Luta {
    let dt = 0.1;
    let mut pronto: HashMap<&str, f64> = HashMap::new();
    for g in &me.golpes {
        pronto.insert(g.nome.as_str(), 0.0);
    }
    let meu: Vec<f64> = me
        .golpes
        .iter()
        .map(|g| dano(me, foe, g.poder, g.categoria, g.tipo, me.stab(g.tipo), k))
        .collect();
    let dele: Vec<f64> = foe
        .golpes
        .iter()
        .map(|g| dano(foe, me, g.poder, g.categoria, g.tipo, foe.stab(g.tipo), k))
        .collect();
    let minha_inv = dano_investida(me, foe, k);
    let inv_dele = dano_investida(foe, me, k);
    let (mut t, mut kills, mut tomado) = (0.0_f64, 0u64, 0.0_f64);
    let (mut prox_me, mut prox_inv, mut max_burst) = (0.0_f64, 0.0_f64, 0.0_f64);
    let mut w: Option<Selvagem> = None;
    let mut janela_dano: VecDeque<(f64, f64)> = VecDeque::new();
    let mut soma = 0.0_f64;
    while t < secs {
        let sel = w.get_or_insert_with(|| Selvagem {
            hp: foe.hp,
            pronto: foe.golpes.iter().map(|_| t).collect(),
            prox: t,
            inv: t,
        });
        if t >= prox_me {
            let mut bi: Option<usize> = None;
            for (i, g) in me.golpes.iter().enumerate() {
                let livre = pronto.get(g.nome.as_str()).copied().unwrap_or(f64::NAN) <= t;
                if livre && bi.is_none_or(|b| meu[i] > meu[b]) {
                    bi = Some(i);
                }
            }
            if let Some(b) = bi {
                sel.hp -= meu[b];
                if let Some(g) = me.golpes.get(b) {
                    pronto.insert(g.nome.as_str(), t + g.recarga * me.mult_recarga);
                }
            }
            prox_me = t + me.intervalo;
        }
        if t >= prox_inv {
            sel.hp -= minha_inv;
            prox_inv = t + me.intervalo_investida;
        }
        let mut hit = 0.0_f64;
        if t >= sel.prox {
            let mut bi: Option<usize> = None;
            for i in 0..foe.golpes.len() {
                if sel.pronto[i] <= t && bi.is_none_or(|b| dele[i] > dele[b]) {
                    bi = Some(i);
                }
            }
            if let Some(b) = bi {
                hit += dele[b];
                sel.pronto[b] = t + foe.golpes[b].recarga * foe.mult_recarga;
            }
            sel.prox = t + foe.intervalo;
        }
        if t >= sel.inv {
            hit += inv_dele;
            sel.inv = t + foe.intervalo_investida;
        }
        tomado += hit;
        janela_dano.push_back((t, hit));
        soma += hit;
        while janela_dano.front().is_some_and(|p| p.0 < t - 5.0) {
            if let Some(p) = janela_dano.pop_front() {
                soma -= p.1;
            }
        }
        if soma > max_burst {
            max_burst = soma;
        }
        if sel.hp <= 0.0 {
            kills += 1;
            w = None;
        }
        t += dt;
    }
    // `coverage(me, foe)[0]`: a investida primeiro, depois os golpes que passam dela.
    let mut bate = tipo_investida(me, foe, k).map_or(f64::NAN, |(_, e)| e);
    for g in &me.golpes {
        let e = k.efetividade_hunt(g.tipo, &foe.tipos);
        if e > bate {
            bate = e;
        }
    }
    Luta {
        kph: kills as f64 * 3600.0 / secs,
        tkill: (kills > 0).then(|| secs / kills as f64),
        dps_in: tomado / secs,
        burst: max_burst,
        hp: me.hp,
        bate,
        pior: max_burst / me.hp,
    }
}

/// `setServerConstants(conta.pub)`: as constantes da wiki trocadas pelas do servidor.
pub fn constantes_da_conta(conta: &ContaViva) -> Constantes {
    let mut k = Constantes::wiki();
    let p = &conta.publico;
    if p.is_null() {
        return k;
    }
    let hp = js::mais(p.get("multHpSelvagem"));
    if hp > 0.0 {
        k.hp_selvagem = hp;
    }
    let dn = js::mais(p.get("multDanoSelvagem"));
    if dn > 0.0 {
        k.dano_selvagem = dn;
    }
    if let Some(Value::Object(a)) = p.get("ampliacaoHunt") {
        for (chave, v) in a {
            let kk = js::mais(Some(&Value::String(chave.clone())));
            let vv = js::mais(Some(v));
            if kk.is_finite() && vv.is_finite() {
                match k.ampliacao_hunt.iter_mut().find(|(c, _)| *c == kk) {
                    Some(par) => par.1 = vv,
                    None => k.ampliacao_hunt.push((kk, vv)),
                }
            }
        }
    }
    k
}

// ---------------------------------------------------------------- previsões

/// `goldCurve(L)`: ouro por abate pela curva da wiki (log-log).
fn curva_ouro(l: f64) -> f64 {
    let l = max_js(1.0, l);
    let c = &CURVA_OURO;
    if l >= c[c.len() - 1].0 {
        return c[c.len() - 1].1;
    }
    for i in 1..c.len() {
        if l <= c[i].0 {
            let ((x0, y0), (x1, y1)) = (c[i - 1], c[i]);
            let k = js::ln(y1 / y0) / js::ln(x1 / x0);
            return y0 * js::pow(l / x0, k);
        }
    }
    c[0].1
}

/// Retorno do `xpModel`.
#[derive(Debug, Clone, Copy, PartialEq)]
struct ModeloXp {
    k: f64,
    a: f64,
    min_h: f64,
    max_h: f64,
}

impl ModeloXp {
    fn xp(&self, h: f64) -> f64 {
        let base = self.a * js::pow(h, self.k);
        if h >= self.min_h * 0.8 && h <= self.max_h * 1.3 {
            return base;
        }
        let r = if h < self.min_h {
            h / (self.min_h * 0.8)
        } else {
            (self.max_h * 1.3) / h
        };
        base * max_js(0.4, js::pow(r, 1.5))
    }
}

/// `xpModel(obs)` com `obs = [(h, xp)]`.
fn modelo_xp(obs: &[(f64, f64)]) -> Option<ModeloXp> {
    let o: Vec<(f64, f64)> = obs
        .iter()
        .copied()
        .filter(|(h, xp)| *h > 0.0 && *xp > 0.0)
        .collect();
    if o.is_empty() {
        return None;
    }
    let n = o.len() as f64;
    let mut k = 1.0;
    if o.len() >= 2 {
        let xs: Vec<f64> = o.iter().map(|x| js::ln(x.0)).collect();
        let ys: Vec<f64> = o.iter().map(|x| js::ln(x.1)).collect();
        let mx = xs.iter().fold(0.0, |a, b| a + b) / n;
        let my = ys.iter().fold(0.0, |a, b| a + b) / n;
        let (mut num_, mut den) = (0.0, 0.0);
        for i in 0..o.len() {
            num_ += (xs[i] - mx) * (ys[i] - my);
            den += js::pow(xs[i] - mx, 2.0);
        }
        if den > 0.0 {
            k = min_js(3.0, max_js(0.3, num_ / den));
        }
    }
    let soma = o.iter().fold(0.0, |s, x| s + js::ln(x.1) - k * js::ln(x.0));
    let a = js::exp(soma / n);
    let min_h = o.iter().fold(f64::INFINITY, |m, x| min_js(m, x.0));
    let max_h = o.iter().fold(f64::NEG_INFINITY, |m, x| max_js(m, x.0));
    Some(ModeloXp { k, a, min_h, max_h })
}

/// `real` do `forecastHunt`.
struct Real {
    kph: Option<f64>,
    xp_kill: Option<f64>,
    gold_kill: Option<f64>,
    pot_por_kill: Option<f64>,
    bolas_por_kill: Option<f64>,
    hunt_lvl: f64,
}

#[derive(Debug, Clone, Copy)]
struct Previsao {
    kph: f64,
    xp_h: Option<f64>,
    lucro_h: f64,
    pot_h: f64,
}

/// `S_.forecastHunt`.
fn previsao(
    cur: &Luta,
    alt: &Luta,
    alt_lvl: f64,
    real: &Real,
    pr: &Precos,
    f: f64,
    modelo: Option<&ModeloXp>,
) -> Previsao {
    let esticado = alt.kph * f;
    let kph = match real.kph {
        Some(r) if r > 0.0 => min_js(esticado, r * 1.3),
        _ => esticado,
    };
    let gold_ratio = match real.gold_kill {
        Some(g) if verd(g) && verd(real.hunt_lvl) => g / curva_ouro(real.hunt_lvl),
        _ => 1.0,
    };
    let gold_kill = curva_ouro(alt_lvl) * gold_ratio;
    let xp_kill = match modelo {
        Some(m) => Some(m.xp(alt_lvl)),
        None => match real.xp_kill {
            Some(x) if verd(x) && verd(real.hunt_lvl) => Some(x * alt_lvl / real.hunt_lvl),
            _ => None,
        },
    };
    let dmg_cur = cur.tkill.filter(|t| verd(*t)).map(|t| cur.dps_in * t);
    let dmg_alt = alt.tkill.filter(|t| verd(*t)).map(|t| alt.dps_in * t);
    let pot_kill = match (real.pot_por_kill, dmg_cur, dmg_alt) {
        (Some(p), Some(c), Some(a)) if verd(c) && verd(a) => p * a / c,
        (p, _, _) => ou(p, 0.0),
    };
    let pot_h = pot_kill * kph;
    let bola_h = ou(real.bolas_por_kill, 0.0) * kph;
    let gastos = pot_h * ou(Some(pr.pot), 0.0) + bola_h * ou(Some(pr.ball), 0.0);
    let gold_h = gold_kill * kph;
    Previsao {
        kph,
        xp_h: xp_kill.map(|x| x * kph),
        lucro_h: gold_h - gastos,
        pot_h,
    }
}

// ---------------------------------------------------------------- valores por objetivo

/// `G` do legado: `gemaPrecos` mais `_chShiny`.
struct Gemas {
    precos: Value,
    ch_shiny: f64,
}

impl Gemas {
    fn g(&self, k: &str) -> f64 {
        let v = js::mais(self.precos.get(k));
        if verd(v) {
            v
        } else {
            0.0
        }
    }
}

/// Espécie para o `valorGemasH` (`{...sp, trLv}`; `{}` quando não há espécie).
struct EspValor {
    d: Option<i64>,
    tr_lv: f64,
}

fn eh_outland(d: Option<i64>) -> bool {
    d.is_some_and(|d| (2000..3000).contains(&d))
}

/// `valorGemasH(kph, sp, G)`. A pokédex não tem `catchBase`: vale o 0,05 do `||`.
fn valor_gemas_h(kph: Option<f64>, sp: &EspValor, g: &Gemas) -> f64 {
    let Some(kph) = kph.filter(|k| verd(*k)) else {
        return 0.0;
    };
    if eh_outland(sp.d) {
        let nivel = ou(Some(sp.tr_lv), 0.0);
        let mult = OUTLANDS
            .iter()
            .rfind(|(l, _)| nivel >= *l)
            .map_or(1.0, |(_, m)| *m)
            * ou(Some(js::mais(g.precos.get("outlandBonus"))), 1.0);
        let soma = RAROS_OUT.iter().fold(0.0, |s, (k, p)| s + p * g.g(k));
        return kph * soma * mult;
    }
    let p_cap = min_js(1.0, 0.05 * 1.5);
    kph * (1.0 / 24000.0) * p_cap * g.g("shiny")
}

/// `lucroJanela(w, pr)`.
fn lucro_janela(w: &Janela, pr: &Precos) -> Option<f64> {
    let kph = w.kph.filter(|k| verd(*k))?;
    Some(
        kph * (ou(w.gold_kill, 0.0)
            - ou(w.pot_kill, 0.0) * pr.pot
            - ou(w.bolas_kill, 0.0) * pr.ball),
    )
}

/// `valorJanela(w, obj, pr, ch)`.
fn valor_janela(w: &Janela, obj: &str, pr: &Precos, ch: Option<f64>) -> Option<f64> {
    match obj {
        "lucro" => lucro_janela(w, pr),
        "shiny" => Some(ou(w.kph, 0.0) * ou(ch, 1.0 / 24000.0) * 24.0),
        _ => w.xp_h,
    }
}

/// `valorHunt(obj, p, sp, G, kphReal)`.
fn valor_hunt(obj: &str, p: &Previsao, sp: &EspValor, g: &Gemas) -> Option<f64> {
    match obj {
        "gemas" => Some(valor_gemas_h(Some(ou(Some(p.kph), 0.0)), sp, g)),
        "shiny" => Some(if eh_outland(sp.d) {
            0.0
        } else {
            ou(Some(p.kph), 0.0) * ou(Some(g.ch_shiny), 1.0 / 24000.0) * 24.0
        }),
        "lucro" => Some(p.lucro_h),
        _ => p.xp_h,
    }
}

fn encolher(v: Option<f64>, e: f64) -> Option<f64> {
    v.map(|x| if x > 0.0 { x * e } else { x })
}

// ---------------------------------------------------------------- calibração (D.calib2)

/// `calibDoLutador`: as razões `r` do mesmo lutador (espécie e nível ±15%).
fn calib_do_lutador(d: &Value, nick: &str, sp: &str, lvl: f64) -> Vec<f64> {
    let Some(Value::Array(a)) = d.get("calib2").and_then(|c| c.get(nick)) else {
        return Vec::new();
    };
    a.iter()
        .filter(|x| x.is_object())
        .filter_map(|x| {
            let r = js::mais(x.get("r"));
            let bate = x.get("sp").and_then(Value::as_str) == Some(sp)
                && (js::mais(x.get("lvl")) - lvl).abs() <= lvl * 0.15;
            (r.is_finite() && r > 0.0 && bate).then_some(r)
        })
        .collect()
}

/// `fatorCalibracao`: mediana das últimas 5 razões, entre 0,2 e 1,2 (padrão 0,75).
fn fator_calibracao(d: &Value, nick: &str, sp: &str, lvl: f64) -> f64 {
    let todas = calib_do_lutador(d, nick, sp, lvl);
    let ultimas = &todas[todas.len().saturating_sub(5)..];
    if ultimas.is_empty() {
        return CALIB_PADRAO;
    }
    let mut o = ultimas.to_vec();
    o.sort_by(f64::total_cmp);
    let n = o.len();
    let m = if n % 2 == 1 {
        o[(n - 1) / 2]
    } else {
        (o[n / 2 - 1] + o[n / 2]) / 2.0
    };
    min_js(1.2, max_js(0.2, m))
}

/// `registrarCalibracao`: a entrada que iria para `D.calib2[nick]`.
fn registrar_calibracao(
    nick: &str,
    previsto: f64,
    real: Option<f64>,
    sp: &str,
    lvl: f64,
    agora: f64,
) -> Option<GravacaoD> {
    let real = real?;
    if previsto.is_nan() || previsto <= 0.0 || !real.is_finite() || real < 0.0 {
        return None;
    }
    Some(GravacaoD {
        nick: nick.to_owned(),
        alvo: "calibracao".into(),
        dados: json!({ "r": jn(js::round(real / previsto * 1000.0) / 1000.0), "sp": sp, "lvl": jn(lvl), "t": jn(agora) }),
    })
}

// ---------------------------------------------------------------- melhoresHunts

struct Ativo<'a> {
    sp: &'a EspecieCombate,
    lvl: f64,
    q: f64,
    pot: f64,
    iv: [f64; 6],
}

/// `ativoDe(conta)`.
fn ativo_de<'a>(conta: &ContaViva, dex: &'a Pokedex) -> Option<Ativo<'a>> {
    let u = &conta.ultimo;
    let at = u.get("ativo").filter(|a| js::verdadeiro(Some(a)))?;
    let nome = match at.get("nome") {
        Some(v) if js::verdadeiro(Some(v)) => js::texto(Some(v)),
        _ => String::new(),
    };
    let sp = dex.por_nome(&nome)?;
    let v = u
        .get("io")
        .and_then(|i| i.get("active"))
        .and_then(|a| a.get("ivs"));
    let mut iv: Option<[f64; 6]> = None;
    if let Some(Value::Array(a)) = v {
        if a.len() == 6 {
            let mut t = [0.0; 6];
            for (i, x) in a.iter().enumerate() {
                t[i] = js::mais(Some(x));
            }
            iv = Some(t);
        }
    }
    if iv.is_none() {
        if let Some(o @ Value::Object(_)) = v {
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
            if r.iter().all(|x| x.is_some_and(|x| x > 0.0)) {
                let mut t = [0.0; 6];
                for (i, x) in r.iter().enumerate() {
                    t[i] = x.unwrap_or(0.0);
                }
                iv = Some(t);
            }
        }
    }
    let iv = iv.unwrap_or_else(|| {
        let e = max_js(
            1.0,
            min_js(32.0, js::round(ou(num(at.get("iv")), 96.0) / 6.0)),
        );
        [e; 6]
    });
    Some(Ativo {
        sp,
        // `lv` nulo vira 0 nas contas do JS (`null` em aritmética).
        lvl: num(at.get("lv")).unwrap_or(0.0),
        q: ou(num(at.get("q")), 1.1),
        pot: ou(num(at.get("pot")), 1.0),
        iv,
    })
}

struct Alt<'a> {
    sp: &'a EspecieCombate,
    h: f64,
    r: Luta,
}

struct Base<'a> {
    at: Ativo<'a>,
    h_nome: String,
    h_lvl: f64,
    cur: Option<Luta>,
    alts: Vec<Alt<'a>>,
}

/// `melhoresHunts(conta)` (sem o cache, que só evita refazer a simulação).
fn melhores_hunts<'a>(
    d: &Value,
    conta: &ContaViva,
    ctx: &ContextoHunt<'a>,
    hc: &Value,
) -> Option<Base<'a>> {
    let at = ativo_de(conta, ctx.dex)?;
    let (h_nome, h_lvl) = hunt_atual(conta)?;
    // lvl nulo da hunt vira 0 (`null` em aritmética).
    let h_lvl = h_lvl.unwrap_or(0.0);
    let tr = num(conta.ultimo.get("trLv")).filter(|t| verd(*t))?;
    let modo = if ver(hc, "soOutland") {
        "out"
    } else if hc.get("objetivo").and_then(Value::as_str) == Some("shiny") {
        "shiny"
    } else {
        "normal"
    };
    let pot = if at.pot.fract() == 0.0 && (1.0..=5.0).contains(&at.pot) {
        at.pot as u8
    } else {
        0
    };
    let me = Combatente::novo(
        at.sp,
        at.lvl,
        &Opcoes {
            qualidade: at.q,
            ivs: at.iv,
            potencia: pot,
            ..Opcoes::default()
        },
        ctx.k,
    );
    let selvagem = Opcoes {
        qualidade: 1.1,
        selvagem: true,
        ..Opcoes::default()
    };
    let f = |sp: &EspecieCombate, lvl: f64, secs: f64| {
        luta(
            &me,
            &Combatente::novo(sp, lvl, &selvagem, ctx.k),
            secs,
            ctx.k,
        )
    };
    let cur = ctx.dex.por_nome(&h_nome).map(|w| f(w, h_lvl, 180.0));
    let medidas = d
        .get("medidas")
        .and_then(|m| m.get(&conta.nick))
        .and_then(Value::as_object);
    let nvs_med: Vec<f64> = medidas
        .map(|m| {
            m.values()
                .map(|x| nm(x, "h"))
                .filter(|x| *x > 0.0)
                .collect()
        })
        .unwrap_or_default();
    let min_h = nvs_med.iter().fold(h_lvl, |a, b| min_js(a, *b));
    let exp_lo = if nvs_med.len() >= 2 { 0.7 } else { 0.8 };
    let lo = max_js(
        1.0,
        min_js(js::round(min_h * exp_lo), js::round(h_lvl * 0.3)),
    );
    let hi = min_js(
        tr,
        max_js(max_js(at.lvl + 15.0, js::round(at.lvl * 1.3)), h_lvl),
    );
    let quer = |s: &EspecieCombate| {
        let out = eh_outland(Some(s.id));
        let pokebola = s.categoria.as_deref() == Some("pokebola");
        s.nivel_captura.is_some_and(|h| h >= lo && h <= hi)
            && match modo {
                "out" => out,
                "shiny" => pokebola && !out,
                _ => pokebola || out,
            }
    };
    let mut lista: Vec<&EspecieCombate> = ctx.dex.especies().iter().filter(|s| quer(s)).collect();
    lista.sort_by(|a, b| {
        let (ha, hb) = (
            a.nivel_captura.unwrap_or(0.0),
            b.nivel_captura.unwrap_or(0.0),
        );
        hb.total_cmp(&ha)
    });
    if lista.len() > 220 {
        let passo = lista.len() as f64 / 220.0;
        lista = (0..220)
            .filter_map(|i| lista.get((i as f64 * passo).floor() as usize).copied())
            .collect();
    }
    let alts = lista
        .into_iter()
        .map(|s| {
            let h = s.nivel_captura.unwrap_or(0.0);
            Alt {
                sp: s,
                h,
                r: f(s, h, 150.0),
            }
        })
        .collect();
    Some(Base {
        at,
        h_nome,
        h_lvl,
        cur,
        alts,
    })
}

// ---------------------------------------------------------------- planoHunt

/// Candidata a troca (`{x, p, val}` do legado).
#[derive(Clone)]
struct Cand {
    nome: String,
    nivel: f64,
    pior: f64,
    p: Previsao,
    val: Option<f64>,
}

impl Cand {
    /// `b.val - a.val` do comparador (nulo vira 0).
    fn dif(a: &Cand, b: &Cand) -> f64 {
        b.val.unwrap_or(0.0) - a.val.unwrap_or(0.0)
    }
}

struct TesteInfo {
    passou: f64,
    dur: f64,
    antes: Option<Value>,
    depois: Option<f64>,
    lucro_depois: Option<f64>,
    obj: String,
    un: String,
}

struct Plano {
    o: Map<String, Value>,
    /// Chave do objetivo (`obj`).
    obj: &'static str,
    base: Option<PlanoBase>,
    best: Option<Cand>,
    w: Janela,
    medida_ok: bool,
    atual: Option<f64>,
    enc: f64,
    explorar: Option<Cand>,
    lucro_h: Option<f64>,
    espera: f64,
    alvo: Option<f64>,
    teste: Option<TesteInfo>,
}

/// O que o `motorHunt` usa de `base`.
struct PlanoBase {
    h_nome: String,
    h_lvl: f64,
    at_lvl: f64,
    at_sp: String,
    tr_lv: f64,
}

fn mapa_obj(pares: Vec<(&str, Value)>) -> Value {
    let mut m = Map::new();
    for (k, v) in pares {
        m.insert(k.to_owned(), v);
    }
    Value::Object(m)
}

/// `planoHunt(conta).o`.
pub fn plano_hunt(d: &Value, r: &Relogio, conta: &ContaViva, ctx: &ContextoHunt) -> Value {
    Value::Object(plano_completo(d, r, conta, ctx).o)
}

fn acao_hunt<'a>(d: &'a Value, nick: &str) -> Option<&'a Value> {
    d.get("acoes")?.as_array()?.iter().find(|a| {
        a.get("nick").and_then(Value::as_str) == Some(nick)
            && a.get("tipo").and_then(Value::as_str) == Some("hunt")
            && ver(a, "ok")
    })
}

fn plano_completo(d: &Value, r: &Relogio, conta: &ContaViva, ctx: &ContextoHunt) -> Plano {
    let ag = r.agora_ms as f64;
    let nick = conta.nick.as_str();
    let cfg = config::efetiva(d, nick);
    let null = Value::Null;
    let c = cfg.get("hunt").unwrap_or(&null);
    let h = hunt_atual(conta);
    let obj_chave = c
        .get("objetivo")
        .and_then(Value::as_str)
        .and_then(objetivo)
        .map_or("xp", |o| o.0);
    let (_, obj_nome, obj_un) = objetivo(obj_chave).unwrap_or(("xp", "XP", "XP/h"));
    let pr = ctx.precos;
    let ch_shiny = {
        let v = js::mais(conta.welcome.get("chanceShiny"));
        if verd(v) {
            v
        } else {
            1.0 / 24000.0
        }
    };
    let gemas = Gemas {
        precos: cfg.get("gemaPrecos").cloned().unwrap_or(Value::Null),
        ch_shiny,
    };
    let ult = acao_hunt(d, nick);
    let huntdesde = ctx.hunt_desde;
    let ult_t = ult.map(|u| nm(u, "t"));
    let inicio_hunt = max_js(huntdesde, ult_t.map_or(0.0, |t| t + 30e3))
        + if verd(huntdesde) { 15e3 } else { 0.0 };
    let desde_troca = max_js(0.0, ag - inicio_hunt);
    let mut w_ms = min_js(20.0 * 60e3, desde_troca);
    let mut w = janela(conta, w_ms, ag, ag);
    if w.kills < 20 {
        let ms = min_js(60.0 * 60e3, desde_troca);
        if ms > 20.0 * 60e3 {
            let w2 = janela(conta, ms, ag, ag);
            if w2.kills > w.kills {
                w = w2;
                w_ms = ms;
            }
        }
    }
    let primeira = conta.linhas.first().map_or(ag, |l| l.1 as f64);
    let dados_min = min_js(min_js(w_ms, desde_troca), ag - primeira) / 60e3;
    let medida_ok = w.kills >= 20 || (w.kills >= 5 && dados_min >= 15.0);
    let lucro_atual = lucro_janela(&w, &pr);
    let sp_atual = h.as_ref().and_then(|(n, _)| ctx.dex.por_nome(n));
    let tr_lv = ou(num(conta.ultimo.get("trLv")), 0.0);
    let atual = if obj_chave == "gemas" {
        let sp = match sp_atual {
            Some(s) => EspValor {
                d: Some(s.id),
                tr_lv,
            },
            None => EspValor {
                d: None,
                tr_lv: 0.0,
            },
        };
        Some(valor_gemas_h(w.kph, &sp, &gemas))
    } else {
        valor_janela(&w, obj_chave, &pr, Some(ch_shiny))
    };
    let intervalo = js::mais(c.get("intervaloMin"));
    let espera0 = ult_t.map_or(0.0, |t| max_js(0.0, intervalo * 60e3 - (ag - t)));
    let mut o = Map::new();
    o.insert("ativo".into(), json!(ver(c, "ativo")));
    o.insert("objetivo".into(), json!(obj_chave));
    o.insert("objNome".into(), json!(obj_nome));
    o.insert("un".into(), json!(obj_un));
    for k in ["margem", "intervaloMin", "maxPiorPct"] {
        if let Some(v) = c.get(k) {
            o.insert(k.into(), v.clone());
        }
    }
    o.insert(
        "hunt".into(),
        h.as_ref().map_or(Value::Null, |(n, l)| {
            mapa_obj(vec![("nome", json!(n)), ("lvl", jo(*l))])
        }),
    );
    o.insert("kills".into(), json!(w.kills));
    o.insert("killsMin".into(), json!(20));
    o.insert("janelaMin".into(), jn(js::round(w_ms / 60e3)));
    o.insert("medidaOk".into(), json!(medida_ok));
    o.insert("xpH".into(), jo(w.xp_h));
    o.insert("lucroH".into(), jo(lucro_atual));
    o.insert("atual".into(), jo(atual));
    o.insert("kph".into(), jo(w.kph));
    o.insert(
        "ultimaTroca".into(),
        match ult {
            Some(u) => {
                let mut m = Map::new();
                for k in ["t", "de", "para"] {
                    if let Some(v) = u.get(k) {
                        m.insert(k.into(), v.clone());
                    }
                }
                m.insert("auto".into(), json!(ver(u, "auto")));
                Value::Object(m)
            }
            None => Value::Null,
        },
    );
    o.insert("espera".into(), jn(espera0));
    let prefixo = format!("{nick}|");
    let bloqueadas: Vec<Value> = d
        .get("bloqueio")
        .and_then(Value::as_object)
        .map(|b| {
            b.iter()
                .filter(|(k, v)| k.starts_with(&prefixo) && js::mais(Some(v)) > ag)
                .map(|(k, v)| {
                    mapa_obj(vec![
                        ("nome", json!(k.get(nick.len() + 1..).unwrap_or(""))),
                        ("ate", (*v).clone()),
                    ])
                })
                .collect()
        })
        .unwrap_or_default();
    o.insert("bloqueadas".into(), Value::Array(bloqueadas));
    let mut teste: Option<TesteInfo> = None;
    if let Some(u) = ult.filter(|u| ver(u, "teste")) {
        let t = nm(u, "t");
        let fim = t + TESTE_MIN * 60e3 + 30e3;
        let ate = min_js(ag, fim);
        let tobj_txt = match u.get("obj") {
            Some(v) if js::verdadeiro(Some(v)) => js::texto(Some(v)),
            _ => "xp".into(),
        };
        // `OBJETIVOS[tobj].un` quebra no legado com objetivo desconhecido; aqui cai em XP/h.
        let tun = objetivo(&tobj_txt).map_or("XP/h", |x| x.2);
        let depois = janela(conta, max_js(0.0, ate - (t + 30e3)), ate, ag);
        let antes = js::nulo_ou(u.get("antesVal"), u.get("antesXpH")).cloned();
        let info = TesteInfo {
            passou: ag - t,
            dur: TESTE_MIN * 60e3 + 30e3,
            antes,
            depois: valor_janela(&depois, &tobj_txt, &pr, Some(ch_shiny)),
            lucro_depois: lucro_janela(&depois, &pr),
            obj: tobj_txt.clone(),
            un: tun.to_owned(),
        };
        let mut m = Map::new();
        m.insert("t".into(), u.get("t").cloned().unwrap_or(Value::Null));
        for k in ["de", "para"] {
            if let Some(v) = u.get(k) {
                m.insert(k.into(), v.clone());
            }
        }
        m.insert("passou".into(), jn(info.passou));
        m.insert("dur".into(), jn(info.dur));
        m.insert("un".into(), json!(info.un));
        if let Some(a) = &info.antes {
            m.insert("antes".into(), a.clone());
        }
        m.insert("depois".into(), jo(info.depois));
        m.insert("killsDepois".into(), json!(depois.kills));
        m.insert("lucroDepois".into(), jo(info.lucro_depois));
        m.insert("obj".into(), json!(tobj_txt));
        o.insert("teste".into(), Value::Object(m));
        teste = Some(info);
    }
    let vazio = |o: Map<String, Value>, motivo: &str, base: Option<PlanoBase>| {
        let mut o = o;
        o.insert("motivo".into(), json!(motivo));
        Plano {
            o,
            obj: obj_chave_estatica(obj_chave),
            base,
            best: None,
            w: w.clone(),
            medida_ok,
            atual,
            enc: CALIB_PADRAO,
            explorar: None,
            lucro_h: lucro_atual,
            espera: espera0,
            alvo: None,
            teste: None,
        }
    };
    let base = melhores_hunts(d, conta, ctx, c);
    let Some(base) = base.filter(|b| b.cur.is_some()) else {
        let motivo = if h.is_none() {
            "Fora de hunt."
        } else {
            "Ainda não reconheci o pokemon ativo ou a hunt."
        };
        return vazio(o, motivo, None);
    };
    let Some(cur) = base.cur else {
        return vazio(o, "", None);
    };
    o.insert("piorAtual".into(), jn(cur.pior));
    // fatorReal(conta, cur) || 1
    let w20 = janela(conta, 20.0 * 60e3, ag, ag);
    let f = ou(
        w20.kph
            .filter(|k| verd(*k) && w20.kills >= 15)
            .map(|k| k / cur.kph),
        1.0,
    );
    let obs: Vec<(f64, f64)> = d
        .get("medidas")
        .and_then(|m| m.get(nick))
        .and_then(Value::as_object)
        .map(|m| m.values().map(|x| (nm(x, "h"), nm(x, "xpKill"))).collect())
        .unwrap_or_default();
    let modelo = modelo_xp(&obs);
    let enc = fator_calibracao(d, nick, &base.at.sp.nome, base.at.lvl);
    let real = Real {
        kph: w.kph,
        xp_kill: w.xp_kill,
        gold_kill: w.gold_kill,
        pot_por_kill: w.pot_kill,
        bolas_por_kill: w.bolas_kill,
        hunt_lvl: base.h_lvl,
    };
    let limite = ctx.limite_rodizio;
    let luta_lv = limite.unwrap_or(base.at.lvl);
    let tr_lv_h = ou(num(conta.ultimo.get("trLv")), luta_lv);
    let piso_lv = (min_js(luta_lv, tr_lv_h) * 0.5).floor();
    let segura_para = |h: f64| h >= piso_lv && limite.is_none_or(|l| h <= l + 15.0);
    let max_pior = js::mais(c.get("maxPiorPct"));
    let bloqueada = |nome: &str| {
        d.get("bloqueio")
            .and_then(|b| b.get(format!("{nick}|{nome}")))
            .is_some_and(|v| js::mais(Some(v)) > ag)
    };
    let medidas = d.get("medidas").and_then(|m| m.get(nick));
    let mesmo_lutador = |m: &Value| {
        let lvl = nm(m, "lvl");
        m.get("sp").and_then(Value::as_str) == Some(base.at.sp.nome.as_str())
            && verd(lvl)
            && (lvl - base.at.lvl).abs() <= base.at.lvl * 0.1
    };
    let med_recente = |nome: &str, lvl: f64| -> Option<f64> {
        let m = medidas?.get(format!("{nome}|{}", nv(lvl)))?;
        let xp_h = nm(m, "xpH");
        (xp_h > 0.0 && ag - nm(m, "t") < 12.0 * HORA && mesmo_lutador(m)).then_some(xp_h)
    };
    let usa_med = obj_chave == "xp" || obj_chave == "equilibrio";
    let preve = |x: &Alt| {
        let p = previsao(&cur, &x.r, x.h, &real, &pr, f, modelo.as_ref());
        let med = if usa_med {
            med_recente(&x.sp.nome, x.h)
        } else {
            None
        };
        let sp = EspValor {
            d: Some(x.sp.id),
            tr_lv: ou(num(conta.ultimo.get("trLv")), 0.0),
        };
        let val = match med {
            Some(m) => Some(m),
            None => encolher(valor_hunt(obj_chave, &p, &sp, &gemas), enc),
        };
        Cand {
            nome: x.sp.nome.clone(),
            nivel: x.h,
            pior: x.r.pior,
            p,
            val,
        }
    };
    let seguras: Vec<&Alt> = base
        .alts
        .iter()
        .filter(|x| {
            x.r.pior < max_pior
                && x.r.bate >= 2.5
                && x.sp.nome != base.h_nome
                && !bloqueada(&x.sp.nome)
                && segura_para(x.h)
        })
        .collect();
    let kph_w = w.kph.filter(|k| verd(*k));
    let mut cands: Vec<Cand> = if kph_w.is_some() {
        seguras
            .iter()
            .map(|x| preve(x))
            .filter(|c| {
                c.val.is_some_and(f64::is_finite)
                    && (obj_chave != "equilibrio" || c.p.lucro_h >= 0.0)
            })
            .collect()
    } else {
        Vec::new()
    };
    ordenar_como_js(&mut cands, Cand::dif);
    let best = cands.first().cloned();
    let ref_lv = min_js(luta_lv, tr_lv_h);
    let abaixo = base.h_lvl < ref_lv * 0.75;
    let l0 = if abaixo { ref_lv } else { base.h_lvl };
    let bandas: &[(f64, f64)] = if abaixo {
        &[
            (1.0001, 1.3),
            (0.9, 1.0001),
            (0.75, 0.9),
            (0.6, 0.75),
            (0.5, 0.6),
        ]
    } else {
        &[
            (1.0001, 1.3),
            (0.9, 1.0001),
            (0.75, 0.9),
            (0.6, 0.75),
            (0.45, 0.6),
            (0.3, 0.45),
        ]
    };
    o.insert(
        "huntAbaixo".into(),
        if abaixo {
            mapa_obj(vec![
                ("hunt", jn(base.h_lvl)),
                ("ref", jn(ref_lv)),
                ("piso", jn(piso_lv)),
            ])
        } else {
            Value::Null
        },
    );
    let seguras_exp: Vec<&Alt> = base
        .alts
        .iter()
        .filter(|x| {
            x.r.pior < max_pior
                && x.sp.nome != base.h_nome
                && !bloqueada(&x.sp.nome)
                && segura_para(x.h)
        })
        .collect();
    let chave_atual = format!("{}|{}", base.h_nome, nv(base.h_lvl));
    let medidas_outras: Vec<f64> = medidas
        .and_then(Value::as_object)
        .map(|m| {
            m.iter()
                .filter(|(k, v)| {
                    **k != chave_atual
                        && nm(v, "xpH") > 0.0
                        && ag - nm(v, "t") < 12.0 * HORA
                        && mesmo_lutador(v)
                })
                .map(|(_, v)| nm(v, "h"))
                .collect()
        })
        .unwrap_or_default();
    let mut explorar: Option<Cand> = None;
    let mut degraus = Vec::new();
    for &(a, b) in bandas {
        let acima = a > 1.0;
        let lo = if acima { l0 + 1.0 } else { js::round(l0 * a) };
        let hi = if acima {
            min_js(
                tr_lv_h,
                max_js(js::round(l0 * b), js::round(ref_lv * 1.3) + 1.0),
            )
        } else {
            js::round(l0 * b)
        };
        let medido = medidas_outras.iter().any(|h| *h >= lo && *h < hi);
        let mut do_degrau: Vec<Cand> = if kph_w.is_some() {
            seguras_exp
                .iter()
                .filter(|x| x.h >= lo && x.h < hi)
                .map(|x| preve(x))
                .collect()
        } else {
            Vec::new()
        };
        ordenar_como_js(&mut do_degrau, Cand::dif);
        if !medido && !do_degrau.is_empty() && explorar.is_none() {
            explorar = do_degrau.first().cloned();
        }
        degraus.push(mapa_obj(vec![
            ("de", jn(lo)),
            ("ate", jn(if acima { hi } else { min_js(hi, l0) })),
            ("acima", json!(acima)),
            ("medido", json!(medido)),
            ("candidatos", json!(do_degrau.len())),
        ]));
    }
    o.insert("degraus".into(), Value::Array(degraus));
    o.insert(
        "explorar".into(),
        explorar.as_ref().map_or(Value::Null, |e| {
            mapa_obj(vec![("nome", json!(e.nome)), ("nivel", jn(e.nivel))])
        }),
    );
    o.insert("seguras".into(), json!(seguras.len()));
    o.insert("analisadas".into(), json!(base.alts.len()));
    let resumo = |c: &Cand| {
        mapa_obj(vec![
            ("nome", json!(c.nome)),
            ("nivel", jn(c.nivel)),
            ("val", jo(c.val)),
            ("xpH", jo(encolher(c.p.xp_h, enc))),
            ("lucroH", jo(encolher(Some(c.p.lucro_h), enc))),
            ("pior", jn(c.pior)),
        ])
    };
    o.insert(
        "calibracao".into(),
        mapa_obj(vec![
            ("fator", jn(js::round(enc * 100.0) / 100.0)),
            (
                "trocas",
                json!(calib_do_lutador(d, nick, &base.at.sp.nome, base.at.lvl).len()),
            ),
            ("lutador", json!(base.at.sp.nome)),
        ]),
    );
    o.insert(
        "candidatos".into(),
        Value::Array(cands.iter().take(3).map(resumo).collect()),
    );
    o.insert("melhor".into(), best.as_ref().map_or(Value::Null, resumo));
    let margem = js::mais(c.get("margem"));
    let alvo = atual.map(|a| a + a.abs() * margem);
    o.insert("alvo".into(), jo(alvo));
    let progresso = match (&best, alvo) {
        (Some(b), Some(al)) => {
            let v = b.val.unwrap_or(0.0);
            Some(if al > 0.0 {
                v / al
            } else if v >= al {
                1.0
            } else {
                0.0
            })
        }
        _ => None,
    };
    o.insert("progresso".into(), jo(progresso));
    let salto = best
        .as_ref()
        .is_some_and(|b| atual.is_some_and(|a| a > 0.0 && b.val.unwrap_or(0.0) >= a * 3.0));
    o.insert("saltoGrande".into(), json!(salto));
    let mut espera = espera0;
    let mut espera_pulada: Option<f64> = None;
    if salto && espera > 0.0 {
        espera_pulada = Some(espera);
        espera = 0.0;
        o.insert("esperaPulada".into(), jn(espera_pulada.unwrap_or(0.0)));
        o.insert("espera".into(), jn(0.0));
    }
    let explorar_ligado = c.get("explorar") != Some(&Value::Bool(false));
    let prox_degrau = |e: &Cand| format!("{} Nv {}", e.nome, nv(e.nivel));
    let ativo = ver(c, "ativo");
    let motivo = if !ativo {
        "Desligada: só mostra a análise.".to_owned()
    } else if let Some(t) = o.get("teste").and_then(|t| t.get("para")) {
        format!(
            "Testando {}: compara os primeiros {} min com os {} min de antes.",
            js::texto(Some(t)),
            nv(TESTE_MIN),
            nv(TESTE_MIN)
        )
    } else if !medida_ok {
        format!(
            "Medindo a hunt atual: {} de 20 abates (ou 5 abates em 15 min, se a hunt for lenta).",
            w.kills
        )
    } else if atual.is_none() {
        "Sem medida da hunt atual ainda.".to_owned()
    } else if obj_chave == "equilibrio" && lucro_atual.is_some_and(|l| l < 0.0) && best.is_none() {
        "A hunt atual dá prejuízo e nenhuma segura prevê lucro.".to_owned()
    } else if espera > 0.0 {
        format!(
            "Espera {} min desde a última troca.",
            nv((espera / 60e3).ceil())
        )
    } else if let (Some(p), Some(b)) = (espera_pulada, best.as_ref()) {
        format!(
            "{} Nv {} prevê {} {obj_un} contra {} de agora: salto grande demais para esperar os {} min restantes. Vai trocar.",
            b.nome,
            nv(b.nivel),
            fmt_br(b.val.unwrap_or(0.0)),
            fmt_br(atual.unwrap_or(0.0)),
            nv((p / 60e3).ceil())
        )
    } else if let Some(b) = best.as_ref() {
        let al = alvo.unwrap_or(f64::NAN);
        if b.val.unwrap_or(0.0) < al {
            let extra = match (&explorar, explorar_ligado) {
                (Some(e), true) => format!(" Próximo degrau a medir: {}.", prox_degrau(e)),
                _ => String::new(),
            };
            format!(
                "{} Nv {} prevê {} {obj_un}; falta chegar em {}.{extra}",
                b.nome,
                nv(b.nivel),
                fmt_br(b.val.unwrap_or(0.0)),
                fmt_br(al)
            )
        } else {
            format!("Vai trocar para {} Nv {}.", b.nome, nv(b.nivel))
        }
    } else {
        match (&explorar, explorar_ligado) {
            (Some(e), true) => format!(
                "Nenhuma previsão passa o alvo; próximo degrau a medir: {}.",
                prox_degrau(e)
            ),
            _ => format!("Nenhuma hunt segura rende mais {obj_nome} pela previsão."),
        }
    };
    o.insert("motivo".into(), json!(motivo));
    let pb = PlanoBase {
        h_nome: base.h_nome.clone(),
        h_lvl: base.h_lvl,
        at_lvl: base.at.lvl,
        at_sp: base.at.sp.nome.clone(),
        tr_lv: ou(num(conta.ultimo.get("trLv")), base.at.lvl),
    };
    Plano {
        o,
        obj: obj_chave_estatica(obj_chave),
        base: Some(pb),
        best,
        w,
        medida_ok,
        atual,
        enc,
        explorar,
        lucro_h: lucro_atual,
        espera,
        alvo,
        teste,
    }
}

fn obj_chave_estatica(k: &str) -> &'static str {
    objetivo(k).map_or("xp", |o| o.0)
}

// ---------------------------------------------------------------- motorHunt

struct Acumulo<'a> {
    nick: &'a str,
    agora: f64,
    saida: SaidaHunt,
    /// A ação `hunt` do `D` com as gravações já aplicadas (o legado muta o objeto).
    ult: Option<Value>,
}

impl Acumulo<'_> {
    fn registrar(&mut self, texto: String, ok: bool) {
        self.saida.decisoes.push(Decisao::Registro {
            nick: self.nick.to_owned(),
            tipo: "hunt".into(),
            texto,
            ok,
        });
    }
    fn gravar(&mut self, alvo: &str, dados: Value) {
        self.saida.gravacoes.push(GravacaoD {
            nick: self.nick.to_owned(),
            alvo: alvo.into(),
            dados,
        });
    }
    /// `ult.campo = valor`: aplica na cópia local e grava.
    fn mexer_acao(&mut self, campos: Vec<(&str, Value)>) {
        let mut m = Map::new();
        for (k, v) in campos {
            m.insert(k.to_owned(), v);
        }
        if let Some(Value::Object(u)) = &mut self.ult {
            for (k, v) in &m {
                u.insert(k.clone(), v.clone());
            }
        }
        self.gravar("acao", Value::Object(m));
    }
    fn bloquear(&mut self, nome: &str, horas: f64) {
        let ate = self.agora + horas * HORA;
        self.gravar(
            "bloqueio",
            json!({ "chave": format!("{}|{nome}", self.nick), "ate": jn(ate) }),
        );
    }
    fn calibrar(&mut self, previsto: f64, real: Option<f64>, sp: &str, lvl: f64) {
        if let Some(g) = registrar_calibracao(self.nick, previsto, real, sp, lvl, self.agora) {
            self.saida.gravacoes.push(g);
        }
    }
}

/// `{...(base || {}), ...novos}` com a ordem de inserção do JS.
fn mesclar(base: Option<&Value>, novos: Vec<(&str, Value)>) -> Value {
    let mut m = match base {
        Some(Value::Object(b)) => b.clone(),
        _ => Map::new(),
    };
    for (k, v) in novos {
        m.insert(k.to_owned(), v);
    }
    Value::Object(m)
}

/// `{antes, depois, un}` (e `minutos`) do `res`; `antes` ausente não entra.
fn resultado_de(
    antes: Option<&Value>,
    depois: Option<f64>,
    un: &str,
    minutos: Option<f64>,
) -> Value {
    let mut m = Map::new();
    if let Some(a) = antes {
        m.insert("antes".into(), a.clone());
    }
    m.insert("depois".into(), jo(depois));
    m.insert("un".into(), json!(un));
    if let Some(x) = minutos {
        m.insert("minutos".into(), jn(x));
    }
    Value::Object(m)
}

/// `motorHunt`: só as decisões.
pub fn motor_hunt(
    d: &Value,
    laco: &mut EstadoLaco,
    r: &Relogio,
    conta: &ContaViva,
    ctx: &ContextoHunt,
) -> Vec<Decisao> {
    motor_hunt_completo(d, laco, r, conta, ctx).decisoes
}

/// `motorHunt` com as gravações no `D` que o legado faria (veja [`GravacaoD`]). A resposta do
/// jogo ao `trocarHunt` é sempre assumida `"ok"`.
pub fn motor_hunt_completo(
    d: &Value,
    laco: &mut EstadoLaco,
    r: &Relogio,
    conta: &ContaViva,
    ctx: &ContextoHunt,
) -> SaidaHunt {
    let nick = conta.nick.as_str();
    let ag = r.agora_ms as f64;
    let mut ac = Acumulo {
        nick,
        agora: ag,
        saida: SaidaHunt::default(),
        ult: acao_hunt(d, nick).cloned(),
    };
    if !js::verdadeiro(Some(&conta.ultimo)) {
        return ac.saida;
    }
    let cfg = config::efetiva(d, nick);
    let null = Value::Null;
    let c = cfg.get("hunt").unwrap_or(&null);
    let p = plano_completo(d, r, conta, ctx);
    let Some(pb) = p.base.as_ref() else {
        return ac.saida;
    };
    let w = &p.w;
    let quem_sp = pb.at_sp.as_str();
    // medida desta hunt: só com os abates depois da troca
    {
        let inicio_med = match &ac.ult {
            Some(u) if u.get("para").and_then(Value::as_str) == Some(pb.h_nome.as_str()) => {
                max_js(nm(u, "t") + 30e3, ctx.hunt_desde)
            }
            _ => f64::INFINITY,
        };
        let desde = ag - inicio_med;
        let wm = if (0.0..20.0 * 60e3).contains(&desde) {
            janela(conta, max_js(0.0, desde), ag, ag)
        } else {
            w.clone()
        };
        if wm.kills >= 20 && wm.xp_kill.is_some_and(verd) {
            let chave = format!("{}|{}", pb.h_nome, nv(pb.h_lvl));
            ac.gravar(
                "medida",
                json!({ "chave": chave, "medida": {
                    "h": jn(pb.h_lvl), "xpKill": jo(wm.xp_kill), "kph": jo(wm.kph), "xpH": jo(wm.xp_h),
                    "t": jn(ag), "sp": quem_sp, "lvl": jn(pb.at_lvl) } }),
            );
            ac.gravar(
                "medHist",
                json!({ "chave": chave, "ponto": [jn(ag), jn(js::round(ou(wm.xp_h, 0.0)))] }),
            );
        }
    }
    let piso_v = (min_js(pb.at_lvl, pb.tr_lv) * 0.5).floor();
    // `voltar`: assume resposta "ok" do jogo.
    let voltar = |ac: &mut Acumulo,
                  motivo: String,
                  horas: f64,
                  resultado: Option<Value>,
                  seguranca: bool| {
        let Some(u) = ac.ult.clone() else {
            return;
        };
        let de = txt(&u, "de");
        if !seguranca
            && u.get("nivelDe").is_some_and(|v| !v.is_null())
            && js::mais(u.get("nivelDe")) < piso_v
        {
            let res = mesclar(
                resultado.as_ref(),
                vec![
                    ("ficou", json!(true)),
                    ("motivo", json!("a anterior está abaixo do piso de nível")),
                    ("t", jn(ag)),
                ],
            );
            ac.mexer_acao(vec![
                ("vigiar", json!(false)),
                ("teste", json!(false)),
                ("resultado", res),
            ]);
            ac.registrar(
                format!(
                    "{} {motivo}, mas não volto para {de} Nv {}: fica abaixo da metade do nível do {} (Nv {}). Fico em {}.",
                    pb.h_nome,
                    txt(&u, "nivelDe"),
                    pb.at_sp,
                    nv(pb.at_lvl),
                    pb.h_nome
                ),
                true,
            );
            return;
        }
        ac.bloquear(&pb.h_nome, horas);
        ac.mexer_acao(vec![("vigiar", json!(false)), ("teste", json!(false))]);
        let tem_slug = ver(&u, "slugDe");
        let fim = format!("e bloqueei {} por {} h.", pb.h_nome, nv(horas));
        let base_txt = format!("{} {motivo}. Voltei para {de}", pb.h_nome);
        if tem_slug {
            let res = mesclar(
                resultado.as_ref(),
                vec![
                    ("ficou", json!(false)),
                    ("motivo", json!(motivo)),
                    ("voltou", json!(true)),
                    ("t", jn(ag)),
                ],
            );
            ac.mexer_acao(vec![("resultado", res)]);
            ac.saida.decisoes.push(Decisao::TrocarHunt {
                nick: nick.to_owned(),
                slug: txt(&u, "slugDe"),
                nome: de.clone(),
                nivel: u.get("nivelDe").cloned().unwrap_or(Value::Null),
                registro: Some(Registro {
                    tipo: "hunt".into(),
                    texto_ok: format!("{base_txt} {fim}"),
                    texto_falha: format!("{base_txt} — falhou: {{r}} {fim}"),
                }),
            });
        } else {
            let res = mesclar(
                resultado.as_ref(),
                vec![
                    ("ficou", json!(false)),
                    ("motivo", json!(motivo)),
                    ("voltou", json!(false)),
                    ("t", jn(ag)),
                ],
            );
            ac.mexer_acao(vec![("resultado", res)]);
            ac.registrar(format!("{base_txt} — falhou: sem-slug {fim}"), false);
        }
    };
    // vigia a última troca: desmaiou ou gastou o dobro de poção do previsto
    if let Some(u) = ac.ult.clone() {
        if ver(&u, "vigiar")
            && ag - nm(&u, "t") < 15.0 * 60e3
            && u.get("para").and_then(Value::as_str) == Some(pb.h_nome.as_str())
        {
            let w5 = janela(conta, ag - nm(&u, "t"), ag, ag);
            let pot_prev = u.get("potPrev").filter(|v| !v.is_null());
            let poção_demais = w5.kills >= 10
                && pot_prev.is_some_and(|pp| {
                    w5.pot_kill
                        .is_some_and(|pk| pk > max_js(0.2, js::mais(Some(pp)) * 2.0))
                });
            if w5.desm > 0 || poção_demais {
                let motivo = format!(
                    "saiu pior que o previsto ({})",
                    if w5.desm > 0 {
                        "desmaio"
                    } else {
                        "poção demais"
                    }
                );
                voltar(&mut ac, motivo, 24.0, None, true);
                return ac.saida;
            }
        }
    }
    // teste da troca automática
    if let Some(t) = &p.teste {
        let u = ac.ult.clone().unwrap_or(Value::Null);
        if u.get("para").and_then(Value::as_str) != Some(pb.h_nome.as_str()) {
            ac.mexer_acao(vec![
                ("teste", json!(false)),
                (
                    "resultado",
                    json!({ "ficou": null, "motivo": "você trocou de hunt no meio do teste", "t": jn(ag) }),
                ),
            ]);
            return ac.saida;
        }
        if t.passou >= t.dur {
            let res = resultado_de(t.antes.as_ref(), t.depois, &t.un, None);
            let para = txt(&u, "para");
            let de = txt(&u, "de");
            if ver(&u, "porPrejuizo") {
                ac.mexer_acao(vec![("teste", json!(false))]);
                if t.lucro_depois.is_some_and(|l| l >= 0.0) {
                    let ld = t.lucro_depois.unwrap_or(0.0);
                    let acomp = json!({ "inicio": jn(nm(&u, "t") + 30e3), "antes": null, "teste": jo(t.depois),
                        "prev": null, "obj": "equilibrio", "un": t.un, "feitos": [] });
                    ac.mexer_acao(vec![("acomp", acomp)]);
                    let r2 = mesclar(Some(&res), vec![("ficou", json!(true)), ("t", jn(ag))]);
                    ac.mexer_acao(vec![("resultado", r2)]);
                    ac.registrar(
                        format!(
                            "Teste de {} min: {para} dá lucro de {} de ouro/h ({} {}). Fica; confiro com 15 e 30 min.",
                            nv(TESTE_MIN),
                            fmt0(ld),
                            fmt0(t.depois.unwrap_or(0.0)),
                            t.un
                        ),
                        true,
                    );
                } else {
                    ac.bloquear(&para, 3.0);
                    let r2 = mesclar(
                        Some(&res),
                        vec![
                            ("ficou", json!(true)),
                            ("motivo", json!("também deu prejuízo")),
                            ("t", jn(ag)),
                        ],
                    );
                    ac.mexer_acao(vec![("resultado", r2)]);
                    ac.registrar(
                        format!(
                            "{para} também deu prejuízo ({} de ouro/h). Bloqueei por 3 h; procuro outra assim que passar o intervalo entre trocas.",
                            fmt0(t.lucro_depois.unwrap_or(0.0))
                        ),
                        false,
                    );
                }
                return ac.saida;
            }
            let prev = {
                let pb2 = nm(&u, "prevBruto");
                if verd(pb2) {
                    pb2
                } else {
                    nm(&u, "prev")
                }
            };
            if t.obj == "equilibrio" && t.lucro_depois.is_some_and(|l| l < 0.0) {
                if !ver(&u, "explorar") {
                    ac.calibrar(prev, t.depois, quem_sp, pb.at_lvl);
                }
                let motivo = format!(
                    "deu prejuízo de {} de ouro/h nos primeiros {} min (poção/bola mais caras que o loot)",
                    fmt0(-t.lucro_depois.unwrap_or(0.0)),
                    nv(TESTE_MIN)
                );
                voltar(&mut ac, motivo, 6.0, Some(res), false);
                return ac.saida;
            }
            let antes_f = t
                .antes
                .as_ref()
                .filter(|a| !a.is_null())
                .map(|a| js::mais(Some(a)));
            let aprovado = match antes_f {
                None => true,
                Some(a) => t.depois.unwrap_or(f64::NEG_INFINITY) >= a,
            };
            if aprovado {
                let r2 = mesclar(Some(&res), vec![("ficou", json!(true)), ("t", jn(ag))]);
                let prev_v = if verd(prev) { jn(prev) } else { Value::Null };
                let acomp = json!({ "inicio": jn(nm(&u, "t") + 30e3), "antes": t.antes.clone().unwrap_or(Value::Null),
                    "teste": jo(t.depois), "prev": prev_v, "obj": t.obj, "un": t.un, "feitos": [] });
                ac.mexer_acao(vec![
                    ("teste", json!(false)),
                    ("resultado", r2),
                    ("acomp", acomp),
                ]);
                ac.registrar(
                    format!(
                        "Teste de {} min: {para} rendeu {} {} contra {} em {de}. Fica por enquanto; confiro de novo com 15 e 30 min.",
                        nv(TESTE_MIN),
                        fmt0(t.depois.unwrap_or(0.0)),
                        t.un,
                        fmt0(antes_f.unwrap_or(0.0))
                    ),
                    true,
                );
            } else {
                if !ver(&u, "explorar") {
                    ac.calibrar(prev, t.depois, quem_sp, pb.at_lvl);
                }
                let motivo = format!(
                    "rendeu {} {} nos primeiros {} min, menos que os {} de {de}",
                    fmt0(t.depois.unwrap_or(0.0)),
                    t.un,
                    nv(TESTE_MIN),
                    fmt0(antes_f.unwrap_or(0.0))
                );
                voltar(&mut ac, motivo, 6.0, Some(res), false);
            }
            return ac.saida;
        }
    }
    // acompanhamento depois do teste (15 e 30 min)
    let ch_shiny = {
        let v = js::mais(conta.welcome.get("chanceShiny"));
        if verd(v) {
            v
        } else {
            1.0 / 24000.0
        }
    };
    let a_atual = ac
        .ult
        .as_ref()
        .and_then(|u| u.get("acomp"))
        .filter(|a| js::verdadeiro(Some(a)))
        .cloned();
    if let Some(a) = a_atual {
        let u = ac.ult.clone().unwrap_or(Value::Null);
        if u.get("para").and_then(Value::as_str) != Some(pb.h_nome.as_str()) {
            ac.mexer_acao(vec![("acomp", Value::Null)]);
        } else {
            let feitos: Vec<f64> = a
                .get("feitos")
                .and_then(Value::as_array)
                .map(|f| f.iter().map(|x| js::mais(Some(x))).collect())
                .unwrap_or_default();
            let el = ag - nm(&a, "inicio");
            let prox = [15.0, 30.0].into_iter().find(|m| !feitos.contains(m));
            if let Some(prox) = prox.filter(|p| el >= p * 60e3) {
                let wa = janela(conta, el, ag, ag);
                if wa.kills >= 10 {
                    let a_obj = match a.get("obj") {
                        Some(v) if js::verdadeiro(Some(v)) => js::texto(Some(v)),
                        _ => "xp".into(),
                    };
                    let val = valor_janela(&wa, &a_obj, &p_precos(ctx), Some(ch_shiny));
                    let luc = lucro_janela(&wa, &p_precos(ctx));
                    let mut novos = feitos.clone();
                    novos.push(prox);
                    let final_ = prox == 30.0;
                    let un = txt(&a, "un");
                    let antes = a.get("antes").cloned().unwrap_or(Value::Null);
                    let res = resultado_de(a.get("antes"), val, &un, Some(prox));
                    let a_prev = a.get("prev").filter(|v| js::verdadeiro(Some(v)));
                    let explorou = ver(&u, "explorar");
                    let aprender = |ac: &mut Acumulo| {
                        if !explorou {
                            if let Some(pv) = a_prev {
                                ac.calibrar(js::mais(Some(pv)), val, quem_sp, pb.at_lvl);
                            }
                        }
                    };
                    let de = txt(&u, "de");
                    let feitos_json: Vec<Value> = novos.iter().map(|x| jn(*x)).collect();
                    let com_feitos = {
                        let mut m = a.as_object().cloned().unwrap_or_default();
                        m.insert("feitos".into(), Value::Array(feitos_json));
                        Value::Object(m)
                    };
                    // `A.feitos.push(prox)` muta o objeto antes de qualquer retorno.
                    ac.mexer_acao(vec![("acomp", com_feitos)]);
                    let antes_num = if antes.is_null() {
                        None
                    } else {
                        Some(js::mais(Some(&antes)))
                    };
                    if antes_num.is_some_and(|an| val.is_some_and(|v| v < an * 0.97)) {
                        aprender(&mut ac);
                        ac.mexer_acao(vec![("acomp", Value::Null)]);
                        let motivo = format!(
                            "com {} min rendeu {} {un} de verdade (o teste de {} min tinha dado {}), menos que os {} de {de}",
                            nv(prox),
                            fmt0(val.unwrap_or(0.0)),
                            nv(TESTE_MIN),
                            fmt0(js::mais(a.get("teste"))),
                            fmt0(antes_num.unwrap_or(0.0))
                        );
                        voltar(&mut ac, motivo, 6.0, Some(res), false);
                        return ac.saida;
                    }
                    if a.get("obj").and_then(Value::as_str) == Some("equilibrio")
                        && luc.is_some_and(|l| l < 0.0)
                    {
                        aprender(&mut ac);
                        ac.mexer_acao(vec![("acomp", Value::Null)]);
                        let motivo = format!(
                            "com {} min está dando prejuízo de {} de ouro/h",
                            nv(prox),
                            fmt0(-luc.unwrap_or(0.0))
                        );
                        voltar(&mut ac, motivo, 6.0, Some(res), false);
                        return ac.saida;
                    }
                    let prev_txt = a_prev
                        .map(|pv| format!(" · previsto {}", fmt0(js::mais(Some(pv)))))
                        .unwrap_or_default();
                    ac.registrar(
                        format!(
                            "Conferi {} com {} min: {} {un} de verdade (teste {}{prev_txt}). Continua melhor que {de} ({}).{}",
                            txt(&u, "para"),
                            nv(prox),
                            fmt0(val.unwrap_or(0.0)),
                            fmt0(js::mais(a.get("teste"))),
                            fmt0(antes_num.unwrap_or(0.0)),
                            if final_ { " Fica de vez." } else { "" }
                        ),
                        true,
                    );
                    if final_ {
                        aprender(&mut ac);
                        let res_ant = ac.ult.as_ref().and_then(|u| u.get("resultado")).cloned();
                        let mut m = match res_ant {
                            Some(Value::Object(o)) => o,
                            _ => Map::new(),
                        };
                        if let Some(ro) = res.as_object() {
                            for (k, v) in ro {
                                m.insert(k.clone(), v.clone());
                            }
                        }
                        m.insert("ficou".into(), json!(true));
                        m.insert("conferido".into(), json!(true));
                        m.insert("t".into(), jn(ag));
                        ac.mexer_acao(vec![
                            ("acomp", Value::Null),
                            ("resultado", Value::Object(m)),
                        ]);
                    }
                }
            }
        }
    }
    // ---- decide a troca
    let ult_acomp_pendente = ac
        .ult
        .as_ref()
        .and_then(|u| u.get("acomp"))
        .filter(|a| js::verdadeiro(Some(a)))
        .is_some_and(|a| {
            !a.get("feitos")
                .and_then(Value::as_array)
                .is_some_and(|f| f.iter().any(|x| js::mais(Some(x)) == 15.0))
        });
    if !ver(c, "ativo")
        || !js::verdadeiro(Some(&conta.welcome))
        || p.teste.is_some()
        || !p.medida_ok
        || p.atual.is_none()
        || p.espera > 0.0
        || ult_acomp_pendente
        || ctx.rodizio_volta
        || ctx.equipe_no_comando
    {
        return ac.saida;
    }
    let mut alvo_troca: Option<Cand> = match (&p.best, p.alvo) {
        (Some(b), Some(al)) if b.val.unwrap_or(f64::NAN) >= al => Some(b.clone()),
        _ => None,
    };
    let mut explorando = false;
    let prejuizo = p.obj == "equilibrio" && p.medida_ok && p.lucro_h.is_some_and(|l| l < 0.0);
    if prejuizo && alvo_troca.is_none() {
        alvo_troca = p.best.clone();
    }
    if alvo_troca.is_none() && c.get("explorar") != Some(&Value::Bool(false)) {
        if let Some(e) = &p.explorar {
            alvo_troca = Some(e.clone());
            explorando = true;
        }
    }
    let Some(alvo) = alvo_troca else {
        if prejuizo && laco.marca(nick, "avisouPrejuizo") == 0.0 {
            laco.marcar(nick, "avisouPrejuizo", ag);
            ac.registrar(
                format!(
                    "{} está dando prejuízo ({} de ouro/h), mas nenhuma hunt segura tem lucro previsto positivo para o {}.",
                    pb.h_nome,
                    fmt_br(p.lucro_h.unwrap_or(0.0)),
                    pb.at_sp
                ),
                false,
            );
        }
        return ac.saida;
    };
    laco.marcar(nick, "avisouPrejuizo", 0.0);
    if prejuizo {
        ac.bloquear(&pb.h_nome, 3.0);
    }
    // trocarPara(alvo, explorando)
    let slug = slug_de(conta, &alvo.nome, Some(alvo.nivel));
    let slug_atual = slug_de(conta, &pb.h_nome, Some(pb.h_lvl));
    let Some(slug) = slug else {
        ac.saida.decisoes.push(Decisao::Registro {
            nick: nick.to_owned(),
            tipo: "aviso".into(),
            texto: format!(
                "A melhor hunt seria {} Nv {}, mas não achei o identificador dela no jogo.",
                alvo.nome,
                nv(alvo.nivel)
            ),
            ok: false,
        });
        return ac.saida;
    };
    let w_t = janela(conta, TESTE_MIN * 60e3, ag, ag);
    let antes = valor_janela(&w_t, p.obj, &p_precos(ctx), None).or(p.atual);
    let un = objetivo(p.obj).map_or("XP/h", |o| o.2);
    let (hn, hl) = (&pb.h_nome, nv(pb.h_lvl));
    let (bn, bl) = (&alvo.nome, nv(alvo.nivel));
    let bval = alvo.val.unwrap_or(0.0);
    let atual = p.atual.unwrap_or(0.0);
    let texto_ok = if prejuizo {
        format!(
            "{hn} Nv {hl} estava dando prejuízo ({} de ouro/h): troquei por {bn} Nv {bl} (lucro previsto {}/h, {} {un}). Bloqueei {hn} por 3 h.",
            fmt_br(p.lucro_h.unwrap_or(0.0)),
            fmt_br(if verd(alvo.p.lucro_h) { alvo.p.lucro_h } else { 0.0 }),
            fmt_br(bval)
        )
    } else if explorando {
        format!(
            "Explorando um degrau de nível: troquei {hn} Nv {hl} ({} {un} medidos) por {bn} Nv {bl} só para medir (previsão {} {un}). Em {} min comparo com os {} {un} de antes e volto se for pior.",
            fmt_br(atual),
            fmt_br(bval),
            nv(TESTE_MIN),
            fmt_br(antes.unwrap_or(0.0))
        )
    } else {
        format!(
            "Troquei {hn} Nv {hl} ({} {un} medidos) por {bn} Nv {bl} ({} {un} previstos). Em {} min comparo com os {} {un} de antes.",
            fmt_br(atual),
            fmt_br(bval),
            nv(TESTE_MIN),
            fmt_br(antes.unwrap_or(0.0))
        )
    };
    let texto_falha = format!("Tentei trocar {hn} por {bn} Nv {bl}, mas {{r}}. Continua em {hn}.");
    ac.saida.decisoes.push(Decisao::TrocarHunt {
        nick: nick.to_owned(),
        slug,
        nome: alvo.nome.clone(),
        nivel: jn(alvo.nivel),
        registro: Some(Registro {
            tipo: "hunt".into(),
            texto_ok,
            texto_falha,
        }),
    });
    let kph_alvo = if verd(alvo.p.kph) { alvo.p.kph } else { 1.0 };
    let enc = if verd(p.enc) { p.enc } else { 1.0 };
    ac.gravar(
        "acaoNova",
        mapa_obj(vec![
            ("t", jn(ag)),
            ("auto", json!(true)),
            ("de", json!(pb.h_nome)),
            ("nivelDe", jn(pb.h_lvl)),
            ("slugDe", slug_atual.map_or(Value::Null, Value::String)),
            ("para", json!(alvo.nome)),
            ("nivelPara", jn(alvo.nivel)),
            ("vigiar", json!(true)),
            ("teste", json!(true)),
            ("obj", json!(p.obj)),
            ("un", json!(un)),
            ("antesVal", jo(antes)),
            ("medido", jo(p.atual)),
            ("prev", jo(alvo.val)),
            ("prevBruto", jn(bval / enc)),
            ("resultado", Value::Null),
            ("potPrev", jn(alvo.p.pot_h / kph_alvo)),
            ("explorar", json!(explorando)),
            ("porPrejuizo", json!(prejuizo)),
        ]),
    );
    ac.saida
}

fn p_precos(ctx: &ContextoHunt) -> Precos {
    ctx.precos
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn formata_como_pt_br() {
        assert_eq!(fmt_br(1234567.4), "1.234.567");
        assert_eq!(fmt_br(999.5), "1.000");
        assert_eq!(fmt_br(-1500.0), "-1.500");
        assert_eq!(fmt_br(-0.4), "-0");
        assert_eq!(fmt_br(f64::NAN), "NaN");
        assert_eq!(fmt0(f64::NAN), "0");
    }

    #[test]
    fn curva_de_ouro_confere_com_o_jogo() {
        assert_eq!(curva_ouro(1.0), 41.0);
        assert!((curva_ouro(100.0) - 658.0).abs() < 1e-9);
        assert!((curva_ouro(150.0) - 840.0).abs() < 5.0);
        assert_eq!(curva_ouro(1e9), 31681.0);
    }

    #[test]
    fn modelo_de_xp() {
        assert!(modelo_xp(&[]).is_none());
        let m = modelo_xp(&[(100.0, 1000.0)]).unwrap_or(ModeloXp {
            k: 0.0,
            a: 0.0,
            min_h: 0.0,
            max_h: 0.0,
        });
        assert_eq!(m.k, 1.0);
        // dentro do envelope: a×h^k; fora, desconto crescente com piso 0,4.
        assert!((m.xp(100.0) - 1000.0).abs() < 1e-6);
        assert!(m.xp(1000.0) < 0.4 * 10000.0 + 1e-6);
    }

    #[test]
    fn calibracao_mediana_e_padrao() {
        let d = json!({"calib2": {"A": [
            {"r": 0.5, "sp": "X", "lvl": 100}, {"r": 0.9, "sp": "X", "lvl": 105},
            {"r": 0.3, "sp": "Y", "lvl": 100}, {"r": 0.7, "sp": "X", "lvl": 300}]}});
        assert_eq!(fator_calibracao(&d, "A", "X", 100.0), 0.7);
        assert_eq!(fator_calibracao(&d, "A", "Z", 100.0), CALIB_PADRAO);
        assert_eq!(calib_do_lutador(&d, "A", "X", 100.0).len(), 2);
    }

    #[test]
    fn registra_calibracao_so_com_dados_validos() {
        assert!(registrar_calibracao("A", 0.0, Some(1.0), "X", 1.0, 1.0).is_none());
        assert!(registrar_calibracao("A", 10.0, None, "X", 1.0, 1.0).is_none());
        let g = registrar_calibracao("A", 3.0, Some(1.0), "X", 100.0, 5.0);
        assert_eq!(g.map(|g| g.dados["r"].clone()), Some(json!(0.333)));
    }

    #[test]
    fn previsao_limita_kph_a_130_por_cento_do_real() {
        let l = Luta {
            kph: 1000.0,
            tkill: Some(3.0),
            dps_in: 1.0,
            burst: 1.0,
            hp: 10.0,
            bate: 2.0,
            pior: 0.1,
        };
        let real = Real {
            kph: Some(100.0),
            xp_kill: Some(50.0),
            gold_kill: None,
            pot_por_kill: Some(0.1),
            bolas_por_kill: None,
            hunt_lvl: 100.0,
        };
        let p = previsao(
            &l,
            &l,
            100.0,
            &real,
            &Precos {
                pot: 10.0,
                ball: 1.0,
            },
            1.0,
            None,
        );
        assert_eq!(p.kph, 130.0);
        assert_eq!(p.xp_h, Some(50.0 * 130.0));
    }

    #[test]
    fn objetivo_desconhecido_vira_xp() {
        assert!(objetivo("xp").is_some());
        assert!(objetivo("nada").is_none());
    }

    /// Valores do `S_.fight` / `xpModel` / `goldCurve` do legado (node, `web/sim.js`), com a
    /// pokédex em `NEXUS_POKEDEX` (padrão: a do legado).
    #[test]
    #[ignore = "precisa da pokedex.json do legado"]
    fn simulador_contra_o_legado() {
        let caminho = std::env::var("NEXUS_POKEDEX")
            .unwrap_or_else(|_| "/opt/holodex-nexus/web/pokedex.json".into());
        let Ok(dex) = Pokedex::de_json(&std::fs::read_to_string(caminho).unwrap_or_default())
        else {
            panic!("pokédex ilegível")
        };
        let k = Constantes::wiki();
        let Some(ch) = dex.por_nome("Charizard") else {
            panic!("sem Charizard")
        };
        let me = Combatente::novo(
            ch,
            300.0,
            &Opcoes {
                qualidade: 1.2,
                ivs: [20.0, 25.0, 18.0, 30.0, 22.0, 28.0],
                potencia: 3,
                ..Opcoes::default()
            },
            &k,
        );
        let sel = Opcoes {
            qualidade: 1.1,
            selvagem: true,
            ..Opcoes::default()
        };
        // (nome, nível, kph, tkill, dpsIn, burst, hp, bate)
        type Esperado<'a> = (&'a str, f64, f64, Option<f64>, f64, f64, f64, f64);
        let esperado: [Esperado; 3] = [
            (
                "Metapod",
                100.0,
                1824.0,
                Some(1.9736842105263157),
                18.103104102596365,
                265.1174674539554,
                5556.0,
                2.5,
            ),
            (
                "Rhydon",
                250.0,
                24.0,
                Some(150.0),
                594.6509147586204,
                5654.238377586206,
                5556.0,
                0.333,
            ),
            (
                "Scizor",
                320.0,
                264.0,
                Some(13.636363636363637),
                309.02998228758605,
                4959.959386034484,
                5556.0,
                5.5,
            ),
        ];
        for (n, l, kph, tk, di, b, hp, e) in esperado {
            let Some(sp) = dex.por_nome(n) else {
                panic!("sem {n}")
            };
            let r = luta(&me, &Combatente::novo(sp, l, &sel, &k), 150.0, &k);
            assert_eq!(
                (r.kph, r.tkill, r.dps_in, r.burst, r.hp, r.bate),
                (kph, tk, di, b, hp, e),
                "{n}"
            );
        }
        let m = modelo_xp(&[(100.0, 900.0), (250.0, 4000.0)]).unwrap_or(ModeloXp {
            k: 0.0,
            a: 0.0,
            min_h: 0.0,
            max_h: 0.0,
        });
        assert_eq!((m.k, m.a), (1.6279274960325414, 0.4993298366231409));
        assert_eq!(m.xp(120.0), 1210.9991464821185);
        assert_eq!(m.xp(600.0), 6653.900730411548);
        assert_eq!(m.xp(30.0), 50.71002378328106);
        assert_eq!(curva_ouro(437.5), 1596.9294479476323);
    }

    fn comparar(a: &Value, b: &Value, caminho: &str) -> Option<String> {
        match (a, b) {
            (Value::Number(x), Value::Number(y)) => {
                let (x, y) = (
                    x.as_f64().unwrap_or(f64::NAN),
                    y.as_f64().unwrap_or(f64::NAN),
                );
                let tol = 1e-3 * x.abs().max(y.abs()).max(1.0);
                ((x - y).abs() > tol).then(|| format!("{caminho}: {x} != {y}"))
            }
            (Value::Object(x), Value::Object(y)) => {
                for (k, v) in x {
                    match y.get(k) {
                        None => return Some(format!("{caminho}.{k}: falta no legado")),
                        Some(w) => {
                            if let Some(e) = comparar(v, w, &format!("{caminho}.{k}")) {
                                return Some(e);
                            }
                        }
                    }
                }
                y.keys()
                    .find(|k| !x.contains_key(*k))
                    .map(|k| format!("{caminho}.{k}: falta no Nexus"))
            }
            (Value::Array(x), Value::Array(y)) => {
                if x.len() != y.len() {
                    return Some(format!("{caminho}: tamanho {} != {}", x.len(), y.len()));
                }
                x.iter()
                    .zip(y)
                    .enumerate()
                    .find_map(|(i, (a, b))| comparar(a, b, &format!("{caminho}[{i}]")))
            }
            _ => (a != b).then(|| format!("{caminho}: {a} != {b}")),
        }
    }

    /// Compara `plano_hunt` com o `planoHunt` do legado (`estado.json`) por conta.
    /// `NEXUS_FIXTURES_MOTOR=/root/fixtures-motor-pc/<data>` e, opcional, `NEXUS_POKEDEX`.
    #[test]
    #[ignore = "precisa das fixtures reais em NEXUS_FIXTURES_MOTOR"]
    fn plano_hunt_contra_o_legado() {
        let Ok(pasta) = std::env::var("NEXUS_FIXTURES_MOTOR") else {
            return;
        };
        let ler = |n: &str| -> Value {
            let t = std::fs::read_to_string(format!("{pasta}/{n}")).unwrap_or_default();
            serde_json::from_str(&t).unwrap_or(Value::Null)
        };
        let pokedex = std::env::var("NEXUS_POKEDEX")
            .unwrap_or_else(|_| "/opt/holodex-nexus/web/pokedex.json".into());
        let dex = Pokedex::de_json(&std::fs::read_to_string(pokedex).unwrap_or_default());
        let Ok(dex) = dex else {
            panic!("pokédex ilegível")
        };
        let janela_json = ler("nexus-janela.json");
        let d = config::carregar(&ler("nexus-dados.json"));
        let estado = ler("estado.json");
        let agora = janela_json["agora"].as_i64().unwrap_or(0);
        let r = Relogio {
            agora_ms: agora,
            fuso_ms: -3 * 3_600_000,
        };
        let mut falhas = Vec::new();
        for cj in janela_json["contas"]
            .as_array()
            .cloned()
            .unwrap_or_default()
        {
            let Some(conta) = ContaViva::de_json(&cj) else {
                continue;
            };
            let nick = conta.nick.clone();
            let legado = estado["contas"]
                .as_array()
                .and_then(|a| a.iter().find(|c| c["nick"] == nick.as_str()))
                .map(|c| c["plano"]["hunt"].clone());
            let Some(legado) = legado else { continue };
            let k = constantes_da_conta(&conta);
            let compra = estado["contas"]
                .as_array()
                .and_then(|a| a.iter().find(|c| c["nick"] == nick.as_str()))
                .map(|c| c["plano"]["compra"].clone())
                .unwrap_or(Value::Null);
            let ctx = ContextoHunt {
                dex: &dex,
                k: &k,
                precos: Precos {
                    pot: js::mais(compra.get("pocaoPreco")),
                    ball: js::mais(compra.get("bolaPreco")),
                },
                limite_rodizio: None,
                equipe_no_comando: false,
                rodizio_volta: false,
                hunt_desde: js::mais(d.pointer(&format!("/linhasCache/{nick}/huntDesde"))).max(0.0),
            };
            let nexus = plano_hunt(&d, &r, &conta, &ctx);
            match comparar(&nexus, &legado, "") {
                None => eprintln!(
                    "{nick}: IGUAL ({} chaves)",
                    nexus.as_object().map_or(0, Map::len)
                ),
                Some(e) => {
                    eprintln!("{nick}: DIFERE {e}");
                    falhas.push(format!("{nick}: {e}"));
                }
            }
        }
        assert!(falhas.is_empty(), "{falhas:?}");
    }
}
