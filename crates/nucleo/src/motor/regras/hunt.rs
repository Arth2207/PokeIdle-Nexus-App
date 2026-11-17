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

mod selecao;

const TESTE_MIN: f64 = 5.0;
#[cfg(test)]
const CALIB_PADRAO: f64 = 0.75;
/// `OBJETIVOS`: chave, nome, unidade.
const OBJETIVOS: [(&str, &str, &str); 5] = [
    ("xp", "XP", "XP/h"),
    ("lucro", "lucro", "ouro/h"),
    ("equilibrio", "XP sem prejuízo", "XP/h"),
    ("gemas", "gemas", "gemas/h"),
    ("shiny", "chance de shiny", "shiny/dia"),
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
    let p = if conta.publico.is_object() {
        &conta.publico
    } else {
        &conta.welcome
    };
    if let Some(mut servidor) = Constantes::com_tabela_do_servidor(&conta.welcome["tabelaTipos"]) {
        if let Some(tab) = servidor.tabela.as_mut() {
            for tipo in crate::combate::tipos::Tipo::TODOS {
                if conta.welcome["tabelaTipos"][tipo.nome()]
                    .get(tipo.nome())
                    .is_none()
                {
                    tab[tipo.indice()][tipo.indice()] =
                        crate::combate::tipos::efetividade(tipo, tipo);
                }
            }
        }
        k.tabela = servidor.tabela;
    }
    let hp = js::mais(
        p.get("multHpSelvagem")
            .or_else(|| conta.welcome.get("multHpSelvagem")),
    );
    if hp > 0.0 {
        k.hp_selvagem = hp;
    }
    let dn = js::mais(p.get("multDanoSelvagem"));
    if dn > 0.0 {
        k.dano_selvagem = dn;
    }
    if let Some(amp) = p
        .get("ampliacaoHunt")
        .and_then(Value::as_f64)
        .filter(|a| *a > 0.0)
    {
        k.ampliacao_hunt = [0.0, 0.0625, 0.125, 0.25, 0.5, 1.0, 2.0, 4.0, 8.0, 16.0]
            .into_iter()
            .map(|v| {
                (
                    v,
                    if v == 0.0 || v == 1.0 {
                        v
                    } else if v > 1.0 {
                        1.0 + (v - 1.0) * amp
                    } else {
                        v / amp
                    },
                )
            })
            .collect();
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

#[derive(Debug, Clone, Copy)]
struct Previsao {
    kph: f64,
    xp_h: Option<f64>,
    lucro_h: f64,
    pot_h: f64,
}

// ---------------------------------------------------------------- valores por objetivo

/// `G` do legado: `gemaPrecos` mais `_chShiny`.
struct Gemas {
    precos: Value,
    ch_shiny: f64,
    out_mult: Option<f64>,
    token_chance: Option<f64>,
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
    capturas_por_kill: Option<f64>,
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
        let Some(mult) = g.out_mult else {
            return f64::NAN;
        };
        let Some(token) = g.token_chance else {
            return f64::NAN;
        };
        let mult = mult * ou(Some(js::mais(g.precos.get("outlandBonus"))), 1.0);
        // As chances atuais dos fragmentos não vieram no login: não inventa receita.
        let soma = token * g.g("bronzeToken");
        return kph * soma * mult;
    }
    let Some(p_cap) = sp.capturas_por_kill else {
        return f64::NAN;
    };
    kph * g.ch_shiny * p_cap * g.g("shiny")
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
        "shiny" => (w.span >= 60_000.0)
            .then(|| w.capturas as f64 * 3_600_000.0 / w.span * ou(ch, 1.0 / 24000.0) * 24.0),
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
            ou(Some(p.kph), 0.0)
                * ou(Some(g.ch_shiny), 1.0 / 24000.0)
                * sp.capturas_por_kill.unwrap_or(f64::NAN)
                * 24.0
        }),
        "lucro" => Some(p.lucro_h),
        _ => p.xp_h,
    }
}

// ---------------------------------------------------------------- calibração (D.calib2)

