//! Catálogo de espécies e o cálculo de poder PvE (fórmula do código público do jogo).

use std::collections::{HashMap, HashSet};

use serde::Deserialize;

use crate::js;

/// Um golpe: `[nome, tipo, categoria, poder, recarga_ms, nivel_minimo]` na fonte.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Golpe(
    pub String,
    pub String,
    pub i64,
    pub Option<f64>,
    pub Option<f64>,
    #[serde(default)] pub Option<i64>,
);

impl Golpe {
    fn tipo(&self) -> &str {
        &self.1
    }
    fn especial(&self) -> bool {
        self.2 == 1
    }
    fn poder(&self) -> Option<f64> {
        self.3
    }
    fn recarga_ms(&self) -> Option<f64> {
        self.4
    }
    fn nivel_minimo(&self) -> i64 {
        self.5.unwrap_or(1)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Especie {
    pub id: i32,
    pub nome: String,
    pub tipos: Vec<String>,
    pub base: [f64; 6],
    pub golpes: Vec<Golpe>,
    pub evolucoes: Vec<i32>,
    pub mega_base: Option<i32>,
    pub sprite_id: Option<i32>,
    pub desempate: Option<i32>,
}

impl Especie {
    /// `megaBase` explícito ou, para ids 3000–3999 com nome "Mega ...", o id da forma base.
    pub fn base_da_mega(&self) -> Option<i32> {
        self.mega_base.or_else(|| {
            let mega = (3000..4000).contains(&self.id)
                && self
                    .nome
                    .get(..5)
                    .is_some_and(|p| p.eq_ignore_ascii_case("mega "));
            mega.then_some(self.id - 3000)
        })
    }
}

/// Atributos de uma carta que entram no poder PvE.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Atributos {
    pub ivs: [i32; 6],
    pub qualidade: f64,
    pub potencia: i32,
    pub nivel: i32,
    /// `shiny` no sentido frouxo do jogo (`true`, `1` ou `"true"`).
    pub shiny: bool,
}

/// Uma evolução final candidata e seu poder no nível 100.
#[derive(Debug, Clone, PartialEq)]
pub struct FormaFinal {
    pub id: i32,
    pub nome: String,
    pub pve: Option<f64>,
}

const LENDARIOS: &[i32] = &[
    144, 145, 146, 150, 243, 244, 245, 249, 250, 151, 251, 494, 377, 378, 379, 380, 381, 382, 383,
    384, 480, 481, 482, 483, 484, 485, 486, 487, 488, 638, 639, 640, 641, 642, 643, 644, 645, 646,
    716, 717, 718, 785, 786, 787, 788, 789, 790, 791, 792, 793, 794, 795, 796, 797, 798, 799, 800,
    803, 804, 805, 806, 888, 889, 890, 891, 892, 894, 895, 896, 897, 898, 905, 1001, 1002, 1003,
    1004, 1007, 1008, 1009, 1010, 1014, 1015, 1016, 1017, 1020, 1021, 1022, 1023, 1024, 385, 386,
    489, 490, 491, 492, 493, 647, 648, 649, 719, 720, 721, 801, 802, 807, 808, 809, 893, 1025,
];

fn lendario(id: i32) -> bool {
    let base = if id >= 13000 {
        id % 1000
    } else if (3000..4000).contains(&id) {
        id - 3000
    } else {
        id
    };
    LENDARIOS.contains(&base)
}

/// Poder PvE da carta `p` como se fosse da espécie `e` (`PokeIdlePvePower.calcular`).
/// `None` se algum atributo estiver fora da faixa válida.
pub fn poder_pve(p: &Atributos, e: &Especie) -> Option<f64> {
    let valido = (1..=9999).contains(&p.nivel)
        && (1..=5).contains(&p.potencia)
        && p.qualidade.is_finite()
        && (0.8..=1.8).contains(&p.qualidade)
        && p.ivs.iter().all(|iv| (1..=32).contains(iv))
        && e.base.iter().all(|b| b.is_finite());
    if !valido {
        return None;
    }
    let potencia = [1.0, 1.05, 1.1, 1.25, 2.0][usize::try_from(p.potencia - 1).ok()?];
    let multiplicador =
        potencia * if p.shiny { 3.0 } else { 1.0 } * if lendario(e.id) { 4.0 } else { 1.0 };
    let nivel = f64::from(p.nivel);
    // Ordem dos status: hp, atk, def, spAtk, spDef, speed. hp e speed usam expoente 0,95.
    let status: [f64; 6] = std::array::from_fn(|i| {
        let expoente = if i == 0 || i == 5 { 0.95 } else { 0.8 };
        js::round(
            (e.base[i] + 2.0 * f64::from(p.ivs[i]))
                * (nivel / 100.0)
                * js::pow(p.qualidade, expoente)
                * multiplicador,
        )
    });
    let [hp, atk, def, sp_atk, sp_def, _] = status;

    // O golpe escolhido é o de maior "score"; no empate fica o primeiro. Sem golpe, "Investida".
    let fator_nivel = 2.0 * nivel / 5.0 + 2.0;
    let golpe = e
        .golpes
        .iter()
        .filter(|g| g.nivel_minimo() <= i64::from(p.nivel))
        .fold((None::<&Golpe>, -1.0), |(melhor, score_melhor), g| {
            let (ataque, defesa) = if g.especial() {
                (sp_atk, sp_def)
            } else {
                (atk, def)
            };
            let stab = if e.tipos.iter().any(|t| t == g.tipo()) {
                1.5
            } else {
                1.0
            };
            let score = ((fator_nivel * g.poder().unwrap_or(0.0) * (ataque / defesa.max(1.0)))
                / 50.0
                + 2.0)
                * stab;
            if score > score_melhor {
                (Some(g), score)
            } else {
                (melhor, score_melhor)
            }
        })
        .0;
    let (poder, recarga) = golpe.map_or((30.0, 2000.0), |g| {
        (g.poder().unwrap_or(30.0), g.recarga_ms().unwrap_or(2000.0))
    });
    let dps = (fator_nivel * poder * atk.max(sp_atk)) / 50.0 / (recarga.max(400.0) / 1000.0);
    let hp_combate = (js::round(hp * 12.0)).max(24.0);
    Some((js::round(dps * hp_combate * ((def + sp_def) / 2.0) / 1e7)).max(1.0))
}

/// O catálogo em memória (`PokeIdleCentral`).
#[derive(Debug, Clone, Default)]
pub struct Central {
    especies: HashMap<i32, Especie>,
    /// Nome normalizado → id, primeira ocorrência na ordem da fonte.
    por_nome: HashMap<String, i32>,
    /// Nome em minúsculas → id, só ids < 1100 (têm arte própria); vale a última ocorrência.
    dex_por_nome: HashMap<String, i32>,
    /// Quem evolui para cada espécie, montado na primeira consulta (ver `nota`).
    pais: std::sync::OnceLock<HashMap<i32, Vec<i32>>>,
}

impl Central {
    /// Recebe as espécies na ordem da fonte.
    pub fn novo(lista: impl IntoIterator<Item = Especie>) -> Self {
        let lista: Vec<Especie> = lista.into_iter().collect();
        let por_nome = lista.iter().rev().fold(HashMap::new(), |mut m, e| {
            // Percorrendo de trás para frente, a primeira ocorrência é a última a gravar.
            m.insert(js::normalizar_nome(&e.nome), e.id);
            m
        });
        let dex_por_nome = lista
            .iter()
            .filter(|e| e.id < 1100)
            .map(|e| (e.nome.to_lowercase(), e.id))
            .collect();
        let especies = lista.into_iter().map(|e| (e.id, e)).collect();
        Self {
            especies,
            por_nome,
            dex_por_nome,
            pais: std::sync::OnceLock::new(),
        }
    }

