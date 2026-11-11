//! Ingestão da bolsa: validação das ofertas, do resumo de itens e do inventário.
//!
//! As regras seguem o `bolsa.js`, com duas correções (documentadas em `docs/migracao.md`):
//! - preço, id e quantidade precisam ser inteiros. No legado um valor fracionário fazia o
//!   `INSERT` inteiro falhar e o lote todo se perdia; aqui só a oferta inválida é descartada;
//! - `completo: true` desativa só as ofertas dos pares (tipo, moeda) presentes no lote. No
//!   legado desativava tudo o que não veio, inclusive de outro tipo de coleta.

use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;

use serde_json::{Map, Value};

use super::catalogo::Catalogo;
use super::repo::ErroRepo;
use crate::precificador::ficha::numero_js;

pub const MAX_OFERTAS_POR_LOTE: usize = 5000;
pub const MAX_ITENS_INVENTARIO: usize = 2000;

#[derive(Debug, Clone, PartialEq)]
pub struct OfertaNova {
    pub id: i64,
    pub tipo: String,
    pub moeda: String,
    pub preco: i64,
    pub especie: Option<i32>,
    pub item_id: Option<i32>,
    pub quantidade: i32,
    pub nome: String,
    pub ficha: Option<Value>,
    pub expira_em: Option<i64>,
}

/// `Number.isInteger(+v) && v != null && v !== ''`, devolvendo o inteiro.
fn inteiro(v: Option<&Value>) -> Option<i64> {
    let v = v.filter(|v| !v.is_null() && v.as_str() != Some(""))?;
    let n = numero_js(v);
    (n.is_finite() && n.fract() == 0.0 && n.abs() < 9.007_199_254_740_992e15).then_some(n as i64)
}

/// `a ?? b`: o primeiro campo presente e não nulo.
fn primeiro<'v>(r: &'v Value, chaves: &[&str]) -> Option<&'v Value> {
    chaves
        .iter()
        .find_map(|k| r.get(*k).filter(|v| !v.is_null()))
}

fn texto_js(v: Option<&Value>) -> Option<String> {
    match v? {
        Value::Null => None,
        Value::String(s) if s.is_empty() => None,
        Value::String(s) => Some(s.clone()),
        Value::Bool(false) => None,
        Value::Number(n) if n.as_f64() == Some(0.0) => None,
        outro => Some(outro.to_string()),
    }
}

/// Lote de ofertas validado.
#[derive(Debug, Clone, PartialEq)]
pub struct LoteOfertas {
    pub recebidas: usize,
    /// Sem ids repetidos (vale a última ocorrência).
    pub ofertas: Vec<OfertaNova>,
    pub validas: usize,
    /// Pares (tipo, moeda) a desativar se a coleta veio completa.
    pub escopo_completo: Option<BTreeSet<(String, String)>>,
}

pub fn validar_ofertas(lista: &Value, completo: &Value, catalogo: &Catalogo) -> LoteOfertas {
    let lista: &[Value] = lista
        .as_array()
        .map_or(&[], |l| &l[..l.len().min(MAX_OFERTAS_POR_LOTE)]);
    let validas: Vec<OfertaNova> = lista
        .iter()
        .filter_map(|r| validar_oferta(r, catalogo))
        .collect();
    let n_validas = validas.len();
    // Dedupe por id: a última ocorrência vence.
    let por_id: BTreeMap<i64, OfertaNova> = validas.into_iter().map(|o| (o.id, o)).collect();
    let ofertas: Vec<OfertaNova> = por_id.into_values().collect();
    let escopo_completo =
        (matches!(completo, Value::Bool(true)) && !ofertas.is_empty()).then(|| {
            ofertas
                .iter()
                .map(|o| (o.tipo.clone(), o.moeda.clone()))
                .collect()
        });
    LoteOfertas {
        recebidas: lista.len(),
        ofertas,
        validas: n_validas,
        escopo_completo,
    }
}

