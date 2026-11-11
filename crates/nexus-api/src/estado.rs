//! Estado compartilhado entre todas as rotas.

use std::sync::Arc;
use std::time::Duration;

use persistencia::PgRepoMercado;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

use crate::bolsa::ServicoBolsa;
use crate::configuracao::Configuracao;
use crate::equipes::bolsa::ServicoEquipeBolsa;
use crate::equipes::{ModeloAtivo, ServicoEquipes};
use crate::limite::Limitador;

/// Clonar é barato: tudo aqui é `Arc` (ou tem um por dentro, como o `PgPool`). Cada rota
/// recebe sua cópia, e todas apontam para os mesmos dados.
#[derive(Clone)]
pub struct Estado {
    pub db: PgPool,
    pub bolsa: Arc<ServicoBolsa<PgRepoMercado>>,
    pub escrita_bolsa: bool,
    pub equipes: Arc<ServicoEquipes>,
    /// Equipe da bolsa: jobs de cálculo e o pool de CPU.
    pub equipe_bolsa: Arc<ServicoEquipeBolsa>,
    pub escrita_equipes: bool,
    pub limitador: Arc<Limitador>,
    pub origens_cors: Arc<Vec<String>>,
    /// Serializa as gravações do inventário (ler, aplicar, gravar).
    pub meus_itens_lock: Arc<tokio::sync::Mutex<()>>,
}

impl Estado {
    /// Cria o pool sem conectar ainda (`connect_lazy`). Assim o processo sobe mesmo com o
    /// banco fora do ar: o `/health/live` responde e o `/health/ready` avisa que o banco caiu.
    pub fn novo(cfg: &Configuracao) -> Result<Self, sqlx::Error> {
        let db = PgPoolOptions::new()
            .max_connections(5)
            .acquire_timeout(Duration::from_secs(2))
            .connect_lazy(cfg.database_url.expor())?;
        Ok(Self {
            escrita_bolsa: cfg.escrita_bolsa,
            escrita_equipes: cfg.escrita_equipes,
            equipes: Arc::new(ServicoEquipes::novo(
                db.clone(),
                ModeloAtivo::de_texto(&cfg.modelo),
            )),
            equipe_bolsa: Arc::new(ServicoEquipeBolsa::novo(cfg.cpu_threads)),
            limitador: Arc::new(Limitador::novo(cfg.limite_rps, cfg.limite_rajada)),
            origens_cors: Arc::new(cfg.origens_cors()),
            ..Self::com_pool(db)
        })
    }

    /// Estado com valores padrão (escrita desligada, sem CORS).
    pub fn com_pool(db: PgPool) -> Self {
        let bolsa = Arc::new(ServicoBolsa::novo(PgRepoMercado::novo(db.clone())));
        Self {
            equipes: Arc::new(ServicoEquipes::novo(db.clone(), ModeloAtivo::Legado)),
            equipe_bolsa: Arc::new(ServicoEquipeBolsa::novo(1)),
            escrita_equipes: false,
            db,
            bolsa,
            escrita_bolsa: false,
            limitador: Arc::new(Limitador::novo(10.0, 40.0)),
            origens_cors: Arc::new(Vec::new()),
            meus_itens_lock: Arc::new(tokio::sync::Mutex::new(())),
        }
    }
}