/// `calibDoLutador`: as razões `r` do mesmo lutador (espécie e nível ±15%).
#[cfg(test)]
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
#[cfg(test)]
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
    shiny: bool,
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
        shiny: conta
            .ultimo
            .pointer("/io/active/shiny")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}

struct Alt<'a> {
    sp: &'a EspecieCombate,
    nome: String,
    slug: String,
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
    _d: &Value,
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
    let mut me =
        crate::combate::combatente::de_ficha(ctx.dex, &conta.ultimo["io"]["active"], ctx.k)
            .map(|(mon, _)| mon)
            .unwrap_or_else(|| {
                Combatente::novo(
                    at.sp,
                    at.lvl,
                    &Opcoes {
                        qualidade: at.q,
                        ivs: at.iv,
                        potencia: pot,
                        shiny: at.shiny,
                        ..Opcoes::default()
                    },
                    ctx.k,
                )
            });
    crate::combate::combatente::aplicar_z_move(&mut me, &conta.ultimo["io"]["active"], 2800.0);
    let dia = conta.ultimo["dia"]
        .as_str()
        .and_then(|t| t.parse::<crate::combate::tipos::Tipo>().ok());
    let selvagem = Opcoes {
        qualidade: 1.1,
        selvagem: true,
        ..Opcoes::default()
    };
    let f = |sp: &EspecieCombate, lvl: f64, secs: f64| {
        let mut meu = me.clone();
        let mut foe = Combatente::novo(sp, lvl, &selvagem, ctx.k);
        // O bônus sazonal vale nas hunts do Mapa, não nas Outlands.
        if !eh_outland(Some(sp.id)) {
            if let Some(tipo) = dia {
                if meu.tipos.contains(&tipo) {
                    crate::combate::combatente::multiplicar_atributos(&mut meu, 2.0);
                }
                if foe.tipos.contains(&tipo) {
                    crate::combate::combatente::multiplicar_atributos(&mut foe, 2.0);
                }
            }
        }
        luta(&meu, &foe, secs, ctx.k)
    };
    let cur = ctx.dex.por_nome(&h_nome).map(|w| f(w, h_lvl, 180.0));
    // O catálogo vivo é a autoridade: a mesma espécie pode existir em regiões/níveis diferentes.
    let lista = conta.welcome.get("hunts").and_then(Value::as_array);
    let alts = lista
        .into_iter()
        .flatten()
        .filter_map(|h| {
            let nivel = h.get("nivel")?.as_f64()?;
            let nome = h.get("nome")?.as_str()?;
            let slug = h.get("slug")?.as_str()?;
            if nivel <= 0.0
                || nivel > tr
                || h.get("bloqueada").and_then(Value::as_bool) == Some(true)
            {
                return None;
            }
            let especies = h.get("especies").and_then(Value::as_array);
            if especies.is_some_and(|es| es.len() != 1) {
                return None;
            }
            let id = especies
                .and_then(|es| es.first())
                .and_then(|e| e.get("pokeId"))
                .and_then(Value::as_i64);
            let sp = id
                .and_then(|id| ctx.dex.especies().iter().find(|s| s.id == id))
                .or_else(|| ctx.dex.por_nome(nome))?;
            let out =
                h.get("area").and_then(Value::as_str) == Some("outland") || eh_outland(Some(sp.id));
            if (modo == "out" && !out) || (modo == "shiny" && out) {
                return None;
            }
            Some(Alt {
                sp,
                nome: nome.to_owned(),
                slug: slug.to_owned(),
                h: nivel,
                r: f(sp, nivel, 90.0),
            })
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
    slug: String,
    nivel: f64,
    p: Previsao,
    val: Option<f64>,
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

fn contexto_medida(conta: &ContaViva) -> Value {
    selecao::contexto(conta)
}

fn acao_hunt<'a>(d: &'a Value, nick: &str) -> Option<&'a Value> {
    d.get("acoes")?.as_array()?.iter().find(|a| {
        a.get("nick").and_then(Value::as_str) == Some(nick)
            && a.get("tipo").and_then(Value::as_str) == Some("hunt")
            && ver(a, "ok")
    })
}

