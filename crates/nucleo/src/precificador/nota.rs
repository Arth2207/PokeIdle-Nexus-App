//! Nota da calculadora (0 a 10 dentro da espécie): porte do `PokeIdleNota` do legado.
//!
//! Compara a carta com o pior e o melhor caso possíveis da mesma espécie. Uma espécie evoluída
//! nunca tem nota menor que a das anteriores: cada passo de evolução vale +0,1 sobre a nota
//! calculada com os status base da anterior. As contas usam `js::pow`, `js::ln` e `js::round`
//! para sair idênticas ao `Math.*` do Node.

use std::collections::{HashMap, HashSet};

use serde_json::Value;

use crate::js;
use crate::precificador::especie::Central;

/// Ordem dos status: hp, atk, def, spAtk, spDef, speed.
const EXPOENTE: [f64; 6] = [0.95, 0.8, 0.8, 0.8, 0.8, 0.95];
const POTENCIA: [f64; 5] = [0.0, 0.05, 0.1, 0.25, 1.0];
/// Nota mínima do site para a última evolução.
pub const NOTA_MINIMA: f64 = 3.5;
/// Até quantas evoluções anteriores entram na conta.
const PASSOS_MAX: u32 = 12;

/// Os atributos da carta que entram na nota.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Entrada {
    pub ivs: [i64; 6],
    pub qualidade: f64,
    /// 1 a 5 (inteiro; fração dá `NaN` no legado, que vira "sem nota").
    pub potencia: i64,
    pub shiny: bool,
}

/// `Math.round(n * 1000) / 1000`.
fn arred(n: f64) -> f64 {
    js::round(n * 1000.0) / 1000.0
}

/// `Math.min(1, Math.max(0, n))`, com `NaN` passando adiante como no JS.
fn limitar(n: f64) -> f64 {
    if n.is_nan() {
        n
    } else {
        n.clamp(0.0, 1.0)
    }
}

/// Soma dos status de nascimento (nível 100), na régua da nota.
fn nascimento(base: &[f64; 6], p: &Entrada) -> f64 {
    let mult = 1.0 + POTENCIA[(p.potencia - 1) as usize % 5];
    let brilho = if p.shiny { 3.0 } else { 1.0 };
    base.iter()
        .enumerate()
        .map(|(i, v)| {
            js::round(
                (v + 2.0 * p.ivs[i] as f64) * js::pow(p.qualidade, EXPOENTE[i]) * mult * brilho,
            )
        })
        .sum()
}

const NEUTRA: [f64; 6] = [80.0; 6];
const PIOR: Entrada = Entrada {
    ivs: [1; 6],
    qualidade: 0.8,
    potencia: 1,
    shiny: false,
};
const MELHOR: Entrada = Entrada {
    ivs: [32; 6],
    qualidade: 1.8,
    potencia: 5,
    shiny: true,
};

fn escala(base: &[f64; 6], p: &Entrada, baixo: f64, alto: f64) -> f64 {
    arred(10.0 * limitar((js::ln(nascimento(base, p)) - baixo) / (alto - baixo))) / 10.0
}

fn nota_da_forma(base: &[f64; 6], p: &Entrada, baixo: f64, alto: f64) -> f64 {
    let min = escala(base, &PIOR, baixo, alto);
    let max = escala(base, &MELHOR, baixo, alto);
    if max > min {
        arred(10.0 * limitar((escala(base, p, baixo, alto) - min) / (max - min)))
    } else {
        0.0
    }
}

/// Nota da carta `p` como se fosse da espécie `id`; `None` se a espécie não existir ou a
/// entrada for inválida.
pub fn nota(central: &Central, id: i32, p: &Entrada) -> Option<f64> {
    let e = central.especie(id)?;
    if !(p.qualidade > 0.0 && (1..=5).contains(&p.potencia)) {
        return None;
    }
    let baixo = js::ln(nascimento(&NEUTRA, &PIOR));
    let alto = js::ln(nascimento(&NEUTRA, &MELHOR));

    // Pais de cada espécie: quem evolui para ela e, nas megas, a forma base (montado uma vez).
    let pais = central.pais_com(|| {
        let mut pais: HashMap<i32, Vec<i32>> = HashMap::new();
        let mut ligar = |de: i32, para: i32| {
            let v = pais.entry(para).or_default();
            if !v.contains(&de) {
                v.push(de);
            }
        };
        for s in central.todas() {
            for filho in &s.evolucoes {
                ligar(s.id, *filho);
            }
            if let Some(base) = s.base_da_mega() {
                if central.especie(base).is_some() {
                    ligar(base, s.id);
                }
            }
        }
        pais
    });

    let mut melhor = nota_da_forma(&e.base, p, baixo, alto);
    let mut camada: Vec<i32> = pais.get(&e.id).cloned().unwrap_or_default();
    let mut vistos: HashSet<i32> = HashSet::from([e.id]);
    let mut passo = 1u32;
    while passo <= PASSOS_MAX && !camada.is_empty() {
        let mut proxima = Vec::new();
        for a in camada {
            if !vistos.insert(a) {
                continue;
            }
            if let Some(s) = central.especie(a) {
                let g = (nota_da_forma(&s.base, p, baixo, alto) + f64::from(passo) * 0.1).min(10.0);
                melhor = melhor.max(g);
            }
            proxima.extend(pais.get(&a).into_iter().flatten().copied());
        }
        camada = proxima;
        passo += 1;
    }
    Some(arred(melhor))
}

