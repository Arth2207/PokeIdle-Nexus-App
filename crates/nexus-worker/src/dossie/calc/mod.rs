//! Rotas `GET /api/dossie/calc/catalogo` e `POST /api/dossie/calc/dossie`: porta do
//! `dossie-calc.js` do legado. Lá, um contexto VM roda o `sim.js` + `dossie-render.js` com um
//! `document` falso que guarda o HTML de cada bloco; aqui o mesmo HTML sai de funções Rust.
//!
//! Dados lidos (caminhos por variável de ambiente, padrão = caminho do legado):
//! - `NEXUS_DOSSIE_ENGINE_DATA` (`/opt/mapper/engine/data`): `pokedex.json` e `evolucoes.json`,
//!   lidos uma vez só;
//! - `NEXUS_DOSSIE_EQUIPES_ESTADO` (`/opt/mapper/data/equipes-estado.json`): relido a cada
//!   chamada (só o campo `bosses`).

mod render;
mod sim;

use std::path::PathBuf;
use std::sync::OnceLock;

use nucleo::combate::combatente::Opcoes;
use nucleo::combate::especie::{EspecieCombate, Pokedex};
use nucleo::equipes::MotorEquipes;
use nucleo::js::json_num;
use serde_json::{json, Map, Value};

use super::js as jsv;
use super::Erro;

/// Chances (`b`) e abates (`a`) por bola de uma espécie.
pub type Captura = (Vec<Option<f64>>, Vec<Option<f64>>);

/// Dados fixos carregados uma vez (em Rust, `OnceLock` ~ variável global preenchida na 1ª vez).
pub struct Dados {
    pub motor: MotorEquipes,
    /// `sp.b` e `sp.a` (chances e abates por bola) de cada espécie, na ordem da pokédex.
    pub captura: Vec<Captura>,
    /// `evolucoes.json`.
    pub evolucoes: Vec<Vec<(Value, Value)>>,
    /// `CANDS` do `renderBuild`: índices das espécies candidatas a parceiro.
    pub candidatas: Vec<usize>,
    pub catalogo: Value,
}

impl Dados {
    pub fn especies(&self) -> &[EspecieCombate] {
        self.motor.dex.especies()
    }
}

fn pasta_dados() -> PathBuf {
    std::env::var("NEXUS_DOSSIE_ENGINE_DATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/opt/mapper/engine/data"))
}

fn arquivo_estado() -> PathBuf {
    std::env::var("NEXUS_DOSSIE_EQUIPES_ESTADO")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/opt/mapper/data/equipes-estado.json"))
}

fn ler_json_arquivo(caminho: &PathBuf) -> Result<Value, String> {
    let texto = std::fs::read(caminho).map_err(|e| format!("{}: {e}", caminho.display()))?;
    serde_json::from_slice(&texto).map_err(|e| format!("{}: {e}", caminho.display()))
}

fn carregar() -> Result<Dados, String> {
    let pasta = pasta_dados();
    carregar_de(&pasta.join("pokedex.json"), &pasta.join("evolucoes.json"))
}

/// Lê pokédex e evoluções dos caminhos dados (os testes apontam para cópias no repositório).
fn carregar_de(pokedex: &PathBuf, evolucoes: &PathBuf) -> Result<Dados, String> {
    let bruta = ler_json_arquivo(pokedex)?;
    let lista = bruta.as_array().ok_or("pokedex.json: esperava lista")?;
    let numeros = |v: Option<&Value>| -> Vec<Option<f64>> {
        v.and_then(Value::as_array)
            .map(|a| a.iter().map(Value::as_f64).collect())
            .unwrap_or_default()
    };
    let captura = lista
        .iter()
        .map(|e| (numeros(e.get("b")), numeros(e.get("a"))))
        .collect();
    let dex = Pokedex::de_valor(bruta.clone()).map_err(|e| e.to_string())?;
    let motor = MotorEquipes::novo(dex);

    let evol = ler_json_arquivo(evolucoes)?;
    let evolucoes = evol
        .as_array()
        .ok_or("evolucoes.json: esperava lista")?
        .iter()
        .map(|c| {
            c.as_array()
                .map(|a| {
                    a.iter()
                        .map(|x| {
                            (
                                x.get("n").cloned().unwrap_or(Value::Null),
                                x.get("evolui").cloned().unwrap_or(Value::Null),
                            )
                        })
                        .collect()
                })
                .unwrap_or_default()
        })
        .collect();

    let especies = motor.dex.especies();
    let candidatas = especies
        .iter()
        .enumerate()
        .filter(|(_, s)| {
            s.categoria.as_deref() == Some("pokebola")
                && s.nivel_captura.is_some()
                && sim::bst(s) >= 470.0
                && !s.lendaria()
        })
        .map(|(i, _)| i)
        .collect();

    // catalogo: `{d,n,t,h,c,legend,cl}`; `h`/`c` ausentes somem (JSON.stringify ignora undefined).
    let catalogo = Value::Array(
        lista
            .iter()
            .zip(especies)
            .map(|(e, sp)| {
                let mut o = Map::new();
                for campo in ["d", "n", "t", "h", "c"] {
                    if let Some(v) = e.get(campo) {
                        let v = match v.as_f64() {
                            Some(x) if campo == "d" || campo == "h" => json_num(x),
                            _ => v.clone(),
                        };
                        o.insert(campo.to_string(), v);
                    }
                }
                o.insert("legend".into(), Value::Bool(sp.lendaria()));
                o.insert(
                    "cl".into(),
                    sim::nivel_captura(sp).map_or(Value::Null, json_num),
                );
                Value::Object(o)
            })
            .collect(),
    );
    Ok(Dados {
        motor,
        captura,
        evolucoes,
        candidatas,
        catalogo,
    })
}

