//! Equipes no `nexus-api`: guarda em memória a pokédex do motor e o estado de equipes, lidos
//! do Postgres, e calcula as respostas fora das threads do tokio.

use std::sync::Arc;
use std::time::{Duration, Instant};

use nucleo::combate::especie::Pokedex;
use nucleo::equipes::{EstadoEquipes, MotorEquipes};
use sqlx::PgPool;
use tokio::sync::Mutex;

/// A pokédex muda pouco: relê a cada 30 minutos, como os catálogos da bolsa.
const RECARGA_MOTOR: Duration = Duration::from_secs(30 * 60);
/// O estado chega pelo relay (ou pela importação): relê a cada 30 segundos.
const RECARGA_ESTADO: Duration = Duration::from_secs(30);

#[derive(Debug, thiserror::Error)]
pub enum ErroServico {
    #[error("banco indisponível: {0}")]
    Banco(#[from] sqlx::Error),
    #[error("pokédex do motor inválida: {0}")]
    Pokedex(#[from] nucleo::combate::especie::ErroPokedex),
    #[error("pokédex do motor ainda não importada")]
    PokedexVazia,
}

/// Valor em memória com o instante em que foi lido.
type Cache<T> = Mutex<Option<(Instant, Arc<T>)>>;

pub struct ServicoEquipes {
    db: PgPool,
    motor: Cache<MotorEquipes>,
    estado: Cache<EstadoEquipes>,
    /// Serializa os ingests (ler, aplicar, gravar).
    pub gravacao: Mutex<()>,
}

impl ServicoEquipes {
    pub fn novo(db: PgPool) -> Self {
        ServicoEquipes {
            db,
            motor: Mutex::new(None),
            estado: Mutex::new(None),
            gravacao: Mutex::new(()),
        }
    }

    pub async fn motor(&self) -> Result<Arc<MotorEquipes>, ErroServico> {
        let mut c = self.motor.lock().await;
        if let Some((_, m)) = c.as_ref().filter(|(em, _)| em.elapsed() < RECARGA_MOTOR) {
            return Ok(m.clone());
        }
        let valor = persistencia::equipes::carregar_pokedex(&self.db).await?;
        // Montar os índices da pokédex é CPU pura: sai da thread do tokio.
        let dex = tokio::task::spawn_blocking(move || Pokedex::de_valor(valor))
            .await
            .map_err(|_| ErroServico::PokedexVazia)??;
        if dex.especies().is_empty() {
            return Err(ErroServico::PokedexVazia);
        }
        let m = Arc::new(MotorEquipes::novo(dex));
        *c = Some((Instant::now(), m.clone()));
        Ok(m)
    }

    pub async fn estado(&self) -> Result<Arc<EstadoEquipes>, ErroServico> {
        let mut c = self.estado.lock().await;
        if let Some((_, e)) = c.as_ref().filter(|(em, _)| em.elapsed() < RECARGA_ESTADO) {
            return Ok(e.clone());
        }
        let e = Arc::new(persistencia::equipes::carregar_estado(&self.db).await?);
        *c = Some((Instant::now(), e.clone()));
        Ok(e)
    }

    /// Grava um ingest e troca o estado em memória.
    pub async fn gravar(&self, e: EstadoEquipes, nick: &str) -> Result<(), ErroServico> {
        persistencia::equipes::gravar_ingest(&self.db, &e, nick).await?;
        *self.estado.lock().await = Some((Instant::now(), Arc::new(e)));
        Ok(())
    }
}

pub mod bolsa;
