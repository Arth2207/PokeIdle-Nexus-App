//! Implementações em Postgres das portas do `nucleo`.
//!
//! Durante a migração, as tabelas do schema `public` são do sistema legado e aqui só são
//! lidas. O Nexus escreve apenas no schema `nexus`, exceto pelas rotas de ingestão da bolsa,
//! que ficam desligadas em produção até o cutover.

pub mod bolsa_equipe;
pub mod equipes;
pub mod importacao;
pub mod ingestao;
pub mod mercado;
pub mod paridade;

pub use mercado::PgRepoMercado;
