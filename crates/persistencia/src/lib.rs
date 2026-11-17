//! Implementações em Postgres das portas do `nucleo`.
//!
//! Durante a migração, as tabelas do schema `public` são do sistema legado e aqui só são
//! lidas. O Nexus escreve apenas no schema `nexus`, exceto pelas rotas de ingestão da bolsa,
//! que ficam desligadas em produção até o cutover.

pub mod bolsa_equipe;
pub mod cacada;
pub mod cacada_contexto;
pub mod comandos;
pub mod equipes;
pub mod eventos;
pub mod importacao;
pub mod ingestao;
pub mod mercado;
pub mod motor;
pub mod outlands;
pub mod paridade;
pub mod pvp;
pub mod raros;

pub use mercado::PgRepoMercado;
