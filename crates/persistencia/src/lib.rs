//! Implementações em Postgres das portas do `nucleo`.
//!
//! Durante a migração, as tabelas do schema `public` são do sistema legado e aqui só são
//! lidas. O Nexus escreve apenas no schema `nexus`.

pub mod mercado;

pub use mercado::PgRepoMercado;