/// `notaFicha` do legado: a nota de uma ficha (`speciesId`, `ivs`, `quality`, `potencia`,
/// `shiny`). `None` quando falta algo ou o resultado não é um número finito.
pub fn nota_ficha(central: &Central, f: &Value) -> Option<f64> {
    let id = js::mais(f.get("speciesId"));
    if !(id.is_finite() && id.fract() == 0.0 && id.abs() < f64::from(i32::MAX)) {
        return None;
    }
    let ivs_json = f.get("ivs")?;
    let mut ivs = [0i64; 6];
    for (i, k) in ["hp", "atk", "def", "spAtk", "spDef", "speed"]
        .iter()
        .enumerate()
    {
        let n = ivs_json.get(*k)?.as_f64()?;
        if n.fract() != 0.0 || n.abs() > 1e9 {
            return None;
        }
        ivs[i] = n as i64;
    }
    let qualidade = js::mais(f.get("quality"));
    let potencia = js::mais(f.get("potencia"));
    if potencia.fract() != 0.0 || !(1.0..=5.0).contains(&potencia) {
        return None;
    }
    let entrada = Entrada {
        ivs,
        qualidade,
        potencia: potencia as i64,
        shiny: f.get("shiny") == Some(&Value::Bool(true)),
    };
    nota(central, id as i32, &entrada).filter(|v| v.is_finite())
}

#[cfg(test)]
mod testes {
    use super::*;
    use crate::precificador::especie::Especie;
    use serde_json::json;

    fn esp(id: i32, nome: &str, base: f64, evo: &[i32]) -> Especie {
        Especie {
            id,
            nome: nome.into(),
            tipos: vec![],
            base: [base; 6],
            golpes: vec![],
            evolucoes: evo.to_vec(),
            mega_base: None,
            sprite_id: None,
            desempate: None,
        }
    }

    fn entrada(iv: i64, q: f64, pot: i64, shiny: bool) -> Entrada {
        Entrada {
            ivs: [iv; 6],
            qualidade: q,
            potencia: pot,
            shiny,
        }
    }

    // Valores abaixo calculados à mão com a fórmula do `nota.js` (mesma ordem de contas).
    #[test]
    fn extremos_da_especie() {
        let c = Central::novo([esp(1, "A", 80.0, &[])]);
        assert_eq!(nota(&c, 1, &entrada(32, 1.8, 5, true)), Some(10.0));
        assert_eq!(nota(&c, 1, &entrada(1, 0.8, 1, false)), Some(0.0));
    }

    #[test]
    fn evolucao_nao_perde_para_a_anterior() {
        // Forte (1) evolui para fraco (2): a carta do fraco herda a nota do forte + 0,1.
        let c = Central::novo([esp(1, "Forte", 150.0, &[2]), esp(2, "Fraco", 30.0, &[])]);
        let p = entrada(16, 1.2, 3, false);
        assert_eq!(nota(&c, 1, &p), Some(2.561));
        assert_eq!(nota(&c, 2, &p), Some(2.661));
    }

    #[test]
    fn mega_conta_como_evolucao_da_base() {
        let c = Central::novo([
            esp(1, "Forte", 150.0, &[]),
            esp(3001, "Mega Fraco", 30.0, &[]),
        ]);
        assert_eq!(nota(&c, 3001, &entrada(16, 1.2, 3, true)), Some(7.18));
    }

    #[test]
    fn entrada_invalida_vira_none() {
        let c = Central::novo([esp(1, "A", 80.0, &[])]);
        assert_eq!(nota(&c, 9, &entrada(10, 1.0, 2, false)), None);
        assert_eq!(nota(&c, 1, &entrada(10, 0.0, 2, false)), None);
        assert_eq!(nota(&c, 1, &entrada(10, 1.0, 6, false)), None);
    }

    #[test]
    fn nota_de_ficha_json() {
        let c = Central::novo([esp(1, "Forte", 150.0, &[2]), esp(2, "Fraco", 30.0, &[])]);
        let ivs = json!({"hp":16,"atk":16,"def":16,"spAtk":16,"spDef":16,"speed":16});
        let f = json!({"speciesId": 2, "ivs": ivs, "quality": "1.2", "potencia": 3});
        assert_eq!(nota_ficha(&c, &f), Some(2.661));
        // shiny só vale com `true` de verdade.
        let f = json!({"speciesId": 2, "ivs": ivs, "quality": 1.2, "potencia": 3, "shiny": 1});
        assert_eq!(nota_ficha(&c, &f), Some(2.661));
        assert_eq!(
            nota_ficha(&c, &json!({"speciesId": 2, "quality": 1.2, "potencia": 3})),
            None
        );
        assert_eq!(
            nota_ficha(
                &c,
                &json!({"speciesId": 2, "ivs": ivs, "quality": 1.2, "potencia": 2.5})
            ),
            None
        );
    }
}
