//! Regras de negócio do Nexus, sem I/O.
//!
//! Este crate não conhece Postgres, HTTP nem relógio. Quando precisa de dados, declara um
//! trait (veja [`bolsa::repo::RepoMercado`]) e quem chama entrega uma implementação.
//! Quando precisa da hora, recebe `agora` por parâmetro. Assim tudo aqui é testável sem
//! banco e com resultados reproduzíveis.

pub mod bolsa;
pub mod bolsa_equipe;
pub mod busca_exata;
pub mod combate;
pub mod equipes;
pub mod js;
pub mod otimizador;
pub mod precificador;
