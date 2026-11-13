//! Comparador de paridade com o legado (o "shadow" da fase 1).
//!
//! O tráfego da bolsa não passa pelo Nginx do Nexus, então espelhar requisições não serve.
//! Em vez disso, o Nexus chama o legado e a si mesmo com os mesmos parâmetros e compara as
//! respostas: JSON normalizado, números com tolerância, campos voláteis ignorados. Para que os
//! dois olhem o mesmo instante, o mercado do Nexus é montado no `geradoEm` do legado.
//!
//! Nada muda no legado: são só GETs na porta dele.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use contrato::bolsa::Params;
use nucleo::bolsa::ingestao::RepoIngestao;
use nucleo::bolsa::repo::RepoMercado;
use serde::Serialize;
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use super::consultas;
use super::servico::ServicoBolsa;

/// Campos que mudam a cada chamada e não entram na comparação.
const VOLATEIS: &[&str] = &["geradoEm", "ultimaCaptura"];
/// Campos que dependem do instante exato da análise. O legado analisa com um `Date.now()`
/// alguns segundos anterior ao `geradoEm` que publica; a diferença aparece na 6ª casa.
const SENSIVEIS_AO_TEMPO: &[&str] = &["spd"];
/// Estatísticas lidas ao vivo (ofertas e vendas mudam entre a leitura do legado e a do Nexus
/// enquanto o relay está ativo): tolerância de 5%.
const AO_VIVO: &[&str] = &[
    "n",
    "min",
    "p25",
    "med",
    "p75",
    "max",
    "comparaveis",
    "askMin",
    "chance",
    // Bolas por hora = `kph × bolasKill` do ritmo, taxa recalculada a cada leitura do relay;
    // o Nexus lê o estado importado com atraso e a taxa oscila 1–3% entre as duas leituras.
    "bph",
];
/// Contadores de bolas que crescem a cada captura: o Nexus lê o estado importado com atraso e
/// fica poucas dezenas atrás do legado. Tolerância de 1%; `azar` (chance de já ter saído o
/// shiny) é derivado de `bolasDesde` e herda o atraso.
const CONTADORES: &[&str] = &["bolasDesde", "totalBolas", "azar"];
const AMOSTRA: usize = 15;
/// Validade do cache do mercado no legado (`TTL` do `bolsa.js`).
const CACHE_MERCADO_LEGADO_MS: i64 = 3 * 60_000;

/// Igualdade numérica com tolerância relativa (as funções `exp`/`ln` do V8 e da libm podem
/// divergir no último bit).
fn perto(a: f64, b: f64) -> bool {
    a == b || (a - b).abs() <= 1e-9 || (a - b).abs() <= 1e-6 * a.abs().max(b.abs())
}

/// O legado ainda não terminou o trabalho em segundo plano de `falta.sugestao` enquanto o
/// Nexus já tem o custo. Enquanto o trabalho roda, o legado responde `null` ou só a `nota`
/// ("Veja a aba Econômica..."), sem `custoAproximado`.
fn sugestao_pendente(legado: &Value, nexus: &Value) -> bool {
    let sem_custo = |v: &Value| {
        v.pointer("/falta/sugestao")
            .is_some_and(|s| s.get("custoAproximado").is_none())
    };
    let com_custo = |v: &Value| v.pointer("/falta/sugestao/custoAproximado").is_some();
    sem_custo(legado) && com_custo(nexus)
}

/// Primeira diferença entre dois JSONs, com o caminho até ela.
pub fn diferenca(a: &Value, b: &Value, caminho: &str) -> Option<String> {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => {
            let (x, y) = (
                x.as_f64().unwrap_or(f64::NAN),
                y.as_f64().unwrap_or(f64::NAN),
            );
            let tempo = SENSIVEIS_AO_TEMPO
                .iter()
                .any(|c| caminho.ends_with(&format!(".{c}")));
            // Só nas estatísticas de ofertas e vendas (ofStat, hist) e nos campos derivados delas.
            let vivo = AO_VIVO.iter().any(|c| caminho.ends_with(&format!(".{c}")))
                && (caminho.contains(".ofStat.")
                    || caminho.contains(".hist.")
                    || [".comparaveis", ".askMin", ".chance"]
                        .iter()
                        .any(|c| caminho.ends_with(c)));
            // Valor justo e custo de completar a equipe: o legado guarda o valor justo de cada
            // ficha por 10 a 30 min e a sugestão vem de um cálculo de até 5 min atrás; o Nexus
            // reavalia a cada chamada. Tolerância de 15% só nesses campos.
            let cache_legado = [
                ".custoOportunidade",
                ".custoAproximado",
                ".custoMedioPorSlot",
            ]
            .iter()
            .any(|c| caminho.ends_with(c));
            let ok = perto(x, y)
                || (cache_legado && (x - y).abs() <= 0.15 * x.abs().max(y.abs()))
                || (tempo && (x - y).abs() <= 1e-4 * x.abs().max(y.abs()))
                || (CONTADORES
                    .iter()
                    .any(|c| caminho.ends_with(&format!(".{c}")))
                    && (x - y).abs() <= 0.01 * x.abs().max(y.abs()))
                || (vivo && (x - y).abs() <= 0.05 * x.abs().max(y.abs()));
            (!ok).then(|| format!("{caminho}: {x} ≠ {y}"))
        }
        (Value::Array(x), Value::Array(y)) => {
            if x.len() != y.len() {
                return Some(format!("{caminho}: tamanho {} ≠ {}", x.len(), y.len()));
            }
            x.iter()
                .zip(y)
                .enumerate()
                .find_map(|(i, (a, b))| diferenca(a, b, &format!("{caminho}[{i}]")))
        }
        (Value::Object(x), Value::Object(y)) => {
            let chaves: std::collections::BTreeSet<&String> = x.keys().chain(y.keys()).collect();
            chaves
                .into_iter()
                .filter(|k| !VOLATEIS.contains(&k.as_str()))
                .find_map(|k| {
                    let nulo = Value::Null;
                    diferenca(
                        x.get(k).unwrap_or(&nulo),
                        y.get(k).unwrap_or(&nulo),
                        &format!("{caminho}.{k}"),
                    )
                })
        }
        _ if a == b => None,
        _ => Some(format!("{caminho}: {a} ≠ {b}")),
    }
}

