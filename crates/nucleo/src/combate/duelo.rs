//! Duelo de uma carta contra o boss e a pontuação dela (`duelo`, `avaliar`, `bestEff` e a
//! penalidade de equipe do `equipes-core.js`).

use super::combatente::{dano, dano_investida, Combatente};
use super::tipos::{Constantes, Tipo};
use crate::js;

/// Teto do tempo até cair, em segundos (`TCAP`).
pub const TEMPO_MAXIMO: f64 = 300.0;
/// Duração do duelo simulado (`SECS`).
pub const DURACAO_DUELO: f64 = 40.0;
/// Tamanho da equipe (`K_EQUIPE`).
pub const TAMANHO_EQUIPE: usize = 5;

/// Dano por segundo causado e recebido num duelo.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Duelo {
    pub dps_causado: f64,
    pub dps_recebido: f64,
}

/// Danos de cada golpe de `att` contra `def`, na ordem dos golpes.
fn danos_golpes(att: &Combatente, def: &Combatente, k: &Constantes) -> Vec<f64> {
    att.golpes
        .iter()
        .map(|g| dano(att, def, g.poder, g.categoria, g.tipo, att.stab(g.tipo), k))
        .collect()
}

/// Índice do golpe pronto mais forte; no empate fica o primeiro (`>` estrito, como no JS).
fn melhor_pronto(danos: &[f64], pronto_em: &[f64], t: f64) -> Option<usize> {
    let mut melhor: Option<usize> = None;
    for (i, (&d, &p)) in danos.iter().zip(pronto_em).enumerate() {
        if p <= t && melhor.is_none_or(|b| danos.get(b).is_some_and(|&db| d > db)) {
            melhor = Some(i);
        }
    }
    melhor
}

/// Lado de um duelo: cada um usa o golpe pronto mais forte a cada `intervalo` e a
/// investida a cada `intervalo_investida`.
struct Lado<'a> {
    mon: &'a Combatente,
    danos: Vec<f64>,
    investida: f64,
    pronto_em: Vec<f64>,
    proximo_golpe: f64,
    proxima_investida: f64,
}

impl<'a> Lado<'a> {
    fn novo(mon: &'a Combatente, alvo: &Combatente, k: &Constantes) -> Self {
        Lado {
            mon,
            danos: danos_golpes(mon, alvo, k),
            investida: dano_investida(mon, alvo, k),
            pronto_em: vec![0.0; mon.golpes.len()],
            proximo_golpe: 0.0,
            proxima_investida: 0.0,
        }
    }

    /// Soma em `total` o dano que este lado causa no instante `t`. Soma direto no total, golpe
    /// e investida separados, para o arredondamento de float sair igual ao do JS.
    fn passo(&mut self, t: f64, total: &mut f64) {
        if t >= self.proximo_golpe {
            if let Some(b) = melhor_pronto(&self.danos, &self.pronto_em, t) {
                if let (Some(d), Some(g), Some(p)) = (
                    self.danos.get(b),
                    self.mon.golpes.get(b),
                    self.pronto_em.get_mut(b),
                ) {
                    *total += d;
                    *p = t + g.recarga * self.mon.mult_recarga;
                }
            }
            self.proximo_golpe = t + self.mon.intervalo;
        }
        if t >= self.proxima_investida {
            *total += self.investida;
            self.proxima_investida = t + self.mon.intervalo_investida;
        }
    }
}

/// `duelo(me, foe, mult, secs, dt)`. O tempo anda somando `dt` (com o mesmo acúmulo de erro
/// de float do JS), e o dano recebido é multiplicado pela penalidade de equipe `mult`.
pub fn duelo(
    me: &Combatente,
    foe: &Combatente,
    mult: f64,
    segundos: f64,
    dt: f64,
    k: &Constantes,
) -> Duelo {
    if dt.is_nan() || dt <= 0.0 {
        // Passo inválido: o laço não terminaria.
        return Duelo {
            dps_causado: 0.0,
            dps_recebido: 0.0,
        };
    }
    let mut eu = Lado::novo(me, foe, k);
    let mut ele = Lado::novo(foe, me, k);
    let (mut causado, mut recebido, mut t) = (0.0, 0.0, 0.0);
    while t < segundos {
        eu.passo(t, &mut causado);
        ele.passo(t, &mut recebido);
        t += dt;
    }
    Duelo {
        dps_causado: causado / segundos,
        dps_recebido: recebido / segundos * mult,
    }
}

/// Melhor efetividade que o atacante consegue (pelos tipos dele e pelos golpes).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Eficacia {
    pub mult: f64,
    pub tipo: Option<Tipo>,
}

/// `bestEff`.
pub fn melhor_eficacia(att: &Combatente, def: &Combatente, k: &Constantes) -> Eficacia {
    let mut b = Eficacia {
        mult: 0.0,
        tipo: att.tipos.first().copied(),
    };
    let candidatos = att
        .tipos
        .iter()
        .copied()
        .chain(att.golpes.iter().map(|g| g.tipo));
    for t in candidatos {
        let e = k.efetividade_hunt(t, &def.tipos);
        if e > b.mult {
            b = Eficacia {
                mult: e,
                tipo: Some(t),
            };
        }
    }
    b
}

/// Resultado do `avaliar`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Avaliacao {
    /// Dano entregue antes de cair: `dps causado × tempo até cair`.
    pub d: f64,
    pub dps_causado: f64,
    pub dps_recebido: f64,
    /// Tempo até cair, limitado a [`TEMPO_MAXIMO`].
    pub tempo_ate_cair: f64,
    pub bate: Eficacia,
    pub apanha: Eficacia,
    /// O boss não acerta nada nele.
    pub imune: bool,
}

/// `avaliar(me, boss, mult)`.
pub fn avaliar(me: &Combatente, boss: &Combatente, mult: f64, k: &Constantes) -> Avaliacao {
    let r = duelo(me, boss, mult, DURACAO_DUELO, 0.1, k);
    let tempo = if r.dps_recebido > 0.0 {
        TEMPO_MAXIMO.min(me.hp / r.dps_recebido)
    } else {
        TEMPO_MAXIMO
    };
    let apanha = melhor_eficacia(boss, me, k);
    Avaliacao {
        d: r.dps_causado * tempo,
        dps_causado: r.dps_causado,
        dps_recebido: r.dps_recebido,
        tempo_ate_cair: tempo,
        bate: melhor_eficacia(me, boss, k),
        apanha,
        imune: apanha.mult == 0.0,
    }
}

/// Força da equipe e o multiplicador do dano recebido (`multPenalidade`):
/// força = Σ mín(1, nível / nível do boss) nas 5 primeiras; mult = 3^máx(0, 5 − força).
pub fn penalidade(niveis: &[f64], nivel_boss: f64) -> (f64, f64) {
    let forca = niveis
        .iter()
        .take(TAMANHO_EQUIPE)
        .fold(0.0, |s, &l| s + (l / nivel_boss).min(1.0));
    (
        forca,
        js::pow(3.0, (TAMANHO_EQUIPE as f64 - forca).max(0.0)),
    )
}

/// Penalidade usada só para ranquear cartas soltas: como se a equipe toda tivesse o nível
/// da carta (`multIndividual`).
pub fn penalidade_individual(nivel: f64, nivel_boss: f64) -> f64 {
    js::pow(
        3.0,
        TAMANHO_EQUIPE as f64 * (1.0 - (nivel / nivel_boss).min(1.0)).max(0.0),
    )
}