    /// O mapa de pais, montado uma vez por catálogo.
    pub(crate) fn pais_com(
        &self,
        montar: impl FnOnce() -> HashMap<i32, Vec<i32>>,
    ) -> &HashMap<i32, Vec<i32>> {
        self.pais.get_or_init(montar)
    }

    /// Id com arte própria (< 1100) pelo nome em minúsculas.
    pub fn dex_por_nome(&self, nome: &str) -> Option<i32> {
        self.dex_por_nome.get(nome).copied()
    }

    pub fn especie(&self, id: i32) -> Option<&Especie> {
        self.especies.get(&id)
    }

    pub fn len(&self) -> usize {
        self.especies.len()
    }

    pub fn is_empty(&self) -> bool {
        self.especies.is_empty()
    }

    /// `speciesLookup`: o id, se existir; senão, pelo nome.
    pub fn achar(&self, id: Option<i32>, nome: Option<&str>) -> Option<i32> {
        match id {
            Some(id) if self.especies.contains_key(&id) => Some(id),
            _ => self
                .por_nome
                .get(&js::normalizar_nome(nome.unwrap_or("")))
                .copied(),
        }
    }

    /// As evoluções finais a partir de `id` (busca em profundidade, sem megas).
    pub fn finais(&self, id: i32) -> Vec<&Especie> {
        fn visitar<'c>(
            central: &'c Central,
            id: i32,
            vistos: &mut HashSet<i32>,
            saida: &mut Vec<&'c Especie>,
        ) {
            if !vistos.insert(id) {
                return;
            }
            let filhos: Vec<i32> = central
                .especies
                .get(&id)
                .map(|e| {
                    e.evolucoes
                        .iter()
                        .copied()
                        .filter(|f| central.especies.contains_key(f))
                        .collect()
                })
                .unwrap_or_default();
            if filhos.is_empty() {
                if let Some(e) = central.especies.get(&id) {
                    saida.push(e);
                }
            } else {
                filhos
                    .into_iter()
                    .for_each(|f| visitar(central, f, vistos, saida));
            }
        }
        let mut saida = Vec::new();
        visitar(self, id, &mut HashSet::new(), &mut saida);
        saida
    }