#[derive(Debug, Default, Serialize)]
pub struct ResultadoRota {
    pub comparados: u64,
    pub iguais: u64,
    pub so_legado: u64,
    pub so_nexus: u64,
    pub exemplos: Vec<String>,
}

impl ResultadoRota {
    fn registrar(&mut self, id: &str, diff: Option<String>) {
        self.comparados += 1;
        match diff {
            None => self.iguais += 1,
            Some(d) if self.exemplos.len() < 5 => self.exemplos.push(format!("{id}: {d}")),
            Some(_) => {}
        }
    }
}

pub type Relatorio = BTreeMap<&'static str, ResultadoRota>;

pub struct Comparador<R> {
    servico: Arc<ServicoBolsa<R>>,
    url: String,
    http: reqwest::Client,
    rodada: std::sync::atomic::AtomicUsize,
    estado: Option<crate::estado::Estado>,
    /// Compara também `/api/bolsa/equipe` e `/equipe-propria`, que disparam o cálculo pesado
    /// do legado.
    equipe_bolsa: bool,
    /// Motor do legado (`motor.*`); sem URL, o motor fica de fora.
    motor_url: Option<String>,
}

/// `R: RepoMercado + RepoIngestao`: o comparador lê o mercado e o inventário, então o
/// repositório precisa implementar as duas portas.
impl<R: RepoMercado + RepoIngestao + 'static> Comparador<R> {
    pub fn novo(servico: Arc<ServicoBolsa<R>>, url: &str) -> Result<Self, reqwest::Error> {
        Ok(Self {
            servico,
            url: url.trim_end_matches('/').to_owned(),
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(60))
                .build()?,
            rodada: Default::default(),
            estado: None,
            equipe_bolsa: false,
            motor_url: None,
        })
    }

    /// Liga a comparação das rotas de equipes (`/api/equipes/*` e as de equipe da bolsa).
    pub fn com_equipes(mut self, estado: crate::estado::Estado, equipe_bolsa: bool) -> Self {
        self.estado = Some(estado);
        self.equipe_bolsa = equipe_bolsa;
        self
    }

    /// Liga a comparação das rotas `GET` do motor (`motor.*`) com o motor do legado.
    pub fn com_motor(mut self, url: Option<&str>) -> Self {
        self.motor_url = url.map(|u| u.trim_end_matches('/').to_owned());
        self
    }

    /// Só a comparação do motor (comando `paridade-motor`).
    pub async fn rodada_motor(&self) -> Result<Relatorio, String> {
        let estado = self.estado.as_ref().ok_or("comparador sem estado")?;
        let base = self
            .motor_url
            .as_deref()
            .ok_or("comparador sem URL do motor")?;
        let mut rel = Relatorio::new();
        self.comparar_motor(estado, base, &mut rel).await?;
        Ok(rel)
    }

    /// Rotas `GET` do motor: cada uma sem `nick` e, as que recebem conta, com cada nick do
    /// estado guardado. Comparação exata: status e corpo. O Nexus calcula no `agora` que o
    /// legado devolveu, para a data e a hora locais serem as mesmas.
    async fn comparar_motor(
        &self,
        estado: &crate::estado::Estado,
        base: &str,
        rel: &mut Relatorio,
    ) -> Result<(), String> {
        use crate::motor::{responder, Rota};
        let d = persistencia::motor::carregar_dados(&estado.db)
            .await
            .map_err(|e| e.to_string())?;
        let mut nicks: Vec<Option<String>> = vec![None];
        if let Some(Value::Object(c)) = d.get("config") {
            nicks.extend(c.keys().cloned().map(Some));
        }
        let sem_nick = [None];
        for rota in Rota::TODAS {
            let pedidos: &[Option<String>] = if rota.por_conta() { &nicks } else { &sem_nick };
            for nick in pedidos {
                let url = format!("{base}{}", rota.caminho());
                let params: Vec<(&str, &str)> =
                    nick.as_deref().map(|n| ("nick", n)).into_iter().collect();
                let url =
                    reqwest::Url::parse_with_params(&url, &params).map_err(|e| e.to_string())?;
                let req = self.http.get(url);
                let r = req.send().await.map_err(|e| e.to_string())?;
                let status = r.status().as_u16();
                let legado: Value = r.json().await.map_err(|e| e.to_string())?;
                let agora = legado
                    .get("agora")
                    .and_then(Value::as_i64)
                    .unwrap_or_else(super::agora_ms);
                let (n_status, n_corpo) = match responder(
                    &estado.db,
                    estado.motor_fuso_ms,
                    agora,
                    rota,
                    nick.as_deref(),
                )
                .await
                .map_err(|e| e.to_string())?
                {
                    Ok(x) => (x.status, x.corpo),
                    Err(_) => (500, serde_json::json!({ "erro": "erro interno" })),
                };
                let diff = if status == n_status {
                    persistencia::motor::primeira_diferenca(&legado, &n_corpo, "")
                } else {
                    Some(format!("status {status} ≠ {n_status}"))
                };
                rel.entry(rota.nome())
                    .or_default()
                    .registrar(nick.as_deref().unwrap_or("sem nick"), diff);
            }
        }
        Ok(())
    }

    /// `/api/bolsa/equipe-propria` (conta padrão, próximo boss) e `/api/bolsa/alvos-coleta`:
    /// comparação normal. `/api/bolsa/equipe`: por qualidade, porque o otimizador é
    /// heurístico e duas buscas corretas podem achar equipes diferentes.
    async fn comparar_equipe_bolsa(
        &self,
        estado: &crate::estado::Estado,
        rel: &mut Relatorio,
    ) -> Result<(), String> {
        let (_, l) = self.legado("/api/bolsa/alvos-coleta").await?;
        let n = estado
            .equipe_bolsa
            .alvos(estado, None, None)
            .await
            .map_err(|e| e.to_string())?;
        // Equilibrada e Econômica dependem dos preços típicos, lidos em instantes diferentes
        // nos dois lados; a Máxima (só dano) e a lista de bosses comparam exatamente.
        let so_motor = |v: &Value| {
            v.get("bosses")
                .and_then(Value::as_array)
                .map(|b| {
                    b.iter()
                        .map(|x| serde_json::json!({ "nome": x["nome"], "nivel": x["nivel"], "tipos": x["tipos"], "maxima": x["maxima"] }))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        };
        rel.entry("alvos-coleta").or_default().registrar(
            "padrão",
            diferenca(&Value::Array(so_motor(&l)), &Value::Array(so_motor(&n)), ""),
        );

        if !self.equipe_bolsa {
            return Ok(());
        }
        let (mut status, mut l) = self.legado("/api/bolsa/equipe-propria").await?;
        let n = estado
            .equipe_bolsa
            .propria(estado, None, None)
            .await
            .map_err(|e| e.to_string())?;
        // O legado calcula a sugestão (`falta.sugestao`) num trabalho em segundo plano e responde
        // `null` até ele terminar; o Nexus calcula na hora. Repete a leitura do legado até o
        // trabalho acabar (é tempo, não lógica). O trabalho entra numa fila de equipes e pode
        // levar perto de um minuto.
        for _ in 0..12 {
            if !sugestao_pendente(&l, &n) {
                break;
            }
            tokio::time::sleep(Duration::from_secs(5)).await;
            (status, l) = self.legado("/api/bolsa/equipe-propria").await?;
        }
        let ns = if n.get("erro").is_some() { 404 } else { 200 };
        let colecao = |v: &Value| {
            (
                v.pointer("/conta/pokemons").cloned(),
                v.pointer("/conta/trLv").cloned(),
            )
        };
        if status == ns && colecao(&l) != colecao(&n) {
            rel.entry("equipe-propria")
                .or_default()
                .exemplos
                .push("coleção do Nexus defasada; comparação pulada nesta rodada".into());
        } else {
            let diff = if status == ns {
                let sem_tempo = |mut v: Value| {
                    if let Value::Object(o) = &mut v {
                        o.remove("geradoEm");
                    }
                    v
                };
                diferenca(&sem_tempo(l), &sem_tempo((*n).clone()), "")
            } else {
                Some(format!("status {status} ≠ {ns}"))
            };
            rel.entry("equipe-propria")
                .or_default()
                .registrar("padrão", diff);
        }

        // O legado responde 202 até terminar: espera até 2 minutos.
        let mut l = None;
        for _ in 0..24 {
            let (status, v) = self.legado("/api/bolsa/equipe").await?;
            if status != 202 {
                l = Some(v);
                break;
            }
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
        let Some(l) = l else {
            rel.entry("equipe")
                .or_default()
                .exemplos
                .push("o legado não terminou o cálculo em 2 min".into());
            return Ok(());
        };
        let pedido = crate::equipes::bolsa::Pedido {
            boss: None,
            moeda: "orb".into(),
            nick: None,
            idade_min: 90.0,
            n: 5,
            orcamento: None,
        };
        let job = estado.equipe_bolsa.trabalho(estado, pedido);
        let n = match job.esperar(Duration::from_secs(120)).await {
            crate::equipes::bolsa::Situacao::Pronto(v) => v,
            _ => return Err("o cálculo de equipe do Nexus não terminou".into()),
        };
        rel.entry("equipe")
            .or_default()
            .registrar("padrão", qualidade_equipe(&l, &n));
        Ok(())
    }

    /// `/api/equipes/bosses` e `/plano` (o próximo boss da conta padrão e, em rodízio, três
    /// bosses nos dois perfis). Só compara quando os dois lados olham o mesmo estado: o do
    /// Nexus chega pela importação, alguns minutos depois do relay gravar no legado.
    async fn comparar_equipes(
        &self,
        eq: &crate::equipes::ServicoEquipes,
        rel: &mut Relatorio,
    ) -> Result<(), String> {
        use nucleo::equipes::{resposta_bosses, resposta_plano};
        let (_, l) = self.legado("/api/equipes/bosses").await?;
        let motor = eq.motor().await.map_err(|e| e.to_string())?;
        let estado = eq.estado().await.map_err(|e| e.to_string())?;
        let n = resposta_bosses(&motor, &estado, None);
        // Mesmo estado no que importa para o motor: lista de bosses e nível de treinador.
        let marca = |v: &Value| {
            let lista: Vec<(Value, Value)> = v
                .get("bosses")
                .and_then(Value::as_array)
                .map(|b| {
                    b.iter()
                        .map(|x| (x["key"].clone(), x["teamLevel"].clone()))
                        .collect()
                })
                .unwrap_or_default();
            (lista, v.pointer("/conta/trLv").cloned())
        };
        if marca(&l) != marca(&n) {
            let r = rel.entry("equipes.bosses").or_default();
            r.exemplos
                .push("estado do Nexus defasado; comparação pulada nesta rodada".into());
            return Ok(());
        }
        rel.entry("equipes.bosses").or_default().registrar(
            "padrão",
            diferenca(&sem_campos_da_conta(l.clone()), &sem_campos_da_conta(n), ""),
        );

        let chaves: Vec<String> = l
            .get("bosses")
            .and_then(Value::as_array)
            .map(|b| {
                b.iter()
                    .filter_map(|x| Some(x.get("key")?.as_str()?.to_owned()))
                    .collect()
            })
            .unwrap_or_default();
        let passo = self.rodada.load(std::sync::atomic::Ordering::Relaxed);
        let mut casos: Vec<(Option<String>, &str)> = vec![(None, "realista")];
        if !chaves.is_empty() {
            for i in 0..3 {
                if let Some(k) = chaves.get((passo * 3 + i) % chaves.len()) {
                    casos.push((Some(k.clone()), "realista"));
                    casos.push((Some(k.clone()), "ideal"));
                }
            }
        }
        let r = rel.entry("equipes.plano").or_default();
        for (boss, perfil) in casos {
            let q = match &boss {
                Some(b) => format!("?boss={}&perfil={perfil}", urlencoding(b)),
                None => format!("?perfil={perfil}"),
            };
            let (status, l) = self.legado(&format!("/api/equipes/plano{q}")).await?;
            let n = match resposta_plano(&motor, &estado, None, boss.as_deref(), Some(perfil)) {
                Ok(v) => (200, v),
                Err(e) => (e.status, serde_json::json!({ "erro": e.erro })),
            };
            let diff = if status == n.0 {
                diferenca(&sem_campos_da_conta(l), &sem_campos_da_conta(n.1), "")
            } else {
                Some(format!("status {status} ≠ {}", n.0))
            };
            r.registrar(&q, diff);
        }
        Ok(())
    }

    /// `/api/raros2` e `/api/shiny`. O estado do Nexus chega pela importação (ou pelo relay,
    /// com `NEXUS_ESCRITA_RAROS`): os instantes (`dadosEm`, `histEm`) ficam fora da comparação.
    /// O valor das cartas depende do mercado de cada lado no instante da chamada: vai numa
    /// rota à parte (`raros2.cartas`), com tolerância de 10% no preço.
    async fn comparar_raros(
        &self,
        estado: &crate::estado::Estado,
        rel: &mut Relatorio,
    ) -> Result<(), String> {
        let (_, l) = self.legado("/api/raros2").await?;
        let n =
            crate::raros::resposta_raros2(estado, "", nucleo::raros::NOTA_MIN_PADRAO, 0.0, None)
                .await
                .map_err(|e| e.to_string())?;
        {
            let (mut lc, mut nc) = (l.clone(), n.clone());
            let (lp, np) = (cartas_de(&mut lc), cartas_de(&mut nc));
            // Totais e ritmo dependem do valor das cartas (mercado de cada lado); os instantes
            // do estado variam com o atraso da importação.
            for campo in [
                "cofre",
                "progresso",
                "falta",
                "ritmoDia",
                "diasParaMeta",
                "porConta",
                "dadosEm",
                "histEm",
                "geradoEm",
            ] {
                tirar(&mut lc, campo);
                tirar(&mut nc, campo);
            }
            rel.entry("raros2")
                .or_default()
                .registrar("padrão", diferenca(&lc, &nc, ""));
            rel.entry("raros2.cartas")
                .or_default()
                .registrar("padrão", comparar_cartas(&lp, &np));
        }
        let (_, l) = self.legado("/api/shiny").await?;
        let n = crate::raros::resposta_shiny(estado, "")
            .await
            .map_err(|e| e.to_string())?;
        {
            let preparar = |mut v: Value| {
                for campo in ["dadosEm", "histEm", "geradoEm"] {
                    tirar(&mut v, campo);
                }
                // Contas com o mesmo valor podem sair em ordem diferente: compara por nick.
                if let Some(Value::Array(c)) = v.get_mut("contas") {
                    c.sort_by_key(|x| x.get("nick").map(Value::to_string).unwrap_or_default());
                }
                v
            };
            rel.entry("shiny")
                .or_default()
                .registrar("padrão", diferenca(&preparar(l), &preparar(n), ""));
        }
        Ok(())
    }

    async fn legado(&self, caminho: &str) -> Result<(u16, Value), String> {
        let r = self
            .http
            .get(format!("{}{caminho}", self.url))
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let status = r.status().as_u16();
        Ok((status, r.json().await.map_err(|e| e.to_string())?))
    }

    /// Uma rodada de comparação.
    pub async fn rodada(&self) -> Result<Relatorio, String> {
        let mut rel = Relatorio::new();
        let (_, mut mercado_legado) = self.legado("/api/bolsa/mercado").await?;
        // O legado guarda o mercado por 3 minutos e só remonta quando chega um pedido depois
        // disso. Com o relay ativo, as ofertas mudam nesse intervalo: espera o cache vencer e
        // pede de novo, para os dois lados montarem com os mesmos dados (diferença de segundos).
        let idade = mercado_legado
            .get("geradoEm")
            .and_then(Value::as_i64)
            .map_or(0, |g| super::agora_ms() - g);
        if idade > 10_000 {
            let espera = (CACHE_MERCADO_LEGADO_MS - idade).max(0) + 1_500;
            tokio::time::sleep(Duration::from_millis(u64::try_from(espera).unwrap_or(0))).await;
            mercado_legado = self.legado("/api/bolsa/mercado").await?.1;
        }
        let gerado_em = mercado_legado
            .get("geradoEm")
            .and_then(Value::as_i64)
            .ok_or("o legado não devolveu geradoEm")?;
        let s = self
            .servico
            .snapshot_em(gerado_em)
            .await
            .map_err(|e| e.to_string())?;
        let json = |v: &dyn erased::Serializar| v.valor();

        // /mercado: por ticker.
        let nosso = consultas::mercado(&s, &Params::default());
        let nossos: BTreeMap<String, Value> = nosso
            .tickers
            .iter()
            .map(|t| (t.id.clone(), json(t)))
            .collect();
        let deles: BTreeMap<String, Value> = mercado_legado
            .get("tickers")
            .and_then(Value::as_array)
            .map(|l| {
                l.iter()
                    .filter_map(|t| Some((t.get("id")?.as_str()?.to_owned(), t.clone())))
                    .collect()
            })
            .unwrap_or_default();
        let r = rel.entry("mercado").or_default();
        for (id, t) in &deles {
            match nossos.get(id) {
                Some(n) => r.registrar(id, diferenca(t, n, "")),
                None => {
                    r.so_legado += 1;
                    if r.exemplos.len() < 5 {
                        r.exemplos.push(format!("{id}: só no legado"));
                    }
                }
            }
        }
        r.so_nexus = nossos.keys().filter(|k| !deles.contains_key(*k)).count() as u64;
        r.registrar(
            "resumo",
            diferenca(
                mercado_legado.get("resumo").unwrap_or(&Value::Null),
                &json(&nosso.resumo),
                "",
            ),
        );

        // Amostra rotativa de tickers para as rotas por id.
        let ids: Vec<&String> = deles.keys().filter(|k| nossos.contains_key(*k)).collect();
        let passo = self
            .rodada
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let amostra: Vec<&String> = ids
            .iter()
            .cycle()
            .skip((passo * AMOSTRA) % ids.len().max(1))
            .take(AMOSTRA.min(ids.len()))
            .copied()
            .collect();
        for id in amostra {
            let q = format!("?id={}", urlencoding(id));
            let p = Params {
                id: Some(id.clone()),
                ..Default::default()
            };
            let (_, l) = self.legado(&format!("/api/bolsa/serie{q}")).await?;
            let n = consultas::serie(&s, &p)
                .map(|v| json(&v))
                .unwrap_or(Value::Null);
            rel.entry("serie")
                .or_default()
                .registrar(id, diferenca(&ordenar_serie(l), &ordenar_serie(n), ""));
            let (_, l) = self.legado(&format!("/api/bolsa/balao{q}")).await?;
            let n = consultas::balao(&s, &p)
                .map(|v| json(&v))
                .unwrap_or(Value::Null);
            rel.entry("balao")
                .or_default()
                .registrar(id, diferenca(&l, &n, ""));
            let (status, mut l) = self.legado(&format!("/api/bolsa/ticker{q}")).await?;
            // O legado quebra (500) nos tickers de item sem id (`i:n:...`): passa `NaN` para
            // o SQL. É defeito dele, não divergência do Nexus.
            if status >= 500 {
                for rota in ["ticker", "ticker.ofertas"] {
                    let r = rel.entry(rota).or_default();
                    if r.exemplos.len() < 5 {
                        r.exemplos
                            .push(format!("{id}: legado respondeu {status} (ignorado)"));
                    }
                }
                continue;
            }
            let mut n = consultas::ticker_completo(&self.servico, &s, &p)
                .await
                .map_err(|e| e.to_string())?
                .map(|v| json(&v))
                .unwrap_or(Value::Null);
            // A lista de ofertas é lida na hora em cada lado; compara à parte.
            let (lo, no) = (tirar(&mut l, "ofertas"), tirar(&mut n, "ofertas"));
            rel.entry("ticker")
                .or_default()
                .registrar(id, diferenca(&l, &n, ""));
            rel.entry("ticker.ofertas").or_default().registrar(
                id,
                diferenca(&ordenar_ofertas(lo), &ordenar_ofertas(no), ""),
            );
        }

        // /feedback e /dicas com os parâmetros padrão.
        let (_, l) = self.legado("/api/bolsa/feedback").await?;
        let n = json(&consultas::feedback(&self.servico, &s, &Params::default()));
        rel.entry("feedback")
            .or_default()
            .registrar("padrão", diferenca(&l, &n, ""));
        let (_, mut l) = self.legado("/api/bolsa/dicas").await?;
        let mut n = json(
            &consultas::dicas(&self.servico, &s, &Params::default())
                .await
                .map_err(|e| e.to_string())?,
        );
        let (lo, no) = (
            tirar(&mut l, "oportunidades"),
            tirar(&mut n, "oportunidades"),
        );
        rel.entry("dicas")
            .or_default()
            .registrar("padrão", diferenca(&l, &n, ""));

        // Ofertas: o legado só avalia pokémon já em cache; compara as que existem nos dois.
        comparar_ofertas(rel.entry("dicas.oportunidades").or_default(), &lo, &no);
        let (_, l) = self.legado("/api/bolsa/ofertas?limite=500").await?;
        let n = json(
            &consultas::ofertas(
                &self.servico,
                &s,
                &Params {
                    limite: Some("500".into()),
                    ..Default::default()
                },
            )
            .await
            .map_err(|e| e.to_string())?,
        );
        comparar_ofertas(
            rel.entry("ofertas").or_default(),
            l.get("ofertas").unwrap_or(&Value::Null),
            n.get("ofertas").unwrap_or(&Value::Null),
        );

        // /meus-itens: o inventário do legado muda pelo relay; compara a avaliação de cada
        // item presente nos dois, sem os campos que dependem da quantidade.
        let (_, l) = self.legado("/api/bolsa/meus-itens").await?;
        let inventario = self
            .servico
            .repo
            .meus_itens()
            .await
            .map_err(|e| e.to_string())?;
        let raros = match &self.estado {
            Some(e) => consultas::raros_nomes(&e.db).await,
            None => Default::default(),
        };
        let n = json(&consultas::meus_itens(
            &s,
            &Params::default(),
            &inventario,
            &raros,
        ));
        let por_item = |v: &Value| -> BTreeMap<i64, Value> {
            v.get("itens")
                .and_then(Value::as_array)
                .map(|l| {
                    l.iter()
                        .filter_map(|i| {
                            let mut i = i.clone();
                            let id = i.get("itemId")?.as_i64()?;
                            for campo in ["quantidade", "valor", "valorAlvo"] {
                                tirar(&mut i, campo);
                            }
                            Some((id, i))
                        })
                        .collect()
                })
                .unwrap_or_default()
        };
        let (li, ni) = (por_item(&l), por_item(&n));
        let r = rel.entry("meus-itens").or_default();
        li.iter().for_each(|(id, i)| match ni.get(id) {
            Some(x) => r.registrar(&id.to_string(), diferenca(i, x, "")),
            None => r.so_legado += 1,
        });
        r.so_nexus = ni.keys().filter(|k| !li.contains_key(*k)).count() as u64;

        if let Some(estado) = &self.estado {
            if let Err(e) = self.comparar_equipes(&estado.equipes, &mut rel).await {
                rel.entry("equipes.bosses")
                    .or_default()
                    .exemplos
                    .push(format!("falhou: {e}"));
            }
            if let Err(e) = self.comparar_raros(estado, &mut rel).await {
                rel.entry("raros2")
                    .or_default()
                    .exemplos
                    .push(format!("falhou: {e}"));
            }
            if let Err(e) = self.comparar_equipe_bolsa(estado, &mut rel).await {
                rel.entry("equipe")
                    .or_default()
                    .exemplos
                    .push(format!("falhou: {e}"));
            }
            if let Some(base) = &self.motor_url {
                if let Err(e) = self.comparar_motor(estado, base, &mut rel).await {
                    rel.entry("motor")
                        .or_default()
                        .exemplos
                        .push(format!("falhou: {e}"));
                }
            }
        }

        for (rota, r) in &rel {
            let rotulo =
                |res: &'static str| [("rota", (*rota).to_owned()), ("resultado", res.to_owned())];
            metrics::counter!("nexus_paridade_total", &rotulo("igual")).increment(r.iguais);
            metrics::counter!("nexus_paridade_total", &rotulo("diferente"))
                .increment(r.comparados - r.iguais);
            metrics::counter!("nexus_paridade_total", &rotulo("so_legado")).increment(r.so_legado);
            metrics::counter!("nexus_paridade_total", &rotulo("so_nexus")).increment(r.so_nexus);
            let razao = if r.comparados == 0 {
                1.0
            } else {
                r.iguais as f64 / r.comparados as f64
            };
            metrics::gauge!("nexus_paridade_razao", "rota" => *rota).set(razao);
        }
        Ok(rel)
    }

    /// Roda a cada `ciclo` e registra o resultado em métricas e log.
    pub fn iniciar(self: Arc<Self>, ciclo: Duration, parar: CancellationToken) {
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    () = parar.cancelled() => break,
                    () = tokio::time::sleep(ciclo) => {}
                }
                match self.rodada().await {
                    Ok(rel) => rel.iter().for_each(|(rota, r)| {
                        tracing::info!(
                            rota,
                            comparados = r.comparados,
                            iguais = r.iguais,
                            so_legado = r.so_legado,
                            so_nexus = r.so_nexus,
                            exemplos = ?r.exemplos,
                            "paridade"
                        );
                    }),
                    Err(erro) => {
                        metrics::counter!("nexus_paridade_falhas_total").increment(1);
                        tracing::warn!(%erro, "rodada de paridade falhou");
                    }
                }
            }
        });
    }
}

