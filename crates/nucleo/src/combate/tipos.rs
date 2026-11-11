//! Os 18 tipos, a tabela de efetividade e as constantes de hunt do servidor.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// Tipo elementar, com o nome em inglês e maiúsculas que o jogo usa (`"FIRE"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Tipo {
    Normal,
    Fire,
    Water,
    Grass,
    Electric,
    Ice,
    Fighting,
    Poison,
    Ground,
    Flying,
    Psychic,
    Bug,
    Rock,
    Ghost,
    Dragon,
    Dark,
    Steel,
    Fairy,
}

impl Tipo {
    /// Todos, na ordem do legado (`TYPES` do `sim.js`).
    pub const TODOS: [Tipo; 18] = [
        Tipo::Normal,
        Tipo::Fire,
        Tipo::Water,
        Tipo::Grass,
        Tipo::Electric,
        Tipo::Ice,
        Tipo::Fighting,
        Tipo::Poison,
        Tipo::Ground,
        Tipo::Flying,
        Tipo::Psychic,
        Tipo::Bug,
        Tipo::Rock,
        Tipo::Ghost,
        Tipo::Dragon,
        Tipo::Dark,
        Tipo::Steel,
        Tipo::Fairy,
    ];

    pub fn nome(self) -> &'static str {
        match self {
            Tipo::Normal => "NORMAL",
            Tipo::Fire => "FIRE",
            Tipo::Water => "WATER",
            Tipo::Grass => "GRASS",
            Tipo::Electric => "ELECTRIC",
            Tipo::Ice => "ICE",
            Tipo::Fighting => "FIGHTING",
            Tipo::Poison => "POISON",
            Tipo::Ground => "GROUND",
            Tipo::Flying => "FLYING",
            Tipo::Psychic => "PSYCHIC",
            Tipo::Bug => "BUG",
            Tipo::Rock => "ROCK",
            Tipo::Ghost => "GHOST",
            Tipo::Dragon => "DRAGON",
            Tipo::Dark => "DARK",
            Tipo::Steel => "STEEL",
            Tipo::Fairy => "FAIRY",
        }
    }
}

impl fmt::Display for Tipo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.nome())
    }
}

/// Erro de um nome de tipo desconhecido.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("tipo desconhecido: {0}")]
pub struct TipoDesconhecido(pub String);

impl FromStr for Tipo {
    type Err = TipoDesconhecido;

    /// Aceita qualquer caixa (`"fire"`, `"Fire"`), como o `toUpperCase` do legado.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let maiusculo = s.to_uppercase();
        Tipo::TODOS
            .into_iter()
            .find(|t| t.nome() == maiusculo)
            .ok_or_else(|| TipoDesconhecido(s.to_string()))
    }
}

/// Multiplicador de um golpe do tipo `golpe` contra um defensor de um único tipo
/// (`CHART` do `sim.js`; o que não está na tabela vale 1).
pub fn efetividade(golpe: Tipo, defesa: Tipo) -> f64 {
    use Tipo::*;
    match (golpe, defesa) {
        (Normal, Rock | Steel) => 0.5,
        (Normal, Ghost) => 0.0,
        (Fire, Fire | Water | Rock | Dragon) => 0.5,
        (Fire, Grass | Ice | Bug | Steel) => 2.0,
        (Water, Fire | Ground | Rock) => 2.0,
        (Water, Water | Grass | Dragon) => 0.5,
        (Grass, Water | Ground | Rock) => 2.0,
        (Grass, Fire | Grass | Poison | Flying | Bug | Dragon | Steel) => 0.5,
        (Electric, Water | Flying) => 2.0,
        (Electric, Grass | Electric | Dragon) => 0.5,
        (Electric, Ground) => 0.0,
        (Ice, Grass | Ground | Flying | Dragon) => 2.0,
        (Ice, Fire | Water | Ice | Steel) => 0.5,
        (Fighting, Normal | Ice | Rock | Dark | Steel) => 2.0,
        (Fighting, Poison | Flying | Psychic | Bug | Fairy) => 0.5,
        (Fighting, Ghost) => 0.0,
        (Poison, Grass | Fairy) => 2.0,
        (Poison, Poison | Ground | Rock | Ghost) => 0.5,
        (Poison, Steel) => 0.0,
        (Ground, Fire | Electric | Poison | Rock | Steel) => 2.0,
        (Ground, Grass | Bug) => 0.5,
        (Ground, Flying) => 0.0,
        (Flying, Grass | Fighting | Bug) => 2.0,
        (Flying, Electric | Rock | Steel) => 0.5,
        (Psychic, Fighting | Poison) => 2.0,
        (Psychic, Psychic | Steel) => 0.5,
        (Psychic, Dark) => 0.0,
        (Bug, Grass | Psychic | Dark) => 2.0,
        (Bug, Fire | Fighting | Poison | Flying | Ghost | Steel | Fairy) => 0.5,
        (Rock, Fire | Ice | Flying | Bug) => 2.0,
        (Rock, Fighting | Ground | Steel) => 0.5,
        (Ghost, Psychic | Ghost) => 2.0,
        (Ghost, Dark) => 0.5,
        (Ghost, Normal) => 0.0,
        (Dragon, Dragon) => 2.0,
        (Dragon, Steel) => 0.5,
        (Dragon, Fairy) => 0.0,
        (Dark, Psychic | Ghost) => 2.0,
        (Dark, Fighting | Dark | Fairy) => 0.5,
        (Steel, Ice | Rock | Fairy) => 2.0,
        (Steel, Fire | Water | Electric | Steel) => 0.5,
        (Fairy, Fighting | Dragon | Dark) => 2.0,
        (Fairy, Fire | Poison | Steel) => 0.5,
        _ => 1.0,
    }
}

/// Produto da efetividade contra todos os tipos do defensor (`effRaw`).
pub fn efetividade_bruta(golpe: Tipo, defesa: &[Tipo]) -> f64 {
    defesa.iter().fold(1.0, |m, &d| m * efetividade(golpe, d))
}

/// Constantes de hunt que o servidor manda no `welcome`. O legado usa sempre as da wiki.
#[derive(Debug, Clone, PartialEq)]
pub struct Constantes {
    /// HP do selvagem × este valor.
    pub hp_selvagem: f64,
    /// Dano do selvagem × este valor.
    pub dano_selvagem: f64,
    /// Na hunt, a efetividade bruta (chave) vira outra (valor).
    pub ampliacao_hunt: Vec<(f64, f64)>,
}

impl Constantes {
    /// Valores da wiki (os do `sim.js`).
    pub fn wiki() -> Self {
        Constantes {
            hp_selvagem: 5.0,
            dano_selvagem: 1.8,
            ampliacao_hunt: vec![
                (0.0, 0.0),
                (0.25, 0.167),
                (0.5, 0.333),
                (1.0, 1.0),
                (2.0, 2.5),
                (4.0, 5.5),
            ],
        }
    }

    /// Efetividade ampliada da hunt (`effHunt`): valores fora da tabela passam iguais.
    pub fn efetividade_hunt(&self, golpe: Tipo, defesa: &[Tipo]) -> f64 {
        let bruta = efetividade_bruta(golpe, defesa);
        self.ampliacao_hunt
            .iter()
            .find(|(k, _)| *k == bruta)
            .map_or(bruta, |(_, v)| *v)
    }
}

impl Default for Constantes {
    fn default() -> Self {
        Constantes::wiki()
    }
}
