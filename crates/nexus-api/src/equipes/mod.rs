//! Equipes no `nexus-api`: guarda em memória a pokédex do motor e o estado de equipes, lidos
//! do Postgres, e calcula as respostas fora das threads do tokio.

use std::sync::Arc;
use std::time::{Duration, Instant};

use nucleo::combate::especie::Pokedex;
use nucleo::combate::jogo::{Jogo, ParametrosBoss};
use nucleo::combate::modelo::{Legado, ModeloCombate};
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

/// Chave em `nexus.parametro` dos parâmetros calibrados do modelo do jogo.
pub const CHAVE_PARAMETROS_JOGO: &str = "modelo.jogo.parametros";

/// Qual modelo de combate responde nas rotas (`NEXUS_MODELO`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModeloAtivo {
    Legado,
    Jogo,
}

impl ModeloAtivo {
    pub fn de_texto(t: &str) -> Self {
        if t.eq_ignore_ascii_case("jogo") {
            ModeloAtivo::Jogo
        } else {
            ModeloAtivo::Legado
        }
    }
}

/// Chave em `nexus.parametro` das constantes do servidor (do `welcome`).
pub const CHAVE_CONSTANTES_SERVIDOR: &str = "servidor.constantes";

/// Constantes com a tabela de tipos do servidor, se já foi importada.
pub async fn constantes_do_servidor(db: &PgPool) -> Option<nucleo::combate::tipos::Constantes> {
    let v = persistencia::equipes::ler_parametro(db, CHAVE_CONSTANTES_SERVIDOR)
        .await
        .ok()
        .flatten()?;
    nucleo::combate::tipos::Constantes::com_tabela_do_servidor(v.get("tabelaTipos")?)
}

/// Parâmetros do modelo do jogo gravados pela calibração (ou os neutros).
pub async fn parametros_jogo(db: &PgPool) -> ParametrosBoss {
    let v = persistencia::equipes::ler_parametro(db, CHAVE_PARAMETROS_JOGO)
        .await
        .ok()
        .flatten();
    let f = |c: &str, p: f64| {
        v.as_ref()
            .and_then(|v| v.get(c))
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(p)
    };
    let d = ParametrosBoss::default();
    ParametrosBoss {
        mult_hp: f("multHp", d.mult_hp),
        mult_dano: f("multDano", d.mult_dano),
        base_penalidade: f("basePenalidade", d.base_penalidade),
    }
}

pub struct ServicoEquipes {
    db: PgPool,
    pub ativo: ModeloAtivo,
    /// `NEXUS_MELHORIAS`: o modelo usa a tabela de tipos do servidor.
    pub melhorias: bool,
    motor: Cache<MotorEquipes>,
    estado: Cache<EstadoEquipes>,
    /// Serializa os ingests (ler, aplicar, gravar).
    pub gravacao: Mutex<()>,
}