/// Relatório → linhas da tabela `nexus.paridade`.
pub fn linhas(rel: &Relatorio) -> Vec<persistencia::paridade::Linha> {
    let i = |n: u64| i32::try_from(n).unwrap_or(i32::MAX);
    rel.iter()
        .map(|(rota, r)| persistencia::paridade::Linha {
            rota: (*rota).to_owned(),
            comparados: i(r.comparados),
            iguais: i(r.iguais),
            so_legado: i(r.so_legado),
            so_nexus: i(r.so_nexus),
            exemplos: r.exemplos.clone(),
        })
        .collect()
}

/// Paridade por qualidade de `/api/bolsa/equipe`: mesmo boss e mesmas candidatas; em cada
/// perfil, dano do Nexus ≥ 99% do Node, restrições respeitadas e custo dentro de ±5%.
fn qualidade_equipe(l: &Value, n: &Value) -> Option<String> {
    if l.pointer("/boss/key") != n.pointer("/boss/key") {
        return Some(format!(
            "boss: {:?} ≠ {:?}",
            l.pointer("/boss/key"),
            n.pointer("/boss/key")
        ));
    }
    // O legado reaproveita um cálculo de até 5 minutos atrás (e cotações de até 30 min); com o
    // relay ativo, as ofertas mudam nesse meio tempo. Conjuntos de candidatas diferentes não
    // têm qualidade comparável (equipe e custo saem de outro conjunto): só compara com o
    // mesmo número de candidatas. Passando de 10% do conjunto,
    // é divergência de lógica e aparece.
    let (a, b) = (
        l.pointer("/candidatas")
            .and_then(Value::as_f64)
            .unwrap_or(0.0),
        n.pointer("/candidatas")
            .and_then(Value::as_f64)
            .unwrap_or(0.0),
    );
    if (a - b).abs() > 0.1 * a.max(b) {
        return Some(format!("candidatas: {a} ≠ {b} (mais de 10%)"));
    }
    if (a - b).abs() > 0.0 {
        return None;
    }
    let num = |v: &Value, p: &str| v.pointer(p).and_then(Value::as_f64);
    for perfil in ["eco", "eq", "max"] {
        let base = format!("/perfis/{perfil}");
        let indisp = |v: &Value| {
            v.pointer(&format!("{base}/indisponivel"))
                .and_then(Value::as_bool)
                .unwrap_or(true)
        };
        match (indisp(l), indisp(n)) {
            (true, true) => continue,
            (false, true) => return Some(format!("{perfil}: só o legado achou equipe")),
            (true, false) => continue,
            (false, false) => {}
        }
        let (dl, dn) = (
            num(l, &format!("{base}/poder")),
            num(n, &format!("{base}/poder")),
        );
        if let (Some(dl), Some(dn)) = (dl, dn) {
            if dn < 0.99 * dl {
                return Some(format!("{perfil}: poder {dn:.3} < 99% de {dl:.3}"));
            }
        }
        let restr = n
            .pointer(&format!("{base}/restr"))
            .and_then(Value::as_object);
        if let Some(r) = restr {
            let relaxadas: Vec<&str> = r
                .get("relaxadas")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            for (k, nome) in [
                ("piso", "piso"),
                ("heroi", "herói"),
                ("estresse", "estresse"),
            ] {
                if r.get(k) == Some(&Value::Bool(false))
                    && !relaxadas.contains(&nome)
                    && perfil != "max"
                {
                    return Some(format!("{perfil}: restrição {k} violada"));
                }
            }
        }
        if let (Some(cl), Some(cn)) = (
            num(l, &format!("{base}/total")),
            num(n, &format!("{base}/total")),
        ) {
            if cl > 0.0 && (cn - cl).abs() > 0.05 * cl {
                return Some(format!("{perfil}: custo {cn} fora de ±5% de {cl}"));
            }
        }
    }
    None
}

