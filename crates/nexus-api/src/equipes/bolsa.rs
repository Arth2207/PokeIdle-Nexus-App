//! Equipe da bolsa (`/api/bolsa/equipe`, `/equipe-propria`, `/alvos-coleta`) no `nexus-api`.
//!
//! O cálculo é CPU pura e roda num pool `rayon` de tamanho fixo (`NEXUS_CPU_THREADS`), fora
//! das threads do tokio: a API continua respondendo enquanto ele roda. A ponte entre os dois
//! mundos é um canal `oneshot`: a tarefa do tokio entrega o trabalho ao pool e espera a
//! resposta pelo canal, sem bloquear a thread.
//!
//! Cada pedido vira um "job" com chave (boss, moeda, nick, idade, n), como no legado: pedidos
//! iguais esperam o mesmo job; um resultado vale 5 minutos; enquanto o job roda, a rota
//! responde `202` com o resultado anterior. Quando o painel troca de boss, o job antigo da
//! mesma conta é cancelado (cancelamento cooperativo: ele confere o sinal entre as etapas).

use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};

use nucleo::bolsa::mercado::OFERTA_FRESCA_POKEMON_MS;
use nucleo::bolsa::ofertas;
use nucleo::bolsa::repo::{FiltroOfertas, OfertaBruta, RepoMercado};
use nucleo::bolsa_equipe::{self as be, BossWiki, EntradaBolsa, EntradaPropria, OfertaEquipe};
use nucleo::equipes::EstadoEquipes;
use serde_json::{json, Value};
use tokio::sync::{Mutex, Notify};
use tokio_util::sync::CancellationToken;

use crate::bolsa::agora_ms;
use crate::bolsa::servico::Snapshot;
use crate::estado::Estado;

const VALIDADE_JOB: Duration = Duration::from_secs(5 * 60);
const RECARGA_WIKI: Duration = Duration::from_secs(30 * 60);
const RECARGA_PRECOS: Duration = Duration::from_secs(30 * 60);
const VALIDADE_PROPRIA: Duration = Duration::from_secs(60);
const MAX_JOBS: usize = 40;
/// Cálculos de equipe rodando ao mesmo tempo, no máximo.
const MAX_ATIVOS: usize = 8;
/// Um cálculo que passa disso é dado como falho.
const LIMITE_CALCULO: Duration = Duration::from_secs(180);
/// Janela das vendas nos preços típicos do modo de melhorias (60 dias).
const JANELA_PRECOS_MS: i64 = 60 * 86_400_000;

/// Parâmetros de um job de equipe.
#[derive(Debug, Clone, PartialEq)]
pub struct Pedido {
    pub boss: Option<String>,
    pub moeda: String,
    pub nick: Option<String>,
    pub idade_min: f64,
    pub n: usize,
    /// `orcamento=<gemas>`: também a melhor equipe até esse valor.
    pub orcamento: Option<f64>,
}

impl Pedido {
    fn chave(&self) -> String {
        json!([
            self.boss.clone().unwrap_or_default(),
            self.moeda,
            self.nick.clone().unwrap_or_default(),
            self.idade_min,
            self.n,
            self.orcamento
        ])
        .to_string()
    }
    /// Pedidos do mesmo painel (só o boss muda).
    fn grupo(&self) -> String {
        json!([
            self.moeda,
            self.nick.clone().unwrap_or_default(),
            self.idade_min,
            self.n
        ])
        .to_string()
    }
}

#[derive(Default)]
struct EstadoJob {
    fim: Option<Instant>,
    data: Option<Arc<Value>>,
    erro: Option<String>,
    anterior: Option<Arc<Value>>,
    progresso: Option<Value>,
}

pub struct Job {
    grupo: String,
    estado: StdMutex<EstadoJob>,
    pronto: Notify,
    cancelar: CancellationToken,
}

/// O que a rota responde sobre um job.
pub enum Situacao {
    Pronto(Arc<Value>),
    Erro(String),
    Carregando {
        anterior: Option<Arc<Value>>,
        progresso: Option<Value>,
    },
}

impl Job {
    fn situacao(&self) -> Situacao {
        let e = self
            .estado
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match (&e.fim, &e.data, &e.erro) {
            (Some(_), _, Some(erro)) => Situacao::Erro(erro.clone()),
            (Some(_), Some(d), None) => Situacao::Pronto(d.clone()),
            _ => Situacao::Carregando {
                anterior: e.anterior.clone(),
                progresso: e.progresso.clone(),
            },
        }
    }