static DADOS: OnceLock<Result<Dados, String>> = OnceLock::new();

fn dados() -> Result<&'static Dados, Erro> {
    DADOS
        .get_or_init(carregar)
        .as_ref()
        .map_err(|e| Erro::novo(500, e.clone()))
}

/// `GET /api/dossie/calc/catalogo`.
pub fn catalogo() -> Result<Value, Erro> {
    Ok(dados()?.catalogo.clone())
}

/// Entradas já saneadas (`ctx.inputs` do legado).
pub struct Entradas {
    pub lv: f64,
    pub q: f64,
    pub p: u8,
    pub sh: bool,
    pub iv: [f64; 6],
}

impl Entradas {
    pub fn opcoes(&self) -> Opcoes {
        Opcoes {
            qualidade: self.q,
            ivs: self.iv,
            potencia: self.p,
            shiny: self.sh,
            ..Opcoes::default()
        }
    }
}

/// `clamp(v, lo, hi, fallback)`: `Number.isFinite(+v) ? max(lo, min(hi, +v)) : fallback`.
fn limitar(v: Option<&Value>, lo: f64, hi: f64, padrao: f64) -> f64 {
    let n = jsv::numero(v);
    if n.is_finite() {
        lo.max(n.min(hi))
    } else {
        padrao
    }
}

/// `v.iv?.[i]`: indexa lista, texto (por letra) ou objeto (chave "i").
fn indice(iv: Option<&Value>, i: usize) -> Option<Value> {
    match iv? {
        Value::Array(a) => a.get(i).cloned(),
        Value::String(s) => s.chars().nth(i).map(|c| Value::String(c.to_string())),
        Value::Object(o) => o.get(&i.to_string()).cloned(),
        _ => None,
    }
}

/// `dex.find(s => s.d === +x)`: primeira espécie cujo id é exatamente o número.
fn achar(d: &Dados, x: f64) -> Option<usize> {
    d.especies().iter().position(|s| s.id as f64 == x)
}

fn entradas(body: &Value) -> Entradas {
    let v = body
        .get("inputs")
        .filter(|x| jsv::verdade(Some(x)))
        .cloned()
        .unwrap_or_else(|| json!({}));
    let c = |nome: &str| jsv::campo(Some(&v), nome);
    let iv = std::array::from_fn(|i| {
        let item = indice(c("iv"), i);
        nucleo::js::round(limitar(item.as_ref(), 1.0, 32.0, 16.0))
    });
    Entradas {
        lv: limitar(c("lv"), 1.0, 1_000_000.0, 100.0),
        q: limitar(c("q"), 0.8, 1.8, 1.1),
        // `Math.round(clamp(...))`: sempre inteiro de 1 a 5.
        p: nucleo::js::round(limitar(c("p"), 1.0, 5.0, 1.0)) as u8,
        sh: jsv::verdade(c("sh")),
        iv,
    }
}

/// `calcular(body)`: `{ sections: { id: { html?, text? } } }`, na ordem de criação dos blocos.
pub fn calcular(body: &Value) -> Result<Value, Erro> {
    calcular_com(dados()?, body, &bosses_vivos)
}

/// Mesmo cálculo com os dados e a lista de bosses vivos dados (`vivos` só é lida no modo normal).
fn calcular_com(d: &Dados, body: &Value, vivos: &dyn Fn() -> Vec<Value>) -> Result<Value, Erro> {
    let id = jsv::numero(jsv::campo(Some(body), "speciesId"));
    let Some(si) = achar(d, id) else {
        return Err(Erro::novo(400, "Espécie não encontrada"));
    };
    let Some(sp) = d.especies().get(si) else {
        return Err(Erro::novo(400, "Espécie não encontrada"));
    };
    let v = entradas(body);
    let mut secoes = render::Secoes::default();
    let teste = jsv::campo(Some(body), "test").filter(|t| jsv::verdade(Some(t)));
    if let Some(t) = teste {
        let alvo_id = jsv::numero(jsv::campo(Some(t), "speciesId"));
        let Some(alvo) = achar(d, alvo_id).and_then(|i| d.especies().get(i)) else {
            return Err(Erro::novo(400, "Selvagem não encontrado"));
        };
        let padrao_lv = alvo.nivel_captura.filter(|h| *h != 0.0).unwrap_or(1.0);
        let lv = limitar(jsv::campo(Some(t), "lv"), 1.0, 1_000_000.0, padrao_lv);
        let q = limitar(jsv::campo(Some(t), "q"), 0.8, 1.8, 1.1);
        render::teste(d, sp, &v, alvo, lv, q, &mut secoes);
    } else {
        let bosses = render::bosses_html(d, sp, &v, &vivos());
        render::dossie(d, si, &v, bosses, &mut secoes);
    }
    Ok(json!({ "sections": secoes.para_json() }))
}

/// `bosses` do `equipes-estado.json` (relido a cada chamada). Arquivo ausente, inválido ou
/// sem lista vira lista vazia, e então vale a tabela da wiki.
fn bosses_vivos() -> Vec<Value> {
    ler_json_arquivo(&arquivo_estado())
        .ok()
        .and_then(|v| v.get("bosses").and_then(Value::as_array).cloned())
        .unwrap_or_default()
}

#[cfg(test)]
mod testes;
