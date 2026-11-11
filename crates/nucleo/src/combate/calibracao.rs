//! Calibração do modelo `Jogo` com tentativas reais de boss.
//!
//! Para cada tentativa, o modelo prevê o dano que a equipe causa e recebe. Os parâmetros
//! desconhecidos saem por mínimos quadrados:
//!
//! - `mult_hp`: nas vitórias, o dano causado até o fim da luta é o HP real do boss. Ajuste
//!   pela origem `dano = k × HP do modelo`, `k = Σxy / Σx²`.
//! - `base_penalidade` e `mult_dano`: o dano recebido observado contra o previsto com a
//!   penalidade `base^(5 − força)`. Para cada base de uma grade (1,00 a 6,00), `mult_dano`
//!   tem solução fechada; fica a base com o menor erro quadrático no logaritmo. Sem variação
//!   de força entre as tentativas, a base não é identificável e fica a da wiki (3).

use serde_json::Value;

use super::combatente;
use super::especie::Pokedex;
use super::jogo::{Jogo, ParametrosBoss};
use super::modelo::{Boss, ModeloCombate};

/// Mínimo de tentativas para calibrar.
pub const MINIMO_TENTATIVAS: usize = 5;

/// Uma tentativa de boss gravada.
#[derive(Debug, Clone, PartialEq)]
pub struct Tentativa {
    pub boss: Boss,
    /// Fichas completas das cartas da equipe.
    pub fichas: Vec<Value>,
    pub venceu: bool,
    pub duracao_s: f64,
    /// Dano total causado, quando os eventos `batalha` foram capturados.
    pub dano_causado: Option<f64>,
    /// Dano total recebido, idem.
    pub dano_recebido: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Calibracao {
    pub parametros: ParametrosBoss,
    pub tentativas: usize,
    /// Erro quadrático médio do dano recebido, no logaritmo (0 = previsão exata).
    pub erro_rms_log: f64,
}

/// Previsão do modelo para uma tentativa, com os parâmetros neutros.
struct Previsao {
    hp_boss: f64,
    dps_causado: f64,
    dps_recebido: f64,
    deficit: f64,
}

fn prever(dex: &Pokedex, t: &Tentativa) -> Option<Previsao> {
    let modelo = Jogo::novo(ParametrosBoss {
        mult_hp: 1.0,
        mult_dano: 1.0,
        base_penalidade: 1.0,
    });
    let k = modelo.constantes();
    let (boss, _) = modelo.boss(&t.boss, dex)?;
    let mut niveis = Vec::new();
    let (mut causado, mut recebido) = (0.0, 0.0);
    for f in &t.fichas {
        let (mon, _) = combatente::de_ficha(dex, f, k)?;
        let r = modelo.avaliar(&mon, &boss, 1.0);
        causado += r.dps_causado;
        recebido += r.dps_recebido;
        niveis.push(mon.nivel);
    }
    let forca = modelo.penalidade(&niveis, t.boss.nivel_equipe).0;
    Some(Previsao {
        hp_boss: boss.hp,
        dps_causado: causado,
        dps_recebido: recebido,
        deficit: (5.0 - forca).max(0.0),
    })
}

/// Ajusta os parâmetros do boss. `None` com menos de [`MINIMO_TENTATIVAS`] tentativas
/// utilizáveis.
pub fn calibrar(dex: &Pokedex, tentativas: &[Tentativa]) -> Option<Calibracao> {
    let dados: Vec<(&Tentativa, Previsao)> = tentativas
        .iter()
        .filter_map(|t| Some((t, prever(dex, t)?)))
        .filter(|(t, _)| t.duracao_s > 0.0)
        .collect();
    if dados.len() < MINIMO_TENTATIVAS {
        return None;
    }
    // HP: só as vitórias.
    let (mut sxy, mut sxx) = (0.0, 0.0);
    for (t, p) in dados.iter().filter(|(t, _)| t.venceu) {
        let y = t.dano_causado.unwrap_or(p.dps_causado * t.duracao_s);
        sxy += p.hp_boss * y;
        sxx += p.hp_boss * p.hp_boss;
    }
    let mult_hp = if sxx > 0.0 { sxy / sxx } else { 1.0 };

    // Dano recebido: grade na base, solução fechada no multiplicador.
    let obs: Vec<(f64, f64, f64)> = dados
        .iter()
        .filter_map(|(t, p)| {
            let o = t.dano_recebido.filter(|o| *o > 0.0)?;
            Some((o, p.dps_recebido * t.duracao_s, p.deficit))
        })
        .filter(|(_, x, _)| *x > 0.0)
        .collect();
    let ajuste = |base: f64| -> (f64, f64) {
        let pred: Vec<f64> = obs.iter().map(|(_, x, d)| x * base.powf(*d)).collect();
        let (num, den) = obs
            .iter()
            .zip(&pred)
            .fold((0.0, 0.0), |(n, d), ((o, _, _), p)| (n + p * o, d + p * p));
        let k = if den > 0.0 { num / den } else { 1.0 };
        let sse = obs
            .iter()
            .zip(&pred)
            .map(|((o, _, _), p)| (o.ln() - (k * p).ln()).powi(2))
            .sum::<f64>();
        (k, sse)
    };
    let varia = obs
        .iter()
        .any(|(_, _, d)| (d - obs.first().map_or(0.0, |x| x.2)).abs() > 1e-9);
    let (base, mult_dano, sse) = if obs.is_empty() {
        (3.0, 1.0, 0.0)
    } else if !varia {
        let (k, sse) = ajuste(3.0);
        (3.0, k, sse)
    } else {
        (100..=600)
            .map(|i| {
                let b = f64::from(i) / 100.0;
                let (k, sse) = ajuste(b);
                (b, k, sse)
            })
            .fold((3.0, 1.0, f64::INFINITY), |m, x| {
                if x.2 < m.2 - 1e-12 {
                    x
                } else {
                    m
                }
            })
    };
    Some(Calibracao {
        parametros: ParametrosBoss {
            mult_hp,
            mult_dano,
            base_penalidade: base,
        },
        tentativas: dados.len(),
        erro_rms_log: if obs.is_empty() {
            0.0
        } else {
            (sse / obs.len() as f64).sqrt()
        },
    })
}

#[cfg(test)]
mod testes {
    use super::*;
    use crate::combate::tipos::Tipo;
    use serde_json::json;