    fn concluir(&self, r: Result<Value, String>) {
        {
            let mut e = self
                .estado
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            match r {
                Ok(v) => e.data = Some(Arc::new(v)),
                Err(erro) => e.erro = Some(erro),
            }
            e.fim = Some(Instant::now());
        }
        self.pronto.notify_waiters();
    }

    fn vigente(&self) -> bool {
        let e = self
            .estado
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if self.cancelar.is_cancelled() || e.erro.is_some() {
            // Cancelado ou com erro: o próximo pedido igual calcula de novo.
            return e.fim.is_none() && !self.cancelar.is_cancelled();
        }
        e.fim.is_none_or(|f| f.elapsed() < VALIDADE_JOB)
    }

    /// Espera o job terminar por até `limite`.
    pub async fn esperar(&self, limite: Duration) -> Situacao {
        let notificado = self.pronto.notified();
        if let s @ (Situacao::Pronto(_) | Situacao::Erro(_)) = self.situacao() {
            return s;
        }
        let _ = tokio::time::timeout(limite, notificado).await;
        self.situacao()
    }
}

type Cache<T> = Mutex<Option<(Instant, Arc<T>)>>;
/// Valores com o instante em que foram calculados, por chave.
type PorChave<T> = Mutex<HashMap<String, (Instant, Arc<T>)>>;

pub struct ServicoEquipeBolsa {
    /// `None` se o pool não pôde ser criado: aí o cálculo usa o pool global do `rayon`.
    pool: Option<rayon::ThreadPool>,
    wiki: Cache<Vec<BossWiki>>,
    precos: PorChave<HashMap<i64, f64>>,
    jobs: StdMutex<HashMap<String, Arc<Job>>>,
    propria: PorChave<Value>,
    alvos: PorChave<Value>,
    /// Serializa as gravações de faltas (ler, aplicar, gravar).
    faltas: Mutex<()>,
}

