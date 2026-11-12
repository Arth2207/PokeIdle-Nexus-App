//! Raros e shiny no `nexus-api`: monta as respostas de `GET /api/raros2` e `GET /api/shiny`
//! a partir do estado (`nexus.estado_raros`), do estado de equipes e do mercado.
//!
//! A nota de uma carta é a `nota` que a ficha já traz ou, sem ela, a `notaFicha` do legado
//! (porte em `nucleo::precificador::nota`). Sem o catálogo de espécies (bolsa fora do ar), a
//! nota fica nula, como no legado quando o motor falha.

use std::sync::Arc;
use std::time::Duration;

use nucleo::bolsa::ofertas;
use nucleo::bolsa::repo::OfertaBruta;
use nucleo::equipes::MotorEquipes;
use nucleo::js;
use nucleo::precificador::especie::Central;
use nucleo::precificador::nota;
use nucleo::raros::{self as nr, PedidoRaros2, Valiosos};
use serde_json::{json, Value};

use crate::bolsa::agora_ms;
use crate::bolsa::servico::Snapshot;
use crate::equipes::ErroServico;
use crate::estado::Estado;

/// Quanto tempo as cartas valiosas de uma conta ficam em cache (como no legado).
const VALIDADE: Duration = Duration::from_secs(10 * 60);
/// Quantas cartas de uma conta recebem preço (as demais só contam no total).
const MAX_CARTAS: usize = 600;
const TRINTA_DIAS_MS: i64 = 30 * 86_400_000;

#[derive(Debug, thiserror::Error)]
pub enum ErroRaros {
    #[error("banco indisponível: {0}")]
    Banco(#[from] sqlx::Error),
    #[error(transparent)]
    Equipes(#[from] ErroServico),
    #[error("falha no cálculo")]
    Calculo,
}

/// Cache das cartas valiosas por conta.
pub struct ServicoRaros {
    cache: moka::sync::Cache<String, Arc<Valiosos>>,
}

impl ServicoRaros {
    pub fn novo() -> Self {
        Self {
            cache: moka::sync::Cache::builder()
                .max_capacity(30)
                .time_to_live(VALIDADE)
                .build(),
        }
    }
}

/// `espDeFicha`: a espécie da ficha pelo `speciesId`, pelo `looktype` ou pelo nome.
fn especie_da_ficha<'m>(
    motor: &'m MotorEquipes,
    f: &Value,
) -> Option<&'m nucleo::combate::especie::EspecieCombate> {
    let por_id = |campo: &str| {
        let n = js::mais(f.get(campo));
        (n.is_finite() && n.fract() == 0.0)
            .then(|| motor.dex.por_id(n as i64))
            .flatten()
    };
    por_id("speciesId")
        .or_else(|| por_id("looktype"))
        .or_else(|| {
            let nome = f.get("nome").filter(|v| js::verdadeiro(Some(v)))?;
            motor.dex.por_nome(&js::texto(Some(nome)))
        })
}

fn verdade(v: Option<&Value>) -> bool {
    js::verdadeiro(v)
}

/// A ficha enriquecida com `nota`, `nome` e `ivTotal` (o `todos` do `pokemonsValiosos`).
fn enriquecer(motor: &MotorEquipes, central: Option<&Central>, p: &Value) -> Value {
    let sp = especie_da_ficha(motor, p);
    let mut o = p.as_object().cloned().unwrap_or_default();
    let nota = p
        .get("nota")
        .filter(|v| !v.is_null())
        .cloned()
        .or_else(|| {
            central
                .and_then(|c| nota::nota_ficha(c, p))
                .map(js::json_num)
        })
        .unwrap_or(Value::Null);
    o.insert("nota".into(), nota);
    let nome = p
        .get("nome")
        .filter(|v| verdade(Some(v)))
        .cloned()
        .or_else(|| sp.map(|s| Value::from(s.nome.clone())))
        .unwrap_or_else(|| Value::from("?"));
    o.insert("nome".into(), nome);
    let iv = p
        .get("ivTotal")
        .filter(|v| !v.is_null())
        .cloned()
        .unwrap_or_else(|| {
            match p
                .get("ivs")
                .filter(|v| verdade(Some(v)))
                .and_then(Value::as_object)
            {
                Some(m) => {
                    let soma: f64 = m
                        .values()
                        .map(|v| {
                            let x = js::mais(Some(v));
                            if x.is_nan() || x == 0.0 {
                                0.0
                            } else {
                                x
                            }
                        })
                        .sum();
                    js::json_num(soma)
                }
                None => Value::Null,
            }
        });
    o.insert("ivTotal".into(), iv);
    Value::Object(o)
}