    fn dex() -> Pokedex {
        Pokedex::de_valor(json!([
            {"d": 144, "n": "Articuno", "t": ["ICE", "FLYING"], "s": [90, 85, 100, 95, 125, 85], "h": null, "c": "arena_mistica",
             "m": [["Ice Beam", 90, 27.6, "E", "ICE"]]},
            {"d": 6, "n": "Charizard", "t": ["FIRE", "FLYING"], "s": [78, 84, 78, 109, 85, 100], "h": 100, "c": "pokebola",
             "m": [["Flamethrower", 90, 27.6, "E", "FIRE"], ["Air Slash", 75, 24.1, "E", "FLYING"]]}
        ]))
        .unwrap()
    }

    /// Gera tentativas com parâmetros conhecidos e confere que a calibração os recupera.
    #[test]
    fn recupera_parametros_conhecidos() {
        let dex = dex();
        let reais = ParametrosBoss {
            mult_hp: 2.5,
            mult_dano: 1.7,
            base_penalidade: 2.2,
        };
        let boss = Boss {
            key: "articuno".into(),
            nome: "Articuno".into(),
            nivel_equipe: 300.0,
            tipos: vec![Tipo::Ice, Tipo::Flying],
        };
        let mut tentativas = Vec::new();
        for (i, nivel) in [300.0, 250.0, 200.0, 320.0, 150.0, 280.0]
            .into_iter()
            .enumerate()
        {
            let fichas: Vec<Value> = (0..5).map(|j| json!({"speciesId": 6, "level": nivel - f64::from(j) * 5.0, "quality": 1.3, "potencia": 3})).collect();
            let t = Tentativa {
                boss: boss.clone(),
                fichas,
                venceu: i % 2 == 0,
                duracao_s: 60.0 + i as f64,
                dano_causado: None,
                dano_recebido: None,
            };
            let p = prever(&dex, &t).unwrap();
            let t = Tentativa {
                dano_causado: Some(p.hp_boss * reais.mult_hp),
                dano_recebido: Some(
                    p.dps_recebido
                        * t.duracao_s
                        * reais.mult_dano
                        * reais.base_penalidade.powf(p.deficit),
                ),
                ..t
            };
            tentativas.push(t);
        }
        let c = calibrar(&dex, &tentativas).unwrap();
        assert!((c.parametros.mult_hp - 2.5).abs() < 1e-9, "{c:?}");
        assert!((c.parametros.base_penalidade - 2.2).abs() < 1e-9, "{c:?}");
        assert!((c.parametros.mult_dano - 1.7).abs() < 1e-6, "{c:?}");
        assert!(c.erro_rms_log < 1e-9);
    }

    #[test]
    fn poucas_tentativas_nao_calibram() {
        assert!(calibrar(&dex(), &[]).is_none());
    }
}