#[derive(Debug, thiserror::Error)]
pub enum ErroEquipeBolsa {
    #[error(transparent)]
    Banco(#[from] sqlx::Error),
    #[error(transparent)]
    Equipes(#[from] super::ErroServico),
    #[error("{0}")]
    Repo(String),
    #[error("o cálculo foi cancelado")]
    Cancelado,
    #[error("o cálculo passou do tempo limite")]
    Tempo,
    #[error("servidor ocupado com outros cálculos de equipe; tente de novo em instantes")]
    Ocupado,
    #[error("o cálculo falhou")]
    Pool,
}

impl ServicoEquipeBolsa {
    pub fn novo(threads: usize) -> Self {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads.max(1))
            .thread_name(|i| format!("nexus-cpu-{i}"))
            .build()
            .inspect_err(|e| tracing::warn!(erro = %e, "pool de CPU não criado; usando o global"))
            .ok();
        ServicoEquipeBolsa {
            pool,
            wiki: Mutex::new(None),
            precos: Mutex::new(HashMap::new()),
            jobs: StdMutex::new(HashMap::new()),
            propria: Mutex::new(HashMap::new()),
            alvos: Mutex::new(HashMap::new()),
            faltas: Mutex::new(()),
        }
    }

    /// Roda `f` no pool `rayon` e espera o resultado sem bloquear o tokio.
    pub async fn no_pool<T: Send + 'static>(
        &self,
        f: impl FnOnce() -> T + Send + 'static,
    ) -> Result<T, ErroEquipeBolsa> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let tarefa = move || {
            let _ = tx.send(f());
        };
        match &self.pool {
            Some(p) => p.spawn(tarefa),
            None => rayon::spawn(tarefa),
        }
        rx.await.map_err(|_| ErroEquipeBolsa::Pool)
    }

    async fn wiki(&self, db: &sqlx::PgPool) -> Result<Arc<Vec<BossWiki>>, ErroEquipeBolsa> {
        let mut c = self.wiki.lock().await;
        if let Some((_, w)) = c.as_ref().filter(|(em, _)| em.elapsed() < RECARGA_WIKI) {
            return Ok(w.clone());
        }
        let w = Arc::new(persistencia::bolsa_equipe::carregar_wiki(db).await?);
        *c = Some((Instant::now(), w.clone()));
        Ok(w)
    }

    /// Preços típicos por espécie, com cache de 30 minutos. Se a consulta falha, segue sem
    /// preços (como o legado).
    async fn precos(
        &self,
        db: &sqlx::PgPool,
        moeda: &str,
        lo: f64,
        hi: f64,
        melhorias: bool,
    ) -> Arc<HashMap<i64, f64>> {
        let k = format!("{moeda}|{lo}|{hi}|{melhorias}");
        let mut c = self.precos.lock().await;
        if let Some((_, p)) = c.get(&k).filter(|(em, _)| em.elapsed() < RECARGA_PRECOS) {
            return p.clone();
        }
        let corte = agora_ms() - OFERTA_FRESCA_POKEMON_MS;
        let p = match persistencia::bolsa_equipe::precos_tipicos_modo(
            db,
            moeda,
            lo,
            hi,
            corte,
            melhorias.then(|| agora_ms() - JANELA_PRECOS_MS),
        )
        .await
        {
            Ok(m) => Arc::new(m),
            Err(e) => {
                // Sem cache: a próxima chamada tenta de novo.
                tracing::warn!(erro = %e, "preços típicos indisponíveis");
                return Arc::new(HashMap::new());
            }
        };
        if c.len() >= 12 {
            c.clear();
        }
        c.insert(k, (Instant::now(), p.clone()));
        p
    }

    /// O job do pedido: o vigente, ou um novo.
    pub fn trabalho(self: &Arc<Self>, estado: &Estado, mut p: Pedido) -> Arc<Job> {
        // Idade e orçamento arredondados: variações mínimas viram o mesmo job.
        p.idade_min = p.idade_min.round();
        p.orcamento = p.orcamento.map(f64::round);
        let chave = p.chave();
        let mut jobs = self
            .jobs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(j) = jobs.get(&chave).filter(|j| j.vigente()) {
            return j.clone();
        }
        let anterior = jobs.get(&chave).and_then(|j| {
            let e = j
                .estado
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            e.data.clone().or_else(|| e.anterior.clone())
        });
        let ativos = jobs
            .values()
            .filter(|j| j.estado.lock().map(|e| e.fim.is_none()).unwrap_or(false))
            .count();
        if ativos >= MAX_ATIVOS {
            // Não guarda: o próximo pedido tenta de novo.
            let ocupado = Arc::new(Job {
                grupo: p.grupo(),
                estado: StdMutex::new(EstadoJob::default()),
                pronto: Notify::new(),
                cancelar: CancellationToken::new(),
            });
            ocupado.concluir(Err(ErroEquipeBolsa::Ocupado.to_string()));
            return ocupado;
        }
        // O painel trocou de boss: o cálculo antigo do mesmo painel deixa de interessar.
        let grupo = p.grupo();
        for (k, j) in jobs.iter() {
            if *k != chave
                && j.grupo == grupo
                && j.estado.lock().map(|e| e.fim.is_none()).unwrap_or(false)
            {
                j.cancelar.cancel();
            }
        }
        let job = Arc::new(Job {
            grupo,
            estado: StdMutex::new(EstadoJob {
                anterior,
                ..Default::default()
            }),
            pronto: Notify::new(),
            cancelar: CancellationToken::new(),
        });
        jobs.insert(chave.clone(), job.clone());
        if jobs.len() > MAX_JOBS {
            let velho = jobs
                .iter()
                .filter(|(k, j)| {
                    **k != chave && j.estado.lock().map(|e| e.fim.is_some()).unwrap_or(false)
                })
                .map(|(k, _)| k.clone())
                .next();
            if let Some(k) = velho {
                jobs.remove(&k);
            }
        }
        drop(jobs);
        let (servico, estado, j) = (self.clone(), estado.clone(), job.clone());
        tokio::spawn(async move {
            let inicio = Instant::now();
            let r = match tokio::time::timeout(LIMITE_CALCULO, servico.calcular(&estado, &p, &j))
                .await
            {
                Ok(r) => r,
                Err(_) => Err(ErroEquipeBolsa::Tempo),
            };
            metrics::histogram!("nexus_equipe_calculo_segundos")
                .record(inicio.elapsed().as_secs_f64());
            if let Err(e) = &r {
                tracing::warn!(erro = %e, boss = ?p.boss, "cálculo de equipe falhou");
            }
            j.concluir(r.map_err(|e| e.to_string()));
        });
        job
    }

    /// Um job de compra já concluído (para a sugestão da equipe própria).
    fn concluido(&self, p: &Pedido) -> Option<Arc<Value>> {
        let jobs = self
            .jobs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let j = jobs.get(&p.chave())?;
        let e = j
            .estado
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        e.fim.and(e.data.clone())
    }

    async fn calcular(
        &self,
        estado: &Estado,
        p: &Pedido,
        job: &Job,
    ) -> Result<Value, ErroEquipeBolsa> {
        let cancelado = || job.cancelar.is_cancelled();
        let motor = estado.equipes.motor().await?;
        let est = estado.equipes.estado().await?;
        let wiki = self.wiki(&estado.db).await?;
        let rb = match be::resolver_boss(&motor, &est, &wiki, p.boss.as_deref(), p.nick.as_deref())
        {
            Ok(b) => b,
            Err(e) => return Ok(json!({ "erro": e })),
        };
        let boss = rb.boss.clone();
        drop(rb);
        let agora = agora_ms();
        let idade_ms = (p.idade_min * 60_000.0) as i64;
        let brutas = estado
            .bolsa
            .repo
            .ofertas_ativas(&FiltroOfertas {
                corte_item: agora - idade_ms,
                corte_pokemon: agora - idade_ms,
                tipo: Some("pokemon".into()),
                moeda: Some(p.moeda.clone()),
                limite: 6000,
            })
            .await
            .map_err(|e| ErroEquipeBolsa::Repo(e.to_string()))?;
        let brutas: Vec<OfertaBruta> = brutas
            .into_iter()
            .filter(|o| o.ficha.as_ref().is_some_and(|f| !f.is_null()))
            .collect();
        if let Ok(mut e) = job.estado.lock() {
            e.progresso = Some(json!({ "avaliadas": 0, "total": brutas.len() }));
        }
        let snapshot = if p.moeda == "orb" {
            Some(
                estado
                    .bolsa
                    .snapshot()
                    .await
                    .map_err(|e| ErroEquipeBolsa::Repo(e.to_string()))?,
            )
        } else {
            None
        };
        let l = boss.team_level;
        let precos = self
            .precos(
                &estado.db,
                &p.moeda,
                js_round(0.7 * l).max(1.0),
                js_round(2.5 * l),
                estado.melhorias,
            )
            .await;
        // Com a escrita ligada, segura o lock desde a leitura: dois cálculos ao mesmo tempo não
        // apagam as faltas um do outro.
        let _vez = if estado.escrita_equipes {
            Some(self.faltas.lock().await)
        } else {
            None
        };
        let mut faltas = persistencia::bolsa_equipe::carregar_faltas(&estado.db).await?;
        if cancelado() {
            return Err(ErroEquipeBolsa::Cancelado);
        }
        let faltas_antes = faltas.clone();
        let (pedido, sn, melhorias) = (p.clone(), snapshot.clone(), estado.melhorias);
        let (resposta, faltas) = self
            .no_pool(move || {
                let Ok(rb) = be::resolver_boss(
                    &motor,
                    &est,
                    &wiki,
                    pedido.boss.as_deref(),
                    pedido.nick.as_deref(),
                ) else {
                    return (json!({ "erro": "boss não encontrado" }), faltas);
                };
                let cotacoes = cotacoes(sn.as_deref(), &brutas, agora);
                let listas = be::avaliar_especies(&motor, &rb.boss).map(|a| {
                    let ex = |perfil: &str| faltas.extras(&rb.boss.key, perfil, agora);
                    be::listas_do_boss(&a, &precos, [0, ex("equilibrada"), ex("economica")])
                });
                let ofertas: Vec<OfertaEquipe> = brutas
                    .iter()
                    .map(|o| OfertaEquipe {
                        id: o.id,
                        preco: o.preco.map_or(f64::NAN, |x| x as f64),
                        quantidade: o.quantidade.map_or(0.0, f64::from),
                        ficha: o.ficha.clone().unwrap_or(Value::Null),
                    })
                    .collect();
                let preco_de = preco_item(sn.as_deref(), &pedido.moeda);
                let entrada = EntradaBolsa {
                    motor: &motor,
                    boss: &rb,
                    moeda: &pedido.moeda,
                    idade_min: pedido.idade_min,
                    n: pedido.n,
                    ofertas: &ofertas,
                    cotacoes: &cotacoes,
                    listas,
                    preco_de: &preco_de,
                    agora,
                    orcamento: pedido.orcamento,
                    melhorias,
                };
                let r = be::equipe_bolsa(&entrada, &mut faltas)
                    .unwrap_or_else(|e| json!({ "erro": e }));
                (r, faltas)
            })
            .await?;
        if estado.escrita_equipes {
            if faltas != faltas_antes {
                persistencia::bolsa_equipe::gravar_faltas(&estado.db, &faltas).await?;
            }
            if let Some(s) = sugestao(&resposta, &boss.key, l, &p.moeda, agora) {
                persistencia::bolsa_equipe::registrar_sugestao(&estado.db, agora, &boss.key, &s)
                    .await?;
            }
        }
        Ok(resposta)
    }

    /// `GET /api/bolsa/equipe-propria`.
    pub async fn propria(
        self: &Arc<Self>,
        estado: &Estado,
        boss: Option<String>,
        nick: Option<String>,
    ) -> Result<Arc<Value>, ErroEquipeBolsa> {
        let motor = estado.equipes.motor().await?;
        let est: Arc<EstadoEquipes> = estado.equipes.estado().await?;
        let wiki = self.wiki(&estado.db).await?;
        let rb = match be::resolver_boss(&motor, &est, &wiki, boss.as_deref(), nick.as_deref()) {
            Ok(b) => b,
            Err(e) => return Ok(Arc::new(json!({ "erro": e }))),
        };
        if rb.conta.is_none() {
            return Ok(Arc::new(json!({ "erro": "conta não encontrada" })));
        }
        let chave = format!(
            "{}|{}|{}|{}|{}",
            rb.boss.key,
            rb.boss.team_level,
            nick.clone().unwrap_or_default(),
            rb.conta
                .and_then(|c| c.get("em"))
                .map(ToString::to_string)
                .unwrap_or_default(),
            rb.conta
                .and_then(|c| c.get("pokemons"))
                .and_then(Value::as_array)
                .map_or(0, Vec::len),
        );
        if let Some((_, v)) = self
            .propria
            .lock()
            .await
            .get(&chave)
            .filter(|(em, _)| em.elapsed() < VALIDADE_PROPRIA)
        {
            return Ok(v.clone());
        }
        // Sugestão de custo para completar: a equipe Econômica de compra do mesmo boss.
        let compra = Pedido {
            boss: Some(rb.boss.key.clone()),
            moeda: "orb".into(),
            nick: nick.clone(),
            idade_min: 90.0,
            n: 5,
            orcamento: None,
        };
        let eco_compra = match self.concluido(&compra) {
            Some(d) => d.pointer("/perfis/eco").and_then(|eco| {
                let slots = eco
                    .get("slots")
                    .and_then(Value::as_array)
                    .map_or(0, Vec::len);
                let ok = !eco
                    .get("indisponivel")
                    .and_then(Value::as_bool)
                    .unwrap_or(true)
                    && slots > 0;
                ok.then(|| {
                    (
                        eco.get("total").and_then(Value::as_f64).unwrap_or(0.0),
                        slots,
                    )
                })
            }),
            None => {
                self.trabalho(estado, compra);
                None
            }
        };
        drop(rb);
        let snapshot = estado
            .bolsa
            .snapshot()
            .await
            .map_err(|e| ErroEquipeBolsa::Repo(e.to_string()))?;
        let agora = agora_ms();
        let melhorias = estado.melhorias;
        let resposta = self
            .no_pool(move || {
                let Ok(rb) =
                    be::resolver_boss(&motor, &est, &wiki, boss.as_deref(), nick.as_deref())
                else {
                    return json!({ "erro": "boss não encontrado" });
                };
                let preco_de = preco_item(Some(&snapshot), "orb");
                let valor = |f: &Value| valor_ficha(&snapshot, f, agora);
                let e = EntradaPropria {
                    motor: &motor,
                    estado: &est,
                    boss: &rb,
                    nick: nick.as_deref(),
                    valor_ficha: &valor,
                    preco_de: &preco_de,
                    eco_compra,
                    agora,
                    melhorias,
                };
                be::equipe_propria(&e).unwrap_or_else(|e| json!({ "erro": e }))
            })
            .await?;
        let v = Arc::new(resposta);
        let mut c = self.propria.lock().await;
        if c.len() >= 60 {
            c.clear();
        }
        c.insert(chave, (Instant::now(), v.clone()));
        Ok(v)
    }
}

