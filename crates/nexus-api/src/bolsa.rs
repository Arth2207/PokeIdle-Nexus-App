//! Serviço da bolsa: o mercado montado pelo `nucleo`, guardado num cache.

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use moka::future::Cache;
use nucleo::bolsa::mercado::{self, Mercado};
use nucleo::bolsa::repo::{ErroRepo, RepoMercado};

/// O legado remonta o mercado a cada 3 minutos. Mantemos o mesmo intervalo para as
/// respostas baterem na comparação com ele.
const TTL_MERCADO: Duration = Duration::from_secs(3 * 60);

/// Genérico em `R`: em produção é o repositório Postgres, em teste pode ser um em memória.
pub struct ServicoBolsa<R> {
    repo: R,
    /// `moka` é um cache em memória com expiração, seguro para várias tasks ao mesmo tempo.
    /// A chave é `()` porque só existe um mercado: o cache guarda no máximo um valor.
    cache: Cache<(), Arc<Mercado>>,
}

impl<R: RepoMercado> ServicoBolsa<R> {
    pub fn novo(repo: R) -> Self {
        let cache = Cache::builder()
            .max_capacity(1)
            .time_to_live(TTL_MERCADO)
            .build();
        Self { repo, cache }
    }

    /// O mercado atual, do cache ou montado agora.
    ///
    /// Se dez requisições chegarem com o cache vazio, `try_get_with` monta o mercado uma vez
    /// só e as outras nove esperam o mesmo resultado (no legado, a variável `building`).
    /// Se a montagem falhar, nada é guardado e a próxima requisição tenta de novo.
    pub async fn mercado(&self) -> Result<Arc<Mercado>, Arc<ErroRepo>> {
        self.cache
            .try_get_with((), async {
                let inicio = std::time::Instant::now();
                let m = mercado::carregar(&self.repo, agora_ms()).await?;
                tracing::info!(
                    tickers = m.total_tickers(),
                    ms = inicio.elapsed().as_millis(),
                    "mercado montado"
                );
                Ok(Arc::new(m))
            })
            .await
    }
}

fn agora_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}
