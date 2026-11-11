//! Ficha de um pokémon (o JSON `ficha` das vendas e ofertas), lida com as mesmas regras de
//! validação do legado.

use serde_json::Value;

use super::especie::Atributos;

const STATS: [&str; 6] = ["hp", "atk", "def", "spAtk", "spDef", "speed"];

/// Campos da ficha que o precificador usa. Campos com tipo errado viram `None`, como as
/// checagens `Number.isInteger`/`Number.isFinite` do JS.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Ficha {
    pub especie: Option<i32>,
    pub nome: Option<String>,
    pub ivs: Option<[i32; 6]>,
    pub qualidade: Option<f64>,
    pub potencia: Option<i32>,
    pub nivel: Option<f64>,
    /// `shiny === true`.
    pub shiny: bool,
    /// `shiny` aceito como `true`, `1` ou `"true"` (usado no cálculo de poder).
    pub shiny_frouxo: bool,
    pub tm_elemental: Option<String>,
    pub tm_aoe: bool,
    pub nota: Option<f64>,
    /// `refinoTotal > 0` ou algum refino > 0 (nos seis status).
    pub refinada: bool,
    /// `refinoTotal > 0` ou qualquer valor de `refino` > 0 (critério das ofertas).
    pub refinada_qualquer: bool,
    pub iv_total: Option<Value>,
    /// Valores crus, repassados nas respostas.
    pub nivel_cru: Option<Value>,
    pub qualidade_cru: Option<Value>,
    pub potencia_cru: Option<Value>,
    pub shiny_cru: Option<Value>,
}

/// `Number.isInteger(v)`.
fn inteiro(v: &Value) -> Option<i64> {
    let n = v.as_f64()?;
    (n.is_finite() && n.fract() == 0.0 && n.abs() < 9.007_199_254_740_992e15).then_some(n as i64)
}

/// `Number(v)` do JS, para os casos que aparecem nas fichas.
pub fn numero_js(v: &Value) -> f64 {
    match v {
        Value::Number(n) => n.as_f64().unwrap_or(f64::NAN),
        Value::Bool(b) => f64::from(u8::from(*b)),
        Value::Null => 0.0,
        Value::String(s) => {
            let s = s.trim();
            if s.is_empty() {
                0.0
            } else {
                s.parse().unwrap_or(f64::NAN)
            }
        }
        _ => f64::NAN,
    }
}

/// `v === true || v === 1 || v === 'true'`.
pub fn sim(v: Option<&Value>) -> bool {
    match v {
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64() == Some(1.0),
        Some(Value::String(s)) => s == "true",
        _ => false,
    }
}

/// Valor "verdadeiro" no sentido do JS (string não vazia, número diferente de zero...).
fn verdadeiro(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|x| x != 0.0 && !x.is_nan()),
        Some(Value::String(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

impl Ficha {
    pub fn de_json(v: &Value) -> Self {
        let campo = |k: &str| v.get(k);
        let ivs = campo("ivs").and_then(|ivs| {
            let lidos: Vec<i32> = STATS
                .iter()
                .filter_map(|k| {
                    ivs.get(*k)
                        .and_then(inteiro)
                        .and_then(|n| i32::try_from(n).ok())
                })
                .collect();
            <[i32; 6]>::try_from(lidos).ok()
        });
        let refino_algum = campo("refino").and_then(Value::as_object).is_some_and(|r| {
            STATS
                .iter()
                .any(|k| r.get(*k).is_some_and(|x| numero_js(x) > 0.0))
        });
        let total = campo("refinoTotal").is_some_and(|x| numero_js(x) > 0.0);
        let refinada = total || refino_algum;
        let refinada_qualquer = total
            || campo("refino")
                .and_then(Value::as_object)
                .is_some_and(|r| r.values().any(|x| numero_js(x) > 0.0));
        Self {
            especie: campo("speciesId")
                .and_then(inteiro)
                .and_then(|n| i32::try_from(n).ok()),
            nome: campo("nome").and_then(Value::as_str).map(str::to_owned),
            ivs,
            qualidade: campo("quality")
                .and_then(Value::as_f64)
                .filter(|q| q.is_finite()),
            potencia: campo("potencia")
                .and_then(inteiro)
                .and_then(|n| i32::try_from(n).ok()),
            nivel: campo("level").and_then(Value::as_f64),
            shiny: matches!(campo("shiny"), Some(Value::Bool(true))),
            shiny_frouxo: sim(campo("shiny")),
            tm_elemental: verdadeiro(campo("tmElemental")).then(|| match campo("tmElemental") {
                Some(Value::String(s)) => s.clone(),
                Some(outro) => outro.to_string(),
                None => String::new(),
            }),
            tm_aoe: sim(campo("tmAoe")),
            nota: campo("nota")
                .and_then(Value::as_f64)
                .filter(|n| n.is_finite()),
            refinada,
            refinada_qualquer,
            iv_total: campo("ivTotal").cloned(),
            nivel_cru: campo("level").cloned(),
            qualidade_cru: campo("quality").cloned(),
            potencia_cru: campo("potencia").cloned(),
            shiny_cru: campo("shiny").cloned(),
        }
    }

    /// Atributos para o cálculo de poder, se a ficha estiver completa (`describe`).
    /// `nivel` precisa ser > 0; a validação fina de faixa fica no cálculo de poder.
    pub fn atributos(&self) -> Option<Atributos> {
        let nivel = self.nivel.filter(|n| *n > 0.0)?;
        Some(Atributos {
            ivs: self
                .ivs
                .filter(|ivs| ivs.iter().all(|iv| (1..=32).contains(iv)))?,
            qualidade: self.qualidade?,
            potencia: self.potencia?,
            // Nível fracionário: o poder no nível 100 não depende dele; guardamos truncado
            // só para tipagem, e o valor real segue em `nivel`.
            nivel: nivel as i32,
            shiny: self.shiny_frouxo,
        })
    }
}