impl ServicoEquipeBolsa {
    /// `GET /api/bolsa/alvos-coleta`: espécies que valem a pena coletar. Cache de 3 h por
    /// (nível de treinador, bosses, máximo, versão dos preços, faltas), como no legado.
    pub async fn alvos(
        &self,
        estado: &Estado,
        nick: Option<String>,
        max: Option<String>,
    ) -> Result<Value, ErroEquipeBolsa> {
        let motor = estado.equipes.motor().await?;
        let est = estado.equipes.estado().await?;
        let (tr, lista, lo, hi) = be::contexto_alvos(&motor, &est, nick.as_deref());
        let precos = self
            .precos(&estado.db, "orb", lo, hi, estado.melhorias)
            .await;
        let agora = agora_ms();
        let mut faltas = {
            let _vez = if estado.escrita_equipes {
                Some(self.faltas.lock().await)
            } else {
                None
            };
            let mut f = persistencia::bolsa_equipe::carregar_faltas(&estado.db).await?;
            if f.limpar(agora) && estado.escrita_equipes {
                persistencia::bolsa_equipe::gravar_faltas(&estado.db, &f).await?;
            }
            f
        };
        faltas.limpar(agora);
        let (fl, ativas, sig) = faltas.para_rota(agora);
        let versao = agora / 1_800_000;
        let chave = format!(
            "{tr}|{}|{}|{versao}|{sig}",
            lista
                .iter()
                .map(|b| format!("{}{}", b.key, b.team_level))
                .collect::<Vec<_>>()
                .join(","),
            max.clone().unwrap_or_default()
        );
        let em_cache = self
            .alvos
            .lock()
            .await
            .get(&chave)
            .filter(|(em, _)| em.elapsed() < Duration::from_secs(3 * 3600))
            .map(|(_, v)| v.clone());
        let v = match em_cache {
            Some(v) => v,
            None => {
                let v = Arc::new(
                    self.no_pool(move || {
                        be::alvos_coleta(
                            &motor,
                            &est,
                            nick.as_deref(),
                            max.as_deref(),
                            &precos,
                            &fl,
                            agora,
                        )
                    })
                    .await?,
                );
                let mut c = self.alvos.lock().await;
                if c.len() >= 20 {
                    c.clear();
                }
                c.insert(chave, (Instant::now(), v.clone()));
                v
            }
        };
        let mut r = (*v).clone();
        if let Value::Object(o) = &mut r {
            o.insert("faltas".into(), Value::Bool(ativas));
        }
        Ok(r)
    }
}