/// Ofertas de um ticker em ordem de (preço, id): a consulta do legado só ordena por preço, e
/// empates saem em qualquer ordem.
/// Tira `pokemons` e `shinies` do JSON e devolve as duas listas juntas.
fn cartas_de(v: &mut Value) -> Vec<Value> {
    let mut cartas = Vec::new();
    for campo in ["pokemons", "shinies"] {
        if let Value::Array(a) = tirar(v, campo) {
            cartas.extend(a);
        }
    }
    cartas
}

/// Compara as cartas: tudo igual, menos o preço (`valorGemas`, `faixa`, `confianca`), que
/// precisa estar a 10% de distância (mercados montados em instantes diferentes). Só olha as
/// 100 primeiras: depois do corte de preço, a ordem de cartas de valor parecido diverge.
fn comparar_cartas(l: &[Value], n: &[Value]) -> Option<String> {
    let chave = |c: &Value| format!("{}|{}", c["nick"], c["id"]);
    let por_chave: BTreeMap<String, &Value> = n.iter().map(|c| (chave(c), c)).collect();
    let tira_preco = |c: &Value| {
        let mut c = c.clone();
        for campo in ["valorGemas", "faixa", "confianca", "semPreco"] {
            tirar(&mut c, campo);
        }
        c
    };
    if l.len() != n.len() {
        return Some(format!("quantidade de cartas {} ≠ {}", l.len(), n.len()));
    }
    for c in l.iter().take(100) {
        let k = chave(c);
        let Some(outra) = por_chave.get(&k) else {
            return Some(format!("{k}: só no legado"));
        };
        if let Some(d) = diferenca(&tira_preco(c), &tira_preco(outra), &k) {
            return Some(d);
        }
        let (a, b) = (c["valorGemas"].as_f64(), outra["valorGemas"].as_f64());
        if let (Some(a), Some(b)) = (a, b) {
            if (a - b).abs() > 0.10 * a.abs().max(b.abs()) {
                return Some(format!("{k}: valorGemas {a} ≠ {b}"));
            }
        } else if a.is_some() != b.is_some() {
            return Some(format!("{k}: preço presente só em um lado"));
        }
    }
    None
}

