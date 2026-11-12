//! Precificador de pokémon: o mesmo modelo do legado (`engine/precificador`), em Rust.
//!
//! O preço de uma carta depende do poder PvE, do nível, da nota e da família. A partir das
//! vendas reais, o precificador:
//! - decompõe cada venda em "preço base" + adicionais (Z-Move, AoE, Mega), cotados em itens;
//! - ajusta uma curva de mercado (`ajuste`), usada para normalizar vendas de cartas diferentes
//!   para uma carta típica da espécie (é isso que a bolsa mostra);
//! - estima o valor justo de uma carta pelos vizinhos mais parecidos (`vizinhos`).

pub mod ajuste;
pub mod cotacao;
pub mod especie;
pub mod ficha;
pub mod motor;
pub mod nota;
pub mod vizinhos;

/// Ids dos itens usados como adicionais.
pub const Z_FRAGMENTO: i32 = 59194;
pub const AOE_FRAGMENTO: i32 = 40530;
pub const MEGA: i32 = 70014;
pub const MEGA_SHINY: i32 = 70015;
/// Fragmentos por adicional.
pub const POR_ADICIONAL: f64 = 10.0;
pub const DIA_MS: i64 = 86_400_000;