impl ServicoEquipeBolsa {
    /// `GET /api/bolsa/equipe-hibrida`: os pokémon da conta, completados pelas ofertas.
    pub async fn hibrida(
        &self,
        estado: &Estado,
        boss: Option<String>,
        nick: Option<String>,
        moeda: String,
    ) -> Result<Value, ErroEquipeBolsa> {
        let motor = estado.equipes.motor().await?;
        let est = estado.equipes.estado().await?;
        let wiki = self.wiki(&estado.db).await?;
        let agora = agora_ms();
        let brutas = estado
            .bolsa
            .repo
            .ofertas_ativas(&FiltroOfertas {
                corte_item: agora - OFERTA_FRESCA_POKEMON_MS,
                corte_pokemon: agora - OFERTA_FRESCA_POKEMON_MS,
                tipo: Some("pokemon".into()),
                moeda: Some(moeda.clone()),
                limite: 6000,
            })
            .await
            .map_err(|e| ErroEquipeBolsa::Repo(e.to_string()))?;
        let melhorias = estado.melhorias;
        self.no_pool(move || {
            let rb = match be::resolver_boss(&motor, &est, &wiki, boss.as_deref(), nick.as_deref())
            {
                Ok(b) => b,
                Err(e) => return json!({ "erro": e }),
            };
            let ofertas: Vec<OfertaEquipe> = brutas
                .iter()
                .filter_map(|o| {
                    Some(OfertaEquipe {
                        id: o.id,
                        preco: o.preco? as f64,
                        quantidade: o.quantidade.map_or(0.0, f64::from),
                        ficha: o.ficha.clone().filter(|f| !f.is_null())?,
                    })
                })
                .collect();
            let e = be::EntradaHibrida {
                motor: &motor,
                boss: &rb,
                moeda: &moeda,
                ofertas: &ofertas,
                n: 5,
                melhorias,
            };
            be::equipe_hibrida(&e).unwrap_or_else(|e| json!({ "erro": e }))
        })
        .await
    }
}