/// Ofertas de um ticker em ordem de (preço, id): a consulta do legado só ordena por preço, e
/// empates saem em qualquer ordem.
fn ordenar_ofertas(mut v: Value) -> Value {
    if let Value::Array(a) = &mut v {
        let chave = |x: &Value| {
            (
                x.get("preco").and_then(Value::as_f64).unwrap_or(f64::NAN),
                x.get("id").and_then(Value::as_i64).unwrap_or(0),
            )
        };
        a.sort_by(|x, y| {
            let (px, ix) = chave(x);
            let (py, iy) = chave(y);
            px.total_cmp(&py).then(ix.cmp(&iy))
        });
    }
    v
}

/// Tira os campos que dependem do instante ou da coleção da conta. O estado do Nexus chega
/// pela importação, até 10 minutos depois do relay gravar no legado; o que fica compara o
/// motor (lista de bosses, equipe teórica, boss).
fn sem_campos_da_conta(mut v: Value) -> Value {
    if let Value::Object(o) = &mut v {
        o.remove("atualizadoEm");
        o.remove("minha");
        // Dependem dos bosses vencidos da conta, que chegam com atraso pela importação.
        o.remove("proximo");
        if let Some(Value::Array(b)) = o.get_mut("bosses") {
            for x in b.iter_mut() {
                if let Value::Object(x) = x {
                    x.remove("feito");
                }
            }
        }
        if let Some(Value::Object(c)) = o.get_mut("conta") {
            c.remove("em");
            c.remove("pokemons");
        }
        if let Some(Value::Array(eq)) = o.get_mut("ideal").and_then(|i| i.get_mut("equipe")) {
            for m in eq.iter_mut() {
                if let Value::Object(m) = m {
                    m.remove("possui");
                    m.remove("nivelQueTenho");
                }
            }
        }
    }
    v
}

