//! Modelo `Jogo`: o boss montado pelo que o código público do jogo mostra, com o que é
//! desconhecido (HP e dano reais do boss, base da penalidade de equipe) como parâmetros
//! calibráveis com lutas reais.
//!
//! O que é público (`bosses-lendarios.mjs`): todo boss usa os golpes padrão *Legendary
//! Strike* (poder 280, recarga 10 s) e *Legendary Burst* (poder 400, recarga 15 s), ambos
//! especiais, com recarga fixa. O módulo não diz o tipo dos golpes; a hipótese aqui é o 1º
//! tipo do boss no Strike e o 2º (ou o 1º) no Burst, como o legado faz com os golpes que
//! inventa. A penalidade `3^(5 − força)` vem da wiki e não aparece no código público, por
//! isso a base também é um parâmetro.

use super::combatente::{Combatente, Golpe, Opcoes};
use super::duelo::{avaliar, Avaliacao, TAMANHO_EQUIPE};
use super::especie::{Categoria, EspecieCombate, Pokedex};
use super::modelo::{Boss, ModeloCombate};
use super::tipos::Constantes;
use crate::js;

/// Parâmetros desconhecidos do boss, a calibrar com lutas reais.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParametrosBoss {
    /// HP real do boss ÷ HP do modelo.
    pub mult_hp: f64,
    /// Dano real recebido ÷ dano do modelo.
    pub mult_dano: f64,
    /// Base da penalidade de equipe (`base^(5 − força)`); a wiki diz 3.
    pub base_penalidade: f64,
}

impl Default for ParametrosBoss {
    fn default() -> Self {
        ParametrosBoss {
            mult_hp: 1.0,
            mult_dano: 1.0,
            base_penalidade: 3.0,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Jogo {
    constantes: Constantes,
    pub parametros: ParametrosBoss,
}

impl Jogo {
    pub fn novo(parametros: ParametrosBoss) -> Self {
        Jogo {
            constantes: Constantes::wiki(),
            parametros,
        }
    }
}

impl ModeloCombate for Jogo {
    fn nome(&self) -> &'static str {
        "jogo"
    }

    fn constantes(&self) -> &Constantes {
        &self.constantes
    }

    fn boss<'a>(&self, boss: &Boss, dex: &'a Pokedex) -> Option<(Combatente, &'a EspecieCombate)> {
        let sp = dex.por_nome(&boss.nome)?;
        let op = Opcoes {
            qualidade: 1.3,
            ivs: [31.0; 6],
            potencia: 3,
            lendario: Some(true),
            ..Opcoes::default()
        };
        let mut mon = Combatente::novo(sp, boss.nivel_equipe, &op, &self.constantes);
        let tipos = if boss.tipos.is_empty() {
            sp.tipos.clone()
        } else {
            boss.tipos.clone()
        };
        let t1 = *tipos.first()?;
        let t2 = tipos.get(1).copied().unwrap_or(t1);
        let golpe = |nome: &str, poder: f64, recarga: f64, tipo| Golpe {
            nome: nome.to_string(),
            poder,
            recarga,
            categoria: Categoria::Especial,
            tipo,
        };
        mon.golpes = vec![
            golpe("Legendary Strike", 280.0, 10.0, t1),
            golpe("Legendary Burst", 400.0, 15.0, t2),
        ];
        // Recarga fixa (cooldownMs do jogo), sem o desconto pelo speed das cartas.
        mon.mult_recarga = 1.0;
        mon.hp *= self.parametros.mult_hp;
        mon.tipos = tipos;
        Some((mon, sp))
    }

    fn avaliar(&self, carta: &Combatente, boss: &Combatente, mult: f64) -> Avaliacao {
        avaliar(
            carta,
            boss,
            mult * self.parametros.mult_dano,
            &self.constantes,
        )
    }

    fn penalidade(&self, niveis: &[f64], nivel_boss: f64) -> (f64, f64) {
        let forca = niveis
            .iter()
            .take(TAMANHO_EQUIPE)
            .fold(0.0, |s, &l| s + (l / nivel_boss).min(1.0));
        let deficit = (TAMANHO_EQUIPE as f64 - forca).max(0.0);
        (forca, js::pow(self.parametros.base_penalidade, deficit))
    }

    fn penalidade_individual(&self, nivel: f64, nivel_boss: f64) -> f64 {
        let deficit = TAMANHO_EQUIPE as f64 * (1.0 - (nivel / nivel_boss).min(1.0)).max(0.0);
        js::pow(self.parametros.base_penalidade, deficit)
    }
}
