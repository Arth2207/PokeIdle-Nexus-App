//! O trait [`ModeloCombate`] e o modelo [`Legado`], cópia exata do `sim.js` + `equipes-core.js`.
//!
//! O modelo decide como o boss é montado e como uma carta é avaliada contra ele. O `Legado`
//! responde até o cutover; o modelo `Jogo` (etapa 5) roda ao lado só nas métricas.

use super::combatente::{Combatente, Golpe, Opcoes};
use super::duelo::{avaliar, Avaliacao};
use super::especie::{Categoria, EspecieCombate, Pokedex};
use super::tipos::{Constantes, Tipo};

/// Boss como a lista do jogo (ou a reserva da wiki) o descreve.
#[derive(Debug, Clone, PartialEq)]
pub struct Boss {
    pub key: String,
    pub nome: String,
    /// Nível de equipe exigido (`teamLevel`): é o nível em que o boss luta.
    pub nivel_equipe: f64,
    /// Tipos informados pela lista; vazio usa os da espécie.
    pub tipos: Vec<Tipo>,
}

/// Um modelo de combate: monta o boss e avalia cartas contra ele.
///
/// `Send + Sync` deixa o mesmo modelo ser usado por várias threads ao mesmo tempo (o
/// otimizador roda em paralelo).
pub trait ModeloCombate: Send + Sync {
    /// Nome curto para logs e métricas.
    fn nome(&self) -> &'static str;

    fn constantes(&self) -> &Constantes;

    /// O boss como combatente, e a espécie dele. `None` se a espécie não está na pokédex.
    fn boss<'a>(&self, boss: &Boss, dex: &'a Pokedex) -> Option<(Combatente, &'a EspecieCombate)>;

    /// Pontuação de uma carta contra o boss, com a penalidade de equipe `mult`.
    fn avaliar(&self, carta: &Combatente, boss: &Combatente, mult: f64) -> Avaliacao {
        avaliar(carta, boss, mult, self.constantes())
    }
}

/// O modelo do legado. O boss é a espécie no nível de equipe (qualidade 1,3, potência 3,
/// IVs 31, lendário), com os dois golpes de maior poder da espécie, o mais forte no 1º tipo
/// do boss e o outro no 2º.
#[derive(Debug, Clone, Default)]
pub struct Legado {
    constantes: Constantes,
}

impl Legado {
    pub fn novo() -> Self {
        Legado {
            constantes: Constantes::wiki(),
        }
    }
}

impl ModeloCombate for Legado {
    fn nome(&self) -> &'static str {
        "legado"
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
        // Ordenação estável por poder, do maior para o menor, como o `sort` do JS.
        let mut fortes: Vec<_> = sp.golpes.iter().filter(|g| g.poder > 0.0).collect();
        fortes.sort_by(|a, b| b.poder.total_cmp(&a.poder));
        let primeiro_tipo = tipos.first().copied();
        let golpe = |nome: &str, poder, recarga, categoria, tipo: Option<Tipo>| {
            tipo.map(|tipo| Golpe {
                nome: nome.to_string(),
                poder,
                recarga,
                categoria,
                tipo,
            })
        };
        let forte = match fortes.first() {
            Some(g) => golpe(&g.nome, g.poder, g.recarga, g.categoria, primeiro_tipo),
            // Espécie sem golpes: um golpe genérico especial de poder 120.
            None => golpe("Golpe", 120.0, 6.0, Categoria::Especial, primeiro_tipo),
        };
        let segundo_tipo = tipos.get(1).copied().or(primeiro_tipo);
        let segundo = match fortes.get(1) {
            Some(g) => golpe(&g.nome, g.poder, g.recarga, g.categoria, segundo_tipo),
            None => forte.clone().map(|g| Golpe {
                tipo: segundo_tipo.unwrap_or(g.tipo),
                ..g
            }),
        };
        mon.golpes = forte.into_iter().chain(segundo).collect();
        mon.tipos = tipos;
        Some((mon, sp))
    }
}