fn comparar_ofertas(r: &mut ResultadoRota, legado: &Value, nexus: &Value) {
    let por_id = |v: &Value| -> BTreeMap<i64, Value> {
        v.as_array()
            .map(|l| {
                l.iter()
                    .filter_map(|o| Some((o.get("ofertaId")?.as_i64()?, o.clone())))
                    .collect()
            })
            .unwrap_or_default()
    };
    let (l, n) = (por_id(legado), por_id(nexus));
    let mut puladas: u64 = 0;
    l.iter().for_each(|(id, o)| match n.get(id) {
        Some(x) if cotacao_de_outro_instante(o, x) => puladas += 1,
        Some(x) => r.registrar(&id.to_string(), diferenca(o, x, "")),
        None => r.so_legado += 1,
    });
    r.so_nexus = n.keys().filter(|k| !l.contains_key(*k)).count() as u64;
    // Fica visível: se quase todas forem puladas sempre, a contagem de vendas do Nexus diverge.
    if puladas > 0 {
        metrics::counter!("nexus_paridade_ofertas_puladas_total").increment(puladas);
        tracing::info!(
            puladas,
            comparadas = r.comparados,
            "ofertas de pokémon fora da comparação: cotação com base de vendas diferente"
        );
    }
}

/// Dado ao vivo: o legado guarda a cotação de cada pokémon por 30 minutos e o Nexus cota em
/// outro instante. Se as vendas usadas (`comparaveis`) diferem, os dois partiram de bases
/// diferentes e a oferta não entra na comparação. Com a mesma base, qualquer diferença conta.
fn cotacao_de_outro_instante(legado: &Value, nexus: &Value) -> bool {
    let precificador = |o: &Value| o.get("fonte").and_then(Value::as_str) == Some("precificador");
    let base = |o: &Value| o.get("comparaveis").and_then(Value::as_f64);
    precificador(legado) && precificador(nexus) && base(legado) != base(nexus)
}

