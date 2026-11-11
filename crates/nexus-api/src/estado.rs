//! Estado compartilhado entre todas as rotas.

use std::time::Duration;

use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

use crate::configuracao::Configuracao;

/// Clonar é barato: o `PgPool` é um ponteiro com contagem de referência (`Arc`) por dentro.
/// Cada rota recebe sua cópia, e todas apontam para o mesmo pool.
#[derive(Clone)]
pub struct Estado {
    pub db: PgPool,
}

impl Estado {
    /// Cria o pool sem conectar ainda (`connect_lazy`). Assim o processo sobe mesmo com o
    /// banco fora do ar: o `/health/live` responde e o `/health/ready` avisa que o banco caiu.
    pub fn novo(cfg: &Configuracao) -> Result<Self, sqlx::Error> {
        let db = PgPoolOptions::new()
            .max_connections(5)
            .acquire_timeout(Duration::from_secs(2))
            .connect_lazy(cfg.database_url.expor())?;
        Ok(Self { db })
    }
}
