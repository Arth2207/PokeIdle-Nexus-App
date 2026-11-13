//! Partes do `sim.js` que o `nucleo` ainda não tinha: a luta contínua contra selvagens
//! (`fight`), o veredito e as funções de espécie (`stageOf`, `captureLevel`, `moveCat`).

use std::collections::{HashMap, VecDeque};

use nucleo::combate::combatente::{dano, dano_investida, tipo_investida, Combatente};
use nucleo::combate::especie::{Categoria, EspecieCombate};
use nucleo::combate::tipos::{Constantes, Tipo};
use nucleo::js;

/// Resultado de uma luta (`fight` do legado).
pub struct Luta {
    pub kph: f64,
    /// `None` quando não houve nenhum abate (o `null` do JS).
    pub tkill: Option<f64>,
    pub burst: f64,
    pub hp: f64,
    pub bate: f64,
    pub bate_t: Tipo,
    pub apanha: f64,
    pub apanha_t: Tipo,
    pub t_die: f64,
}

/// Selvagem em combate (o objeto `w` do JS).
struct Selvagem {
    hp: f64,
    pronto: Vec<f64>,
    prox: f64,
    inv: f64,
}

/// `coverage`: o maior multiplicador de tipo que o atacante tem contra o defensor.
fn cobertura(att: &Combatente, def: &Combatente, k: &Constantes) -> (f64, Tipo) {
    let (t0, e0) = tipo_investida(att, def, k).unwrap_or((Tipo::Normal, f64::NAN));
    let mut melhor = (e0, t0);
    for g in &att.golpes {
        let e = k.efetividade_hunt(g.tipo, &def.tipos);
        if e > melhor.0 {
            melhor = (e, g.tipo);
        }
    }
    melhor
}

/// `fight(me, foe, secs, 0.1)`: um selvagem por vez, cada um nasce com as recargas zeradas.
pub fn lutar(me: &Combatente, foe: &Combatente, secs: f64, k: &Constantes) -> Luta {
    let dt = 0.1;
    // `ready` do JS é um objeto indexado pelo NOME do golpe: golpes repetidos dividem a
    // recarga. Um HashMap<nome, tempo> reproduz isso (em Rust, HashMap ~ objeto/Map do JS).
    let mut pronto: HashMap<&str, f64> = me.golpes.iter().map(|m| (m.nome.as_str(), 0.0)).collect();
    let meu_dano: Vec<f64> = me
        .golpes
        .iter()
        .map(|m| dano(me, foe, m.poder, m.categoria, m.tipo, me.stab(m.tipo), k))
        .collect();
    let dano_foe: Vec<f64> = foe
        .golpes
        .iter()
        .map(|m| dano(foe, me, m.poder, m.categoria, m.tipo, foe.stab(m.tipo), k))
        .collect();
    let minha_inv = dano_investida(me, foe, k);
    let inv_foe = dano_investida(foe, me, k);

    let (mut t, mut kills, mut tomado) = (0.0_f64, 0.0_f64, 0.0_f64);
    let (mut prox_me, mut prox_inv, mut max_burst) = (0.0_f64, 0.0_f64, 0.0_f64);
    let mut w: Option<Selvagem> = None;
    let mut janela: VecDeque<(f64, f64)> = VecDeque::new();
    let mut soma_janela = 0.0_f64;
    while t < secs {
        let s = w.get_or_insert_with(|| Selvagem {
            hp: foe.hp,
            pronto: vec![t; foe.golpes.len()],
            prox: t,
            inv: t,
        });
        if t >= prox_me {
            let mut bi: Option<usize> = None;
            for (i, g) in me.golpes.iter().enumerate() {
                let livre = pronto.get(g.nome.as_str()).is_some_and(|&r| r <= t);
                let melhor = bi.is_none_or(|b| meu_dano.get(i) > meu_dano.get(b));
                if livre && melhor {
                    bi = Some(i);
                }
            }
            if let Some(b) = bi {
                if let (Some(g), Some(&d)) = (me.golpes.get(b), meu_dano.get(b)) {
                    s.hp -= d;
                    pronto.insert(g.nome.as_str(), t + g.recarga * me.mult_recarga);
                }
            }
            prox_me = t + me.intervalo;
        }
        if t >= prox_inv {
            s.hp -= minha_inv;
            prox_inv = t + me.intervalo_investida;
        }
        let mut hit = 0.0_f64;
        if t >= s.prox {
            let mut bi: Option<usize> = None;
            for i in 0..foe.golpes.len() {
                let livre = s.pronto.get(i).is_some_and(|&r| r <= t);
                let melhor = bi.is_none_or(|b| dano_foe.get(i) > dano_foe.get(b));
                if livre && melhor {
                    bi = Some(i);
                }
            }
            if let Some(b) = bi {
                if let (Some(g), Some(&d)) = (foe.golpes.get(b), dano_foe.get(b)) {
                    hit += d;
                    if let Some(p) = s.pronto.get_mut(b) {
                        *p = t + g.recarga * foe.mult_recarga;
                    }
                }
            }
            s.prox = t + foe.intervalo;
        }
        if t >= s.inv {
            hit += inv_foe;
            s.inv = t + foe.intervalo_investida;
        }
        tomado += hit;
        janela.push_back((t, hit));
        soma_janela += hit;
        while janela.front().is_some_and(|&(ti, _)| ti < t - 5.0) {
            if let Some((_, h)) = janela.pop_front() {
                soma_janela -= h;
            }
        }
        if soma_janela > max_burst {
            max_burst = soma_janela;
        }
        if s.hp <= 0.0 {
            kills += 1.0;
            w = None;
        }
        t += dt;
    }
    let (bate, bate_t) = cobertura(me, foe, k);
    let (apanha, apanha_t) = cobertura(foe, me, k);
    Luta {
        kph: kills * 3600.0 / secs,
        tkill: (kills != 0.0).then(|| secs / kills),
        burst: max_burst,
        hp: me.hp,
        bate,
        bate_t,
        apanha,
        apanha_t,
        t_die: if tomado != 0.0 {
            me.hp / (tomado / secs)
        } else {
            f64::INFINITY
        },
    }
}