/// O `pokemonsValiosos` do legado para uma conta, sem cache.
fn calcular(
    motor: &MotorEquipes,
    snap: Option<&Snapshot>,
    conta: Option<&Value>,
    nick: &str,
    nota_min: f64,
    agora: i64,
) -> Valiosos {
    let fichas = conta
        .and_then(|c| c.get("pokemons"))
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice);
    let todos: Vec<Value> = fichas
        .iter()
        .filter(|p| verdade(Some(p)) && p.get("slot").is_none_or(Value::is_null))
        .map(|p| enriquecer(motor, snap.map(|s| &*s.mercado.central), p))
        .collect();
    let nota_de = |p: &Value| js::mais(p.get("nota"));
    let potencia_5 = |p: &Value| js::mais(p.get("potencia")) == 5.0;
    let eh_shiny = |p: &Value| p.get("shiny") == Some(&Value::Bool(true));
    let lista: Vec<&Value> = todos
        .iter()
        .filter(|p| nota_de(p) >= nota_min || eh_shiny(p) || potencia_5(p))
        .collect();
    let quase = todos
        .iter()
        .filter(|p| {
            let n = nota_de(p);
            n < nota_min && n >= nota_min - 0.3 && !eh_shiny(p) && !potencia_5(p)
        })
        .count();

    // Preço de todas as cartas de uma vez: monta o avaliador uma vez só.
    let candidatas: Vec<&Value> = lista.iter().copied().take(MAX_CARTAS).collect();
    let valores = snap.map(|s| {
        let brutas: Vec<OfertaBruta> = candidatas
            .iter()
            .enumerate()
            .map(|(i, f)| OfertaBruta {
                id: i as i64,
                ts_visto: agora,
                tipo: Some("pokemon".into()),
                moeda: Some("orb".into()),
                preco: Some(1),
                especie: None,
                item_id: None,
                quantidade: Some(1),
                nome: f.get("nome").and_then(Value::as_str).map(str::to_owned),
                ficha: Some((*f).clone()),
            })
            .collect();
        let pre = ofertas::precalcular(&s.mercado, &brutas, agora);
        ofertas::avaliar(&s.mercado, &brutas, &pre)
    });

    let mut saida: Vec<Value> = candidatas
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let sp = especie_da_ficha(motor, p);
            let v = valores
                .as_ref()
                .and_then(|v| v.iter().find(|a| a.oferta_id == i as i64))
                .and_then(|a| a.faixa.map(|f| (a.justo, f, a.confianca.as_ref().map(|c| c.percent))));
            let nivel = js::mais(p.get("level"));
            let nulo = |campo: &str| p.get(campo).cloned().unwrap_or(Value::Null);
            json!({
                "id": nulo("id"), "nick": nick, "nome": nulo("nome"),
                "sprite": sp.map_or(Value::Null, |s| motor.sprite(Some(s), eh_shiny(p))),
                "nivel": if nivel.is_nan() || nivel == 0.0 { Value::Null } else { js::json_num(nivel) },
                "nota": nulo("nota"), "quality": nulo("quality"), "potencia": nulo("potencia"),
                "ivTotal": nulo("ivTotal"), "shiny": verdade(p.get("shiny")),
                "valorGemas": v.map_or(Value::Null, |v| js::json_num(js::round(v.0))),
                "faixa": v.map_or(Value::Null, |v| json!([js::json_num(v.1[0]), js::json_num(v.1[1])])),
                "confianca": v.and_then(|v| v.2).map_or(Value::Null, js::json_num),
                "semPreco": v.is_none(),
            })
        })
        .collect();
    let valor = |x: &Value| x.get("valorGemas").and_then(Value::as_f64).unwrap_or(0.0);
    saida.sort_by(|a, b| valor(b).total_cmp(&valor(a)));
    Valiosos {
        lista: saida,
        total: lista.len(),
        quase,
    }
}