fn validar_oferta(r: &Value, catalogo: &Catalogo) -> Option<OfertaNova> {
    let id = inteiro(r.get("id")).filter(|i| *i > 0)?;
    let preco = inteiro(r.get("preco")).filter(|p| *p > 0)?;
    let ficha = r.get("ficha").and_then(Value::as_object).map(|f| {
        let mut f: Map<String, Value> = f.clone();
        f.remove("apelido");
        Value::Object(f)
    });
    let especie = inteiro(
        primeiro(r, &["speciesId", "species_id"])
            .or_else(|| ficha.as_ref().and_then(|f| f.get("speciesId"))),
    )
    .and_then(|n| i32::try_from(n).ok());
    let item_id = inteiro(primeiro(r, &["itemId", "item_id"])).and_then(|n| i32::try_from(n).ok());
    let tipo = if r.get("tipo").and_then(Value::as_str) == Some("item") {
        "item"
    } else if ficha.is_some() || especie.is_some_and(|s| s != 0) {
        "pokemon"
    } else {
        "item"
    };
    let moeda: String = texto_js(r.get("moeda"))
        .unwrap_or_else(|| "gold".into())
        .chars()
        .take(10)
        .collect();
    let quantidade = primeiro(r, &["quantidade", "qtd"])
        .map(numero_js)
        .filter(|q| q.is_finite() && *q != 0.0)
        .unwrap_or(1.0)
        .max(1.0)
        .floor()
        .min(f64::from(i32::MAX)) as i32;
    let nome: String = texto_js(r.get("nome"))
        .or_else(|| texto_js(ficha.as_ref().and_then(|f| f.get("nome"))))
        .or_else(|| item_id.and_then(|i| catalogo.item(i).map(|c| c.nome.clone())))
        .unwrap_or_default()
        .chars()
        .take(120)
        .collect();
    let expira = r.get("expiraEm").map(numero_js).filter(|e| *e > 1e12);
    Some(OfertaNova {
        id,
        tipo: tipo.into(),
        moeda,
        preco,
        especie,
        item_id,
        quantidade,
        nome,
        ficha,
        expira_em: expira.map(|e| crate::js::round(e) as i64),
    })
}

#[derive(Debug, Clone, PartialEq)]
pub struct LinhaResumo {
    pub item_id: i32,
    pub moeda: &'static str,
    pub anuncios: i32,
    pub preco_min: i64,
}

/// `resumo` → linhas (gold e orb por item). Devolve (itens recebidos, linhas válidas).
pub fn validar_resumo(resumo: &Value) -> (usize, Vec<LinhaResumo>) {
    let Some(mapa) = resumo.as_object() else {
        return (0, vec![]);
    };
    let linhas = mapa
        .iter()
        .filter_map(|(id, r)| {
            {
                let id = inteiro(Some(&Value::String(id.clone())))?;
                i32::try_from(id).ok()
            }
            .map(|id| (id, r))
        })
        .flat_map(|(id, r)| {
            [("gold", "minGold"), ("orb", "minOrb")]
                .into_iter()
                .filter_map(move |(moeda, campo)| {
                    let min = inteiro(r.get(campo)).filter(|m| *m > 0)?;
                    let anuncios = r
                        .get("anuncios")
                        .map(numero_js)
                        .filter(|a| a.is_finite() && *a != 0.0)
                        .unwrap_or(0.0);
                    Some(LinhaResumo {
                        item_id: id,
                        moeda,
                        anuncios: anuncios
                            .trunc()
                            .clamp(f64::from(i32::MIN), f64::from(i32::MAX))
                            as i32,
                        preco_min: min,
                    })
                })
        })
        .collect();
    (mapa.len(), linhas)
}

/// Inventário de itens: item → quantidade.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MeusItens {
    pub ts: Option<i64>,
    pub itens: BTreeMap<i32, i64>,
}

/// Aplica um `POST /api/bolsa/meus-itens` ao inventário atual.
pub fn aplicar_meus_itens(
    mut atual: MeusItens,
    itens: &Value,
    limpar: &Value,
    somar: &Value,
    agora: i64,
) -> MeusItens {
    if matches!(limpar, Value::Bool(true)) {
        atual.itens.clear();
    }
    let somar = !matches!(somar, Value::Null | Value::Bool(false))
        && !(somar.as_f64() == Some(0.0))
        && somar.as_str() != Some("");
    let pares: Vec<(Value, Value)> = match itens {
        Value::Array(l) => l
            .iter()
            .map(|r| {
                (
                    r.get("itemId").cloned().unwrap_or(Value::Null),
                    r.get("quantidade").cloned().unwrap_or(Value::Null),
                )
            })
            .collect(),
        Value::Object(m) => m
            .iter()
            .map(|(k, v)| (Value::String(k.clone()), v.clone()))
            .collect(),
        _ => vec![],
    };
    pares
        .into_iter()
        .take(MAX_ITENS_INVENTARIO)
        .for_each(|(id, q)| {
            let id = numero_js(&id);
            let q = numero_js(&q).floor();
            if id.is_nan()
                || id <= 0.0
                || !q.is_finite()
                || id.fract() != 0.0
                || id > f64::from(i32::MAX)
            {
                return;
            }
            let id = id as i32;
            if q <= 0.0 {
                atual.itens.remove(&id);
            } else {
                let q = q as i64;
                let novo = if somar {
                    atual.itens.get(&id).copied().unwrap_or(0).saturating_add(q)
                } else {
                    q
                };
                atual.itens.insert(id, novo);
            }
        });
    atual.ts = Some(agora);
    atual
}