fn plano_completo(d: &Value, r: &Relogio, conta: &ContaViva, ctx: &ContextoHunt) -> Plano {
    selecao::plano(d, r, conta, ctx)
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
    if super::safari_pago(conta)
        || conta.ultimo["sessaoTomada"] == true
        || conta.ultimo.pointer("/hdx/ws") == Some(&Value::Bool(false))
    {
        return SaidaHunt::default();
    }
    if config::efetiva(d, &conta.nick).pointer("/safari/ativo") == Some(&Value::Bool(true)) {
        return SaidaHunt {
            decisoes: super::safari::motor(d, laco, r, conta, ctx.dex),
            gravacoes: Vec::new(),
        };
    }
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
    use std::hash::{Hash, Hasher};
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    contexto_medida(conta).to_string().hash(&mut hash);
    let assinatura = (hash.finish() & ((1_u64 << 52) - 1)) as f64;
    if laco.marca(nick, "huntContextoV2") != assinatura {
        laco.marcar(nick, "huntContextoV2", assinatura);
        laco.marcar(nick, "huntContextoDesde", ag);
    }
    let ctx_local = ContextoHunt {
        dex: ctx.dex,
        k: ctx.k,
        precos: ctx.precos,
        limite_rodizio: ctx.limite_rodizio,
        equipe_no_comando: ctx.equipe_no_comando,
        rodizio_volta: ctx.rodizio_volta,
        hunt_desde: ctx.hunt_desde.max(laco.marca(nick, "huntContextoDesde")),
    };
    let ctx = &ctx_local;
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
                    "h": jn(pb.h_lvl), "contexto": contexto_medida(conta), "kills": wm.kills, "span": wm.span,
                    "capturasPorKill": wm.capturas as f64 / wm.kills as f64, "xpKill": jo(wm.xp_kill), "kph": jo(wm.kph), "xpH": jo(wm.xp_h),
                    "t": jn(ag), "sp": quem_sp, "lvl": jn(pb.at_lvl) } }),
            );
            ac.gravar(
                "medHist",
                json!({ "chave": chave, "ponto": [jn(ag), jn(js::round(ou(wm.xp_h, 0.0)))] }),
            );
        }
    }
    if !ver(c, "ativo") {
        return ac.saida;
    }
    // `voltar`: assume resposta "ok" do jogo.
    let voltar = |ac: &mut Acumulo,
                  motivo: String,
                  horas: f64,
                  resultado: Option<Value>,
                  _seguranca: bool| {
        let Some(u) = ac.ult.clone() else {
            return;
        };
        let de = txt(&u, "de");
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
                    custo: None,
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
    if let Some(u) = ac
        .ult
        .clone()
        .filter(|u| ver(u, "teste") || ver(u, "vigiar") || !u["acomp"].is_null())
    {
        if u["versao"] != 2 || u["contexto"] != contexto_medida(conta) {
            ac.mexer_acao(vec![
                ("teste", json!(false)),
                ("vigiar", json!(false)),
                ("acomp", Value::Null),
            ]);
            ac.registrar(
                "Reiniciei a medição: mudou o lutador, os bônus ou o contexto da hunt.".into(),
                true,
            );
            return ac.saida;
        }
    }
    // vigia a última troca: desmaiou ou gastou o dobro de poção do previsto
    if let Some(u) = ac.ult.clone() {
        if ver(&u, "vigiar")
            && ag - nm(&u, "t") <= 30.5 * 60e3
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
            let kills =
                p.o.get("killsDepois")
                    .and_then(Value::as_f64)
                    .unwrap_or(0.0);
            if kills < 20.0 || t.depois.is_none_or(|v| !v.is_finite()) {
                if t.passou >= 15.0 * 60e3 {
                    ac.mexer_acao(vec![("teste", json!(false)), ("vigiar", json!(false))]);
                    ac.registrar("Teste encerrado sem comparação: amostra insuficiente em 15 minutos; vou medir novamente antes de trocar.".into(), false);
                }
                return ac.saida;
            }
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
                    let cfg = config::efetiva(d, nick);
                    let mut precos = cfg["gemaPrecos"].clone();
                    if let Some(v) = conta.welcome.get("nexusTokenPreco") {
                        precos["bronzeToken"] = v.clone();
                    }
                    let g = Gemas {
                        precos,
                        ch_shiny,
                        out_mult: crate::outlands::atual(&conta.welcome, &conta.ultimo)
                            .map(|t| t.mult),
                        token_chance: crate::outlands::chance_token(&conta.welcome),
                    };
                    let out = ctx
                        .dex
                        .por_nome(&pb.h_nome)
                        .is_some_and(|s| eh_outland(Some(s.id)));
                    let val = selecao::valor_atual(&wa, &a_obj, &p_precos(ctx), &g, out);
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
                            ("vigiar", json!(false)),
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
    let slug = Some(alvo.slug.clone());
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
    let antes = p.atual;
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
            custo: None,
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
            ("versao", json!(2)),
            ("contexto", contexto_medida(conta)),
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
    fn shiny_usa_capturas_e_nao_abates() {
        let c = ContaViva {
            linhas: vec![
                (
                    1,
                    0,
                    "Abra derrotado! +1 xp treinador, +1 xp pokemon, +1 ouro".into(),
                ),
                (2, 60_000, "Capturado: Abra".into()),
            ],
            ..Default::default()
        };
        let w = janela(&c, 60_000.0, 60_000.0, 60_000.0);
        assert_eq!(w.capturas, 1);
        let v = valor_janela(&w, "shiny", &Precos::default(), Some(1.0 / 24000.0));
        assert!((v.unwrap_or(0.0) - 0.06).abs() < 1e-9);
    }

    #[test]
    fn outland_usa_chance_e_tier_reais_sem_receita_inventada_de_fragmentos() {
        let g = Gemas {
            precos: json!({"bronzeToken":100,"fragChave":100000,"outlandBonus":1}),
            ch_shiny: 0.0,
            out_mult: Some(1.25),
            token_chance: Some(0.0001),
        };
        assert_eq!(
            valor_gemas_h(
                Some(1000.0),
                &EspValor {
                    d: Some(2001),
                    capturas_por_kill: None
                },
                &g
            ),
            12.5
        );
        let sem_tier = Gemas {
            out_mult: None,
            ..g
        };
        assert!(valor_gemas_h(
            Some(1000.0),
            &EspValor {
                d: Some(2001),
                capturas_por_kill: None
            },
            &sem_tier
        )
        .is_nan());
    }

    #[test]
    fn outland_150_continua_candidata_para_lutador_1000() -> Result<(), Box<dyn std::error::Error>>
    {
        let dex = Pokedex::de_valor(json!([
            {"d":6,"n":"Charizard","t":["FIRE","FLYING"],"s":[78,84,78,109,85,100],"h":80,"c":"pokebola","m":[["Ember",100,1,"E","FIRE"]]},
            {"d":2001,"n":"Furious Scizor","t":["BUG","STEEL"],"s":[70,130,100,55,80,65],"h":150,"c":"pokebola","m":[["Tackle",40,5,"F","NORMAL"]]},
            {"d":2002,"n":"Tribal Scizor","t":["BUG","STEEL"],"s":[70,130,100,55,80,65],"h":150,"c":"pokebola","m":[["Tackle",40,5,"F","NORMAL"]]}
        ]))?;
        let r = Relogio {
            agora_ms: 1_800_000_000_000,
            fuso_ms: 0,
        };
        let c = ContaViva {
            nick: "Ana".into(),
            hunt_desde: r.agora_ms - 1_200_000,
            ultimo: json!({"hunt":"Furious Scizor · Outland 104 · nível 150","trLv":"1000","outlandTier":104,
                "ativo":{"nome":"Charizard","lv":"Nv 1000","q":"1,5","pot":"P1"}}),
            welcome: json!({"hunts":[{"nome":"Furious Scizor","slug":"furious","nivel":150,"area":"outland"},{"nome":"Tribal Scizor","slug":"tribal","nivel":150,"area":"outland"}],"outlandTiers":[{"tier":104,"numero":4,"nivel":1000,"mult":1.25,"rotulo":"Outland 4"}],"chanceBossToken":0.0001,"nexusTokenPreco":100}),
            linhas: (0..25)
                .map(|i| {
                    (
                        i,
                        r.agora_ms - 60_000 + i * 1000,
                        "Furious Scizor derrotado! +10 xp treinador, +10 xp pokemon, +10 ouro"
                            .into(),
                    )
                })
                .collect(),
            ..Default::default()
        };
        let k = Constantes::wiki();
        let ctx = ContextoHunt {
            dex: &dex,
            k: &k,
            precos: Precos::default(),
            limite_rodizio: None,
            equipe_no_comando: false,
            rodizio_volta: false,
            hunt_desde: c.hunt_desde as f64,
        };
        let d = json!({"config":{"Ana":{"hunt":{"objetivo":"gemas","soOutland":true,"maxPiorPct":10000}}}});
        let plano = plano_completo(&d, &r, &c, &ctx);
        assert!(
            plano.base.is_some(),
            "a hunt deve ser reconhecida sem o sufixo de apresentação"
        );
        assert!(
            plano.best.is_some(),
            "Outland 150 não deve ser excluída pelo piso de XP de 500"
        );
        Ok(())
    }

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
    }

    /// Replay opcional: o novo seletor não deve mais reproduzir as heurísticas do legado.
    #[test]
    #[ignore = "precisa das fixtures reais em NEXUS_FIXTURES_MOTOR"]
    fn plano_hunt_replay_com_catalogo_do_jogo() -> Result<(), Box<dyn std::error::Error>> {
        let pasta = std::env::var("NEXUS_FIXTURES_MOTOR")?;
        let ler = |n: &str| -> Result<Value, Box<dyn std::error::Error>> {
            Ok(serde_json::from_slice(&std::fs::read(format!(
                "{pasta}/{n}"
            ))?)?)
        };
        let pokedex = std::env::var("NEXUS_POKEDEX")
            .unwrap_or_else(|_| "/opt/holodex-nexus/web/pokedex.json".into());
        let dex = Pokedex::de_json(&std::fs::read_to_string(pokedex)?)?;
        let j = ler("nexus-janela.json")?;
        let d = config::carregar(&ler("nexus-dados.json")?);
        let r = Relogio {
            agora_ms: j["agora"].as_i64().unwrap_or(0),
            fuso_ms: -3 * 3_600_000,
        };
        let contas = j["contas"].as_array().ok_or("faltam contas")?;
        assert!(!contas.is_empty());
        for v in contas {
            let c = ContaViva::de_json(v).ok_or("conta inválida")?;
            let k = constantes_da_conta(&c);
            let ctx = ContextoHunt {
                dex: &dex,
                k: &k,
                precos: Precos::default(),
                limite_rodizio: None,
                equipe_no_comando: false,
                rodizio_volta: false,
                hunt_desde: c.hunt_desde as f64,
            };
            let p = plano_hunt(&d, &r, &c, &ctx);
            assert_eq!(p["versao"], 2);
            let tr = num(c.ultimo.get("trLv")).unwrap_or(0.0);
            for cand in p["candidatos"].as_array().ok_or("falta ranking")? {
                assert!(cand["nivel"].as_f64().is_some_and(|n| n <= tr));
                assert!(c.welcome["hunts"]
                    .as_array()
                    .is_some_and(|hs| hs.iter().any(|h| h["slug"] == cand["slug"]
                        && h["nivel"].as_f64() == cand["nivel"].as_f64())));
                assert!(cand["val"].as_f64().is_some_and(f64::is_finite));
            }
        }
        Ok(())
    }
}