fn js_round(x: f64) -> f64 {
    nucleo::js::round(x)
}

/// Cotação de cada oferta em orb (o `cotarOfertas` do legado): o JSON da oferta avaliada.
fn cotacoes(s: Option<&Snapshot>, brutas: &[OfertaBruta], agora: i64) -> HashMap<i64, Value> {
    let Some(s) = s else { return HashMap::new() };
    let valores = ofertas::precalcular(&s.mercado, brutas, agora);
    ofertas::avaliar(&s.mercado, brutas, &valores)
        .iter()
        .filter_map(|a| {
            Some((
                a.oferta_id,
                serde_json::to_value(crate::conversao::oferta(a)).ok()?,
            ))
        })
        .collect()
}

/// Valor justo de uma ficha (o `valorFicha` do legado).
fn valor_ficha(s: &Snapshot, f: &Value, agora: i64) -> Option<f64> {
    let o = OfertaBruta {
        id: 0,
        ts_visto: agora,
        tipo: Some("pokemon".into()),
        moeda: Some("orb".into()),
        preco: Some(1),
        especie: None,
        item_id: None,
        quantidade: Some(1),
        nome: f.get("nome").and_then(Value::as_str).map(str::to_owned),
        ficha: Some(f.clone()),
    };
    let lista = [o];
    let valores = ofertas::precalcular(&s.mercado, &lista, agora);
    ofertas::avaliar(&s.mercado, &lista, &valores)
        .first()
        .map(|a| a.justo)
}