/// Porta de escrita da bolsa. Em produção fica desligada até o cutover.
pub trait RepoIngestao: Send + Sync {
    /// Upsert das ofertas (pelo id). Com `desativar`, marca inativas as ofertas dos pares
    /// (tipo, moeda) dados que não foram vistas em `agora`.
    fn gravar_ofertas(
        &self,
        ofertas: &[OfertaNova],
        agora: i64,
        desativar: Option<&BTreeSet<(String, String)>>,
    ) -> impl Future<Output = Result<u64, ErroRepo>> + Send;

    fn gravar_resumo(
        &self,
        linhas: &[LinhaResumo],
        ts: i64,
    ) -> impl Future<Output = Result<u64, ErroRepo>> + Send;

    /// Marca inativas as ofertas expiradas ou não vistas há 6 horas.
    fn limpar_ofertas(&self, agora: i64) -> impl Future<Output = Result<u64, ErroRepo>> + Send;

    fn meus_itens(&self) -> impl Future<Output = Result<MeusItens, ErroRepo>> + Send;

    fn gravar_meus_itens(
        &self,
        itens: &MeusItens,
    ) -> impl Future<Output = Result<(), ErroRepo>> + Send;
}

#[cfg(test)]
mod testes {
    use serde_json::json;

    use super::*;

    #[test]
    fn oferta_segue_as_regras_do_legado() {
        let lote = validar_ofertas(
            &json!([
                { "id": "10", "preco": 500, "moeda": "orb", "ficha": { "speciesId": 25, "apelido": "x", "nome": "Pikachu" } },
                { "id": 11, "preco": 0 },
                { "id": 12, "preco": 1.5 },
                { "id": 13, "preco": 7, "itemId": 4, "qtd": "3" },
                { "id": 10, "preco": 600, "moeda": "orb", "ficha": { "speciesId": 25 } },
            ]),
            &json!(true),
            &Catalogo::default(),
        );
        assert_eq!(lote.recebidas, 5);
        assert_eq!(lote.validas, 3);
        assert_eq!(lote.ofertas.len(), 2);
        let p = &lote.ofertas[0];
        assert_eq!(
            (p.id, p.preco, p.tipo.as_str(), p.especie),
            (10, 600, "pokemon", Some(25))
        );
        assert!(p.ficha.as_ref().is_some_and(|f| f.get("apelido").is_none()));
        let i = &lote.ofertas[1];
        assert_eq!(
            (i.tipo.as_str(), i.moeda.as_str(), i.quantidade),
            ("item", "gold", 3)
        );
        let escopo = lote.escopo_completo.unwrap();
        assert!(escopo.contains(&("pokemon".into(), "orb".into())));
        assert!(escopo.contains(&("item".into(), "gold".into())));
    }

    #[test]
    fn resumo_gera_linhas_por_moeda() {
        let (n, l) = validar_resumo(
            &json!({ "4": { "minOrb": 3, "minGold": 900, "anuncios": 2 }, "x": { "minOrb": 1 }, "5": { "minOrb": 0 } }),
        );
        assert_eq!(n, 3);
        assert_eq!(l.len(), 2);
    }

    #[test]
    fn meus_itens_define_soma_e_remove() {
        let m = aplicar_meus_itens(
            MeusItens::default(),
            &json!({ "4": 2, "5": 3 }),
            &Value::Null,
            &Value::Null,
            1,
        );
        let m = aplicar_meus_itens(
            m,
            &json!([{ "itemId": 4, "quantidade": 5 }, { "itemId": 5, "quantidade": 0 }]),
            &Value::Null,
            &json!(true),
            2,
        );
        assert_eq!(m.itens.into_iter().collect::<Vec<_>>(), vec![(4, 7)]);
        assert_eq!(m.ts, Some(2));
    }
}
