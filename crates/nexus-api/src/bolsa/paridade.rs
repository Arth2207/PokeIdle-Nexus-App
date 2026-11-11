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
const AMOSTRA: usize = 15;

/// Igualdade numérica com tolerância relativa (as funções `exp`/`ln` do V8 e da libm podem
/// divergir no último bit).
fn perto(a: f64, b: f64) -> bool {
    a == b || (a - b).abs() <= 1e-9 || (a - b).abs() <= 1e-6 * a.abs().max(b.abs())
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
            let ok = perto(x, y) || (tempo && (x - y).abs() <= 1e-4 * x.abs().max(y.abs()));
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
    equipes: Option<Arc<crate::equipes::ServicoEquipes>>,
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
            equipes: None,
        })
    }

    /// Liga a comparação das rotas `/api/equipes/*`.
    pub fn com_equipes(mut self, equipes: Arc<crate::equipes::ServicoEquipes>) -> Self {
        self.equipes = Some(equipes);
        self
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
        let marca = |v: &Value| {
            (
                v.get("atualizadoEm").cloned(),
                v.pointer("/conta/em").cloned(),
            )
        };
        if marca(&l) != marca(&n) {
            let r = rel.entry("equipes.bosses").or_default();
            r.exemplos
                .push("estado do Nexus defasado; comparação pulada nesta rodada".into());
            return Ok(());
        }
        rel.entry("equipes.bosses")
            .or_default()
            .registrar("padrão", diferenca(&l, &n, ""));

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
                diferenca(&l, &n.1, "")
            } else {
                Some(format!("status {status} ≠ {}", n.0))
            };
            r.registrar(&q, diff);
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
        let (_, mercado_legado) = self.legado("/api/bolsa/mercado").await?;
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
            let (_, mut l) = self.legado(&format!("/api/bolsa/ticker{q}")).await?;
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
            rel.entry("ticker.ofertas")
                .or_default()
                .registrar(id, diferenca(&lo, &no, ""));
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
        let n = json(&consultas::meus_itens(&s, &Params::default(), &inventario));
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

        if let Some(eq) = &self.equipes {
            if let Err(e) = self.comparar_equipes(eq, &mut rel).await {
                rel.entry("equipes.bosses")
                    .or_default()
                    .exemplos
                    .push(format!("falhou: {e}"));
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
    l.iter().for_each(|(id, o)| match n.get(id) {
        Some(x) => r.registrar(&id.to_string(), diferenca(o, x, "")),
        None => r.so_legado += 1,
    });
    r.so_nexus = n.keys().filter(|k| !l.contains_key(*k)).count() as u64;
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
    fn diferenca_ignora_volateis_e_tolera_floats() {
        let a = json!({ "geradoEm": 1, "x": 1.0, "y": [1, 2], "z": null });
        let b = json!({ "geradoEm": 2, "x": 1.000_000_000_1, "y": [1, 2] });
        assert_eq!(diferenca(&a, &b, ""), None);
        let c = json!({ "x": 1.1, "y": [1, 2] });
        assert!(diferenca(&a, &c, "").is_some_and(|d| d.starts_with(".x")));
    }
}