/// `verdict(r)`: (texto do legado, classe css).
pub fn veredito(r: &Luta) -> (&'static str, &'static str) {
    let b = r.burst / r.hp;
    if b < 0.5 {
        ("SEGURO", "ok")
    } else if b < 1.0 {
        ("ARRISCADO", "warn")
    } else {
        ("MORRE", "bad")
    }
}

/// Rótulo exibido do veredito.
pub fn rotulo(v: &str) -> &'static str {
    match v {
        "SEGURO" => "Seguro",
        "ARRISCADO" => "Arriscado",
        _ => "Morre",
    }
}

/// `stageOf`: (nome, teto, pedra) pelo multiplicador `b[4]`.
pub fn estagio(sp: &EspecieCombate) -> Option<(&'static str, f64)> {
    match sp.estagio {
        Some(2.93) => Some(("1º de cadeia de três", 20.0)),
        Some(2.35) => Some(("meio, ou 1º de cadeia de dois", 40.0)),
        Some(1.88) => Some(("última evolução ou não evolui", 100.0)),
        _ => None,
    }
}

/// `captureLevel`: `None` é o `null` do JS.
pub fn nivel_captura(sp: &EspecieCombate) -> Option<f64> {
    let h = sp.nivel_captura?;
    if h <= 100.0 {
        return Some(h);
    }
    Some(estagio(sp).map_or(100.0, |(_, teto)| teto))
}

/// `moveCat`: a categoria que soma mais poder (golpes com poder entre 1 e 599).
pub fn categoria_principal(sp: &EspecieCombate) -> Categoria {
    let (mut f, mut e) = (0.0, 0.0);
    for g in &sp.golpes {
        if g.poder > 0.0 && g.poder < 600.0 {
            if g.categoria == Categoria::Fisico {
                f += g.poder;
            } else {
                e += g.poder;
            }
        }
    }
    if f >= e {
        Categoria::Fisico
    } else {
        Categoria::Especial
    }
}

/// `bst`: soma dos stats base.
pub fn bst(sp: &EspecieCombate) -> f64 {
    sp.base.iter().fold(0.0, |a, b| a + b)
}

/// `Math.round((a/b - 1) * 100)`.
pub fn pct(a: f64, b: f64) -> f64 {
    js::round((a / b - 1.0) * 100.0)
}
