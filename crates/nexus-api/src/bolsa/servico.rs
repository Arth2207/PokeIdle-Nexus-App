//! Snapshot do mercado, sempre pronto.
//!
//! No legado a primeira requisição depois de 3 minutos espera o mercado ser remontado, com o
//! event loop travado. Aqui uma task em segundo plano remonta o mercado a cada ciclo e troca
//! o snapshot de uma vez (`ArcSwap`): quem lê pega sempre um mercado inteiro, sem esperar.
//! O cálculo roda em `spawn_blocking`, fora das threads que atendem requisições.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use arc_swap::ArcSwapOption;
use moka::sync::Cache;
use nucleo::bolsa::carregador::Carregador;
use nucleo::bolsa::mercado::{Mercado, OFERTA_FRESCA_MS, OFERTA_FRESCA_POKEMON_MS};
use nucleo::bolsa::ofertas::{self, ValorOferta};
use nucleo::bolsa::repo::{ErroRepo, FiltroOfertas, RepoMercado};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use super::agora_ms;

/// Mercado montado + valor justo das ofertas de pokémon ativas.
pub struct Snapshot {
    pub mercado: Mercado,
    pub valores: HashMap<i64, ValorOferta>,
    pub montado: Instant,
}

#[derive(Debug, thiserror::Error)]
pub enum ErroBolsa {
    #[error(transparent)]
    Repo(#[from] ErroRepo),
    #[error("a montagem do mercado foi interrompida")]
    Interrompida,
}

/// Lista de feedback já filtrada e ordenada, guardada por filtro.
pub struct Feedback {
    pub gerado_em: i64,
    pub indices: Vec<usize>,
    pub contagem: std::collections::BTreeMap<String, u64>,
}

/// Genérico em `R`: em produção é o repositório Postgres; nos testes, um em memória.
pub struct ServicoBolsa<R> {
    pub repo: R,
    atual: ArcSwapOption<Snapshot>,
    /// Um único montador por vez. Quem chega com o snapshot vazio espera o mesmo resultado.
    carregador: Mutex<Carregador>,
    /// Cache LRU do feedback (40 filtros), invalidado quando o mercado é remontado.
    pub feedback: Cache<String, Arc<Feedback>>,
}

const LIMITE_OFERTAS: i64 = 6000;

impl<R: RepoMercado + 'static> ServicoBolsa<R> {
    pub fn novo(repo: R) -> Self {
        Self {
            repo,
            atual: ArcSwapOption::empty(),
            carregador: Mutex::new(Carregador::default()),
            feedback: Cache::new(40),
        }
    }

    /// O snapshot atual. Só espera se ainda não houver nenhum (logo após subir).
    pub async fn snapshot(&self) -> Result<Arc<Snapshot>, Arc<ErroBolsa>> {
        if let Some(s) = self.atual.load_full() {
            return Ok(s);
        }
        self.atualizar().await.map_err(Arc::new)
    }

    /// Monta um mercado novo e troca o snapshot.
    pub async fn atualizar(&self) -> Result<Arc<Snapshot>, ErroBolsa> {
        let pedido = Instant::now();
        let mut carregador = self.carregador.lock().await;
        // Se outra task montou enquanto esperávamos o lock, aproveita.
        if let Some(s) = self.atual.load_full().filter(|s| s.montado >= pedido) {
            return Ok(s);
        }
        let inicio = Instant::now();
        let agora = agora_ms();
        let fontes = carregador.fontes(&self.repo, agora).await?;
        let lidas = carregador.ultima_leitura;
        let em_memoria = carregador.vendas_em_memoria();
        drop(carregador);

        let ofertas = self
            .repo
            .ofertas_ativas(&FiltroOfertas {
                corte_item: agora - OFERTA_FRESCA_MS,
                corte_pokemon: agora - OFERTA_FRESCA_POKEMON_MS,
                tipo: None,
                moeda: None,
                limite: LIMITE_OFERTAS,
            })
            .await?;

        // Cálculo pesado fora do runtime async.
        let (mercado, valores) = tokio::task::spawn_blocking(move || {
            let m = Mercado::montar(&fontes, agora);
            let v = ofertas::precalcular(&m, &ofertas, agora);
            (m, v)
        })
        .await
        .map_err(|_| ErroBolsa::Interrompida)?;

        let duracao = inicio.elapsed();
        metrics::histogram!("nexus_bolsa_montagem_duracao_segundos").record(duracao.as_secs_f64());
        metrics::gauge!("nexus_bolsa_tickers").set(mercado.total_tickers() as f64);
        metrics::gauge!("nexus_bolsa_vendas_janela").set(mercado.total_vendas as f64);
        metrics::gauge!("nexus_bolsa_vendas_memoria").set(em_memoria as f64);
        metrics::counter!("nexus_bolsa_vendas_lidas_total").increment(lidas as u64);
        metrics::gauge!("nexus_bolsa_ofertas_avaliadas").set(valores.len() as f64);
        metrics::gauge!("nexus_bolsa_gerado_em_segundos").set(agora as f64 / 1000.0);
        tracing::info!(
            tickers = mercado.total_tickers(),
            vendas = mercado.total_vendas,
            lidas,
            ofertas_avaliadas = valores.len(),
            ms = duracao.as_millis(),
            "mercado montado"
        );

        let snapshot = Arc::new(Snapshot {
            mercado,
            valores,
            montado: Instant::now(),
        });
        self.atual.store(Some(Arc::clone(&snapshot)));
        self.feedback.invalidate_all();
        Ok(snapshot)
    }

    /// Monta um mercado "como se fosse" o instante `alvo`, com os dados atuais. Usado pelo
    /// comparador para alinhar com o instante em que o legado montou o dele.
    pub async fn snapshot_em(&self, alvo: i64) -> Result<Snapshot, ErroBolsa> {
        let atual = self.snapshot().await.map_err(|_| ErroBolsa::Interrompida)?;
        let fontes = self
            .carregador
            .lock()
            .await
            .fontes_com(&self.repo, agora_ms(), true)
            .await?;
        let valores = atual.valores.clone();
        let mercado = tokio::task::spawn_blocking(move || Mercado::montar(&fontes, alvo))
            .await
            .map_err(|_| ErroBolsa::Interrompida)?;
        Ok(Snapshot {
            mercado,
            valores,
            montado: Instant::now(),
        })
    }

    /// Remonta o mercado a cada `ciclo` até o processo pedir para parar.
    pub fn iniciar(self: Arc<Self>, ciclo: Duration, parar: CancellationToken) {
        tokio::spawn(async move {
            loop {
                if let Err(erro) = self.atualizar().await {
                    metrics::counter!("nexus_bolsa_montagem_falhas_total").increment(1);
                    tracing::warn!(%erro, "não consegui montar o mercado; tento no próximo ciclo");
                }
                tokio::select! {
                    () = parar.cancelled() => break,
                    () = tokio::time::sleep(ciclo) => {}
                }
            }
        });
    }
}
