//! Motor de combate: fórmulas das cartas, duelo contra o boss e os modelos de combate.
//!
//! Tudo aqui é cálculo puro. A pokédex e a lista de bosses chegam por parâmetro, e as
//! constantes do servidor ficam num [`tipos::Constantes`] em vez de variáveis globais, para
//! dois modelos (legado e jogo) poderem rodar lado a lado.

pub mod combatente;
pub mod duelo;
pub mod especie;
pub mod modelo;
pub mod tipos;
