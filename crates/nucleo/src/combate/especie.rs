//! Espécies como o motor de combate do legado as lê (`pokedex.json`, formato compacto).

use std::collections::HashMap;

use serde::Deserialize;
use serde_json::Value;

use super::tipos::Tipo;

/// Categoria do golpe. Só `"F"` é físico; qualquer outra coisa usa os atributos especiais.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Categoria {
    Fisico,
    Especial,
}

impl Categoria {
    fn de(texto: &str) -> Self {
        if texto == "F" {
            Categoria::Fisico
        } else {
            Categoria::Especial
        }
    }
}

/// Golpe de uma espécie.
#[derive(Debug, Clone, PartialEq)]
pub struct GolpeEspecie {
    pub nome: String,
    pub poder: f64,
    /// Recarga base em segundos.
    pub recarga: f64,
    pub categoria: Categoria,
    pub tipo: Tipo,
    /// Nível a partir do qual o golpe é liberado (`m[5]`); sem valor, sempre liberado.
    pub nivel_min: Option<f64>,
}

/// Uma espécie da pokédex do motor.
#[derive(Debug, Clone, PartialEq)]
pub struct EspecieCombate {
    /// `d`: id da espécie (formas e megas têm ids acima de 1000).
    pub id: i64,
    /// `n`
    pub nome: String,
    /// `t`
    pub tipos: Vec<Tipo>,
    /// `s`: HP, ATK, DEF, SpATK, SpDEF, SPD base.
    pub base: [f64; 6],
    /// `h`: nível de captura.
    pub nivel_captura: Option<f64>,
    /// `c`: `pokebola`, `mega`, `arena_mistica` ou `boss`.
    pub categoria: Option<String>,
    /// `b[4]`: o multiplicador que identifica o estágio evolutivo.
    pub estagio: Option<f64>,
    /// `m`
    pub golpes: Vec<GolpeEspecie>,
}

/// Formas lendárias que a pokédex não marca como `arena_mistica` nem `boss`.
const LENDAS_EXTRAS: [&str; 34] = [
    "galarian articuno",
    "galarian zapdos",
    "galarian moltres",
    "deoxys attack",
    "deoxys defense",
    "deoxys speed",
    "dialga origin",
    "palkia origin",
    "shaymin sky",
    "tornadus therian",
    "thundurus therian",
    "landorus therian",
    "white kyurem",
    "black kyurem",
    "keldeo resolute",
    "zygarde 10%",
    "zygarde complete",
    "hoopa unbound",
    "zacian crowned",
    "zamazenta crowned",
    "enamorus therian",
    "mega mewtwo y",
    "mega mewtwo x",
    "mega latias",
    "mega latios",
    "primal kyogre",
    "primal groudon",
    "mega rayquaza",
    "mega heatran",
    "mega darkrai",
    "mega zygarde",
    "mega diancie",
    "mega magearna",
    "mega zeraora",
];

impl EspecieCombate {
    /// `isLegend`: categoria `arena_mistica` ou `boss`, ou uma das formas da lista extra.
    pub fn lendaria(&self) -> bool {
        matches!(self.categoria.as_deref(), Some("arena_mistica" | "boss"))
            || LENDAS_EXTRAS.contains(&self.nome.to_lowercase().as_str())
    }

    /// Só espécies capturáveis em pokébola (ou sem categoria) entram nas equipes teóricas.
    pub fn de_pokebola(&self) -> bool {
        matches!(self.categoria.as_deref(), None | Some("pokebola"))
    }
}

/// Erro ao ler a pokédex.
#[derive(Debug, thiserror::Error)]
pub enum ErroPokedex {
    #[error("JSON inválido: {0}")]
    Json(#[from] serde_json::Error),
    #[error("espécie {id}: {motivo}")]
    Especie { id: i64, motivo: String },
}

/// Formato cru do arquivo (as chaves de uma letra do legado).
#[derive(Deserialize)]
struct EspecieCru {
    d: i64,
    n: String,
    t: Vec<String>,
    s: [f64; 6],
    h: Option<f64>,
    c: Option<String>,
    #[serde(default)]
    b: Vec<Option<f64>>,
    #[serde(default)]
    m: Vec<Vec<Value>>,
}

impl TryFrom<EspecieCru> for EspecieCombate {
    type Error = ErroPokedex;

    fn try_from(cru: EspecieCru) -> Result<Self, Self::Error> {
        let erro = |motivo: String| ErroPokedex::Especie { id: cru.d, motivo };
        let tipos = cru
            .t
            .iter()
            .map(|t| t.parse::<Tipo>().map_err(|e| erro(e.to_string())))
            .collect::<Result<Vec<_>, _>>()?;
        let golpes = cru
            .m
            .iter()
            .map(|g| golpe(g).ok_or_else(|| erro(format!("golpe inválido: {g:?}"))))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(EspecieCombate {
            id: cru.d,
            nome: cru.n,
            tipos,
            base: cru.s,
            nivel_captura: cru.h,
            categoria: cru.c,
            estagio: cru.b.get(4).copied().flatten(),
            golpes,
        })
    }
}

/// `[nome, poder, recarga, categoria, tipo, nível mínimo?]`.
fn golpe(g: &[Value]) -> Option<GolpeEspecie> {
    let [nome, poder, recarga, categoria, tipo, resto @ ..] = g else {
        return None;
    };
    Some(GolpeEspecie {
        nome: nome.as_str()?.to_string(),
        poder: poder.as_f64()?,
        recarga: recarga.as_f64()?,
        categoria: Categoria::de(categoria.as_str()?),
        tipo: tipo.as_str()?.parse().ok()?,
        nivel_min: resto.first().and_then(Value::as_f64),
    })
}

/// A pokédex indexada por id e por nome (minúsculo), como os `Map` do `equipes-core.js`:
/// em nomes repetidos vale a última espécie, como no `new Map(...)`.
#[derive(Debug, Clone, Default)]
pub struct Pokedex {
    especies: Vec<EspecieCombate>,
    por_id: HashMap<i64, usize>,
    por_nome: HashMap<String, usize>,
}

impl Pokedex {
    pub fn nova(especies: Vec<EspecieCombate>) -> Self {
        let mut por_id = HashMap::with_capacity(especies.len());
        let mut por_nome = HashMap::with_capacity(especies.len());
        for (i, sp) in especies.iter().enumerate() {
            por_id.insert(sp.id, i);
            por_nome.insert(sp.nome.to_lowercase(), i);
        }
        Pokedex {
            especies,
            por_id,
            por_nome,
        }
    }

    /// Lê o `pokedex.json` do legado.
    pub fn de_json(texto: &str) -> Result<Self, ErroPokedex> {
        Self::de_valor(serde_json::from_str(texto)?)
    }

    /// Mesma coisa, a partir de um JSON já lido.
    pub fn de_valor(valor: Value) -> Result<Self, ErroPokedex> {
        let crus: Vec<EspecieCru> = serde_json::from_value(valor)?;
        let especies = crus
            .into_iter()
            .map(EspecieCombate::try_from)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self::nova(especies))
    }

    pub fn especies(&self) -> &[EspecieCombate] {
        &self.especies
    }

    pub fn por_id(&self, id: i64) -> Option<&EspecieCombate> {
        self.por_id.get(&id).and_then(|&i| self.especies.get(i))
    }

    /// Busca pelo nome, sem diferenciar maiúsculas.
    pub fn por_nome(&self, nome: &str) -> Option<&EspecieCombate> {
        self.por_nome
            .get(&nome.to_lowercase())
            .and_then(|&i| self.especies.get(i))
    }
}