fn tirar(v: &mut Value, campo: &str) -> Value {
    v.as_object_mut()
        .and_then(|o| o.remove(campo))
        .unwrap_or(Value::Null)
}

/// Vendas no mesmo milissegundo saem em ordem arbitrária no legado: ordena antes de comparar.
fn ordenar_serie(mut v: Value) -> Value {
    if let Some(l) = v.get_mut("vendas").and_then(Value::as_array_mut) {
        l.sort_by(|a, b| {
            let chave = |x: &Value| {
                (
                    x.get(0).and_then(Value::as_i64).unwrap_or(0),
                    x.get(1).and_then(Value::as_f64).unwrap_or(0.0),
                )
            };
            let (a, b) = (chave(a), chave(b));
            a.0.cmp(&b.0).then(a.1.total_cmp(&b.1))
        });
    }
    v
}

fn urlencoding(texto: &str) -> String {
    texto
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b':' => {
                char::from(b).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Serializa qualquer DTO para `Value` (sem expor o tipo concreto).
mod erased {
    use serde::Serialize;
    use serde_json::Value;

    pub trait Serializar {
        fn valor(&self) -> Value;
    }

    impl<T: Serialize> Serializar for T {
        fn valor(&self) -> Value {
            serde_json::to_value(self).unwrap_or(Value::Null)
        }
    }
}

#[cfg(test)]
mod testes {
    use serde_json::json;

    use super::*;

    #[test]
    fn contador_de_bolas_tolera_atraso_e_sugestao_pendente_e_detectada() {
        let a = json!({"contas": [{"bolasDesde": 118441, "outro": 5}]});
        let b = json!({"contas": [{"bolasDesde": 118416, "outro": 5}]});
        assert_eq!(diferenca(&a, &b, ""), None);
        let c = json!({"contas": [{"bolasDesde": 118441, "outro": 6}]});
        assert!(diferenca(&c, &b, "").is_some());
        let longe = json!({"contas": [{"bolasDesde": 100000, "outro": 5}]});
        assert!(diferenca(&a, &longe, "").is_some());
        let l = json!({"falta": {"sugestao": null}});
        let n = json!({"falta": {"sugestao": {"custoAproximado": 1090}}});
        assert!(sugestao_pendente(&l, &n));
        assert!(!sugestao_pendente(&n, &n));
        assert!(!sugestao_pendente(&l, &l));
        let nota = json!({"falta": {"sugestao": {"nota": "Veja a aba Econômica"}}});
        assert!(sugestao_pendente(&nota, &n));
        assert!(!sugestao_pendente(&nota, &nota));
    }

    #[test]
    fn cache_do_legado_tolera_custo_de_oportunidade_e_conjuntos_diferentes() {
        let a = json!({"perfis": {"max": {"custoOportunidade": 111, "total": 5}}});
        let b = json!({"perfis": {"max": {"custoOportunidade": 122, "total": 5}}});
        assert_eq!(diferenca(&a, &b, ""), None);
        let c = json!({"perfis": {"max": {"custoOportunidade": 111, "total": 6}}});
        assert!(diferenca(&a, &c, "").is_some());
        let longe = json!({"perfis": {"max": {"custoOportunidade": 200, "total": 5}}});
        assert!(diferenca(&a, &longe, "").is_some());
        // Conjuntos de candidatas diferentes não são comparáveis; mesmo conjunto, é lógica.
        let perfil = |c: u32, tot: u32| {
            json!({"boss": {"key": "x"}, "candidatas": c,
                "perfis": {"max": {"indisponivel": false, "poder": 1.0, "total": tot}}})
        };
        assert_eq!(qualidade_equipe(&perfil(30, 1000), &perfil(31, 2000)), None);
        assert!(qualidade_equipe(&perfil(18, 1000), &perfil(31, 2000)).is_some());
        assert!(qualidade_equipe(&perfil(31, 1000), &perfil(31, 2000)).is_some());
        assert!(qualidade_equipe(&perfil(100, 1000), &perfil(200, 1000)).is_some());
    }

    #[test]
    fn dado_ao_vivo_tem_tolerancia_so_onde_deve() {
        let a = json!({"confianca": {"chance": 0.4989494951215404}, "ofStat": {"n": 100}, "poder": 1.0});
        let b = json!({"confianca": {"chance": 0.49961427578013706}, "ofStat": {"n": 103}, "poder": 1.0});
        assert_eq!(diferenca(&a, &b, ""), None);
        let c = json!({"confianca": {"chance": 0.4989494951215404}, "ofStat": {"n": 100}, "poder": 1.03});
        assert!(diferenca(&a, &c, "").is_some());
    }

    #[test]
    fn cotacao_com_base_diferente_fica_fora() {
        let of = |id: i64, base: f64, desconto: f64| json!({"ofertaId": id, "fonte": "precificador", "comparaveis": base, "desconto": desconto});
        let mut r = ResultadoRota::default();
        let l = json!([of(1, 35689.0, -0.28), of(2, 100.0, -0.1)]);
        let n = json!([of(1, 35710.0, -0.19), of(2, 100.0, -0.2)]);
        comparar_ofertas(&mut r, &l, &n);
        assert_eq!((r.comparados, r.iguais), (1, 0));
        let item = |d: f64| json!([{"ofertaId": 3, "fonte": "mediana 7d do item", "comparaveis": 5, "desconto": d}]);
        let mut r = ResultadoRota::default();
        comparar_ofertas(&mut r, &item(-0.1), &item(-0.3));
        assert_eq!((r.comparados, r.iguais), (1, 0));
    }

    #[test]
    fn diferenca_ignora_volateis_e_tolera_floats() {
        let a = json!({ "geradoEm": 1, "x": 1.0, "y": [1, 2], "z": null });
        let b = json!({ "geradoEm": 2, "x": 1.000_000_000_1, "y": [1, 2] });
        assert_eq!(diferenca(&a, &b, ""), None);
        let c = json!({ "x": 1.1, "y": [1, 2] });
        assert!(diferenca(&a, &c, "").is_some_and(|d| d.starts_with(".x")));
    }
}