/// Preço de um item para o lucro: mediana 7d, último ou menor anúncio do ticker.
fn preco_item<'s>(
    s: Option<&'s Snapshot>,
    moeda: &str,
) -> impl Fn(&Value, &Value) -> (Option<f64>, Option<&'static str>) + 's {
    let moeda = moeda.to_owned();
    move |id: &Value, nome: &Value| {
        let Some(s) = s else { return (None, None) };
        let chave = format!("i:{}:{moeda}", nucleo::js::texto(Some(id)));
        let nome = nucleo::js::texto(Some(nome)).to_lowercase();
        let t = s
            .mercado
            .tickers
            .iter()
            .find(|t| t.texto == chave)
            .or_else(|| {
                s.mercado.tickers.iter().find(|t| {
                    !t.pokemon && t.moeda.como_str() == moeda && t.nome.to_lowercase() == nome
                })
            });
        let Some(t) = t else { return (None, None) };
        let min = t.of_stat.as_ref().map_or(0.0, |o| o.min);
        if t.a.ma7 > 0.0 {
            (Some(t.a.ma7), Some("mediana 7d"))
        } else if t.a.ultimo > 0.0 {
            (Some(t.a.ultimo), Some("último"))
        } else if min > 0.0 {
            (Some(min), Some("menor anúncio"))
        } else {
            (None, Some("menor anúncio"))
        }
    }
}

/// Linha da sugestão (o `equipe-sugestoes.jsonl` do legado).
fn sugestao(r: &Value, boss: &str, nivel: f64, moeda: &str, agora: i64) -> Option<Value> {
    let perfis = r.get("perfis")?.as_object()?;
    let p: serde_json::Map<String, Value> = perfis
        .iter()
        .filter(|(_, p)| !p.get("indisponivel").and_then(Value::as_bool).unwrap_or(true))
        .map(|(k, p)| {
            let slots = p.get("slots").and_then(Value::as_array).cloned().unwrap_or_default();
            (
                k.clone(),
                json!({
                    "total": p.get("total"), "margem": p.get("poder"), "mult": p.get("mult"), "abaixo": p.get("abaixo"),
                    "tSob": p.get("tSobrevive"),
                    "ids": slots.iter().map(|s| s.get("ofertaId").cloned().unwrap_or(Value::Null)).collect::<Vec<_>>(),
                    "lv": slots.iter().map(|s| s.get("nivel").cloned().unwrap_or(Value::Null)).collect::<Vec<_>>(),
                }),
            )
        })
        .collect();
    Some(
        json!({ "ts": agora, "boss": boss, "nv": nivel, "moeda": moeda, "ref": r.pointer("/ref/soma"), "perfis": p }),
    )
}