impl ServicoEquipes {
    pub fn novo(db: PgPool, ativo: ModeloAtivo, melhorias: bool) -> Self {
        ServicoEquipes {
            db,
            ativo,
            melhorias,
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
        let servidor = constantes_do_servidor(&self.db).await;
        let modelo: Arc<dyn ModeloCombate> = match (self.ativo, servidor) {
            (ModeloAtivo::Legado, Some(k)) if self.melhorias => Arc::new(Legado::com_constantes(k)),
            (ModeloAtivo::Legado, _) => Arc::new(Legado::novo()),
            (ModeloAtivo::Jogo, Some(k)) => {
                Arc::new(Jogo::com_constantes(parametros_jogo(&self.db).await, k))
            }
            (ModeloAtivo::Jogo, None) => Arc::new(Jogo::novo(parametros_jogo(&self.db).await)),
        };
        let m = Arc::new(MotorEquipes::com_modelo(dex, modelo));
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

    /// O estado lido agora do banco (sem o cache de 30 s), para quem vai alterá-lo.
    pub async fn estado_fresco(&self) -> Result<Arc<EstadoEquipes>, ErroServico> {
        let mut c = self.estado.lock().await;
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

/// Tarefa periódica do modelo do jogo: calibra com as tentativas gravadas (quando há o
/// bastante), grava os parâmetros e publica nas métricas quanto o modelo do jogo diverge do
/// legado em cada boss. Só métricas: as respostas seguem o modelo ativo.
pub fn iniciar_modelos(
    estado: crate::estado::Estado,
    ciclo: std::time::Duration,
    parar: tokio_util::sync::CancellationToken,
) {
    tokio::spawn(async move {
        loop {
            if let Err(e) = rodada_modelos(&estado).await {
                tracing::warn!(erro = %e, "comparação de modelos falhou");
            }
            tokio::select! {
                () = parar.cancelled() => break,
                () = tokio::time::sleep(ciclo) => {}
            }
        }
    });
}

async fn rodada_modelos(estado: &crate::estado::Estado) -> Result<(), String> {
    use nucleo::combate::calibracao::{calibrar, Tentativa};
    use nucleo::combate::modelo::Boss;

    let motor = estado.equipes.motor().await.map_err(|e| e.to_string())?;
    let est = estado.equipes.estado().await.map_err(|e| e.to_string())?;
    let linhas = persistencia::equipes::carregar_tentativas(&estado.db, 2000)
        .await
        .map_err(|e| e.to_string())?;
    metrics::gauge!("nexus_boss_tentativas").set(linhas.len() as f64);
    let tentativas: Vec<Tentativa> = linhas
        .into_iter()
        .map(|l| Tentativa {
            boss: Boss {
                key: l.boss.clone(),
                nome: l.boss,
                nivel_equipe: l.nivel_boss,
                tipos: l.tipos_boss.iter().filter_map(|t| t.parse().ok()).collect(),
            },
            fichas: l.equipe.as_array().cloned().unwrap_or_default(),
            venceu: l.venceu,
            duracao_s: l.duracao_s,
            dano_causado: l.dano_causado,
            dano_recebido: l.dano_recebido,
        })
        .collect();
    let dex = motor.dex.clone();
    let servidor = constantes_do_servidor(&estado.db).await;
    let (calibrado, divergencias) = estado
        .equipe_bolsa
        .no_pool(move || {
            let calibrado = calibrar(&dex, &tentativas);
            let p = calibrado.as_ref().map(|c| c.parametros).unwrap_or_default();
            let legado = MotorEquipes::com_modelo(dex.clone(), Arc::new(Legado::novo()));
            let jogo: Arc<dyn ModeloCombate> = match servidor {
                Some(k) => Arc::new(Jogo::com_constantes(p, k)),
                None => Arc::new(Jogo::novo(p)),
            };
            let jogo = MotorEquipes::com_modelo(dex, jogo);
            let lista = legado.lista_bosses(&est.bosses);
            (
                calibrado,
                nucleo::equipes::comparar_modelos(&legado, &jogo, &lista),
            )
        })
        .await
        .map_err(|e| e.to_string())?;
    if let Some(c) = calibrado {
        let p = c.parametros;
        metrics::gauge!("nexus_modelo_jogo_parametro", "parametro" => "mult_hp").set(p.mult_hp);
        metrics::gauge!("nexus_modelo_jogo_parametro", "parametro" => "mult_dano").set(p.mult_dano);
        metrics::gauge!("nexus_modelo_jogo_parametro", "parametro" => "base_penalidade")
            .set(p.base_penalidade);
        metrics::gauge!("nexus_modelo_jogo_erro_rms_log").set(c.erro_rms_log);
        let valor = serde_json::json!({
            "multHp": p.mult_hp, "multDano": p.mult_dano, "basePenalidade": p.base_penalidade,
            "tentativas": c.tentativas, "erroRmsLog": c.erro_rms_log,
        });
        if estado.escrita_equipes {
            persistencia::equipes::gravar_parametro(&estado.db, CHAVE_PARAMETROS_JOGO, &valor)
                .await
                .map_err(|e| e.to_string())?;
        }
    }
    for d in divergencias {
        metrics::gauge!("nexus_modelo_cobertura", "boss" => d.boss.clone(), "modelo" => "legado")
            .set(d.cobertura_a);
        metrics::gauge!("nexus_modelo_cobertura", "boss" => d.boss.clone(), "modelo" => "jogo")
            .set(d.cobertura_b);
        metrics::gauge!("nexus_modelo_especies_em_comum", "boss" => d.boss)
            .set(d.coincidentes as f64);
    }
    Ok(())
}