    /// `evaluate(p, false).final`: a evolução final de maior PvE no nível 100.
    /// Empate: menor `desempate`, depois menor id.
    pub fn melhor_final(&self, especie: i32, p: &Atributos) -> Option<FormaFinal> {
        let nascimento = Atributos { nivel: 100, ..*p };
        let mut opcoes: Vec<(FormaFinal, i32)> = self
            .finais(especie)
            .into_iter()
            .filter_map(|forma| {
                let poder = poder_pve(&nascimento, forma)?;
                Some((
                    FormaFinal {
                        id: forma.id,
                        nome: forma.nome.clone(),
                        pve: (!forma.golpes.is_empty()).then_some(poder),
                    },
                    forma.desempate.unwrap_or(0),
                ))
            })
            .collect();
        // No JS, `null - x` vale `0 - x`: PvE ausente ordena como zero.
        opcoes.sort_by(|(a, ta), (b, tb)| {
            b.pve
                .unwrap_or(0.0)
                .total_cmp(&a.pve.unwrap_or(0.0))
                .then(ta.cmp(tb))
                .then(a.id.cmp(&b.id))
        });
        opcoes.into_iter().next().map(|(f, _)| f)
    }

    /// Tipos da espécie (vazio se não existir).
    pub fn tipos(&self, id: i32) -> &[String] {
        self.especies.get(&id).map_or(&[], |e| e.tipos.as_slice())
    }

    /// Todas as espécies, em ordem de id.
    pub fn todas(&self) -> impl Iterator<Item = &Especie> {
        let mut v: Vec<&Especie> = self.especies.values().collect();
        v.sort_by_key(|e| e.id);
        v.into_iter()
    }
}