/// `GET /api/raros2`. `restrito`: a conta a que o token está preso (só ela aparece).
pub async fn resposta_raros2(
    estado: &Estado,
    nick: &str,
    nota_min: f64,
    meta: f64,
    restrito: Option<&str>,
) -> Result<Value, ErroRaros> {
    let agora = agora_ms();
    let raros = persistencia::raros::carregar(&estado.db).await?;
    let eq = estado.equipes.estado().await?;
    let motor = estado.equipes.motor().await?;
    // Sem mercado montado, as cartas saem sem preço (como o legado com a bolsa fora).
    let snap = estado.bolsa.snapshot().await.ok();

    let mut contas = nr::contas_do_cofre(&raros, &eq.contas);
    if let Some(r) = restrito {
        contas.retain(|c| c.eq_ignore_ascii_case(r));
    }
    let mut por_conta = std::collections::HashMap::new();
    for c in &contas {
        let dados = eq.conta(Some(c));
        let chave = format!(
            "{}|{}|{}|{}",
            c.to_lowercase(),
            js::numero(nota_min),
            dados
                .and_then(|d| d.get("em"))
                .map_or(0.0, |e| js::mais(Some(e))),
            dados
                .and_then(|d| d.get("pokemons"))
                .and_then(Value::as_array)
                .map_or(0, Vec::len),
        );
        let v = match estado.raros.cache.get(&chave) {
            Some(v) => v,
            None => {
                let (motor, snap, c2, dados) =
                    (motor.clone(), snap.clone(), c.clone(), dados.cloned());
                let v = tokio::task::spawn_blocking(move || {
                    calcular(
                        &motor,
                        snap.as_deref(),
                        dados.as_ref(),
                        &c2,
                        nota_min,
                        agora,
                    )
                })
                .await
                .map_err(|_| ErroRaros::Calculo)?;
                let v = Arc::new(v);
                estado.raros.cache.insert(chave, v.clone());
                v
            }
        };
        por_conta.insert(c.clone(), v);
    }
    let historico = persistencia::raros::historico_cofre(&estado.db, agora - TRINTA_DIAS_MS).await;
    let icone = |id: i64| {
        let snap = snap.as_ref()?;
        snap.mercado.icone_item(i32::try_from(id).ok())
    };
    let valiosos = |c: &str| por_conta.get(c).map(|v| (**v).clone()).unwrap_or_default();
    let pedido = PedidoRaros2 {
        nick,
        nota_min,
        meta,
        agora,
    };
    let mut v = nr::raros2(&raros, &contas, &valiosos, &historico, &icone, &pedido);
    if let Some(r) = restrito {
        so_da_conta(&mut v, r);
    }
    Ok(v)
}

/// O item traz as quantidades de todas as contas: token de conta só vê a própria.
fn so_da_conta(v: &mut Value, conta: &str) {
    if let Some(itens) = v.get_mut("itens").and_then(Value::as_array_mut) {
        for item in itens {
            if let Some(m) = item.get_mut("contas").and_then(Value::as_object_mut) {
                m.retain(|k, _| k.eq_ignore_ascii_case(conta));
            }
        }
    }
}

/// `GET /api/shiny`.
pub async fn resposta_shiny(estado: &Estado, nick: &str) -> Result<Value, ErroRaros> {
    let raros = persistencia::raros::carregar(&estado.db).await?;
    let captura = persistencia::raros::captura_shiny(&estado.db).await;
    Ok(nr::shiny(&raros, nick, captura))
}

#[cfg(test)]
mod testes_conta {
    use serde_json::json;

    use super::*;

    #[test]
    fn token_de_conta_so_ve_a_propria() {
        let mut v = json!({"itens": [{"contas": {"Ana": 2, "bia": 1}}, {"id": 1}]});
        so_da_conta(&mut v, "ana");
        assert_eq!(v["itens"][0]["contas"], json!({"Ana": 2}));
    }
}
