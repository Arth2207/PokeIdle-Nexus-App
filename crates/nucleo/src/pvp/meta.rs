//! Meta do PvP por faixa de pontos: quais espécies aparecem e vencem, em que casas, e quais
//! casas vencem para cada alcance. Lê as partidas que o app grava (`pvp.partida`).

use std::collections::{BTreeMap, HashMap, HashSet};

use serde::Serialize;
use serde_json::Value;

/// Largura da faixa de pontos (uma divisão do jogo tem 100 pontos).
pub const LARGURA_FAIXA: i64 = 100;

/// Faixa de uma pontuação (`461` → `400`).
pub fn faixa(pontos: i64) -> i64 {
    pontos.div_euclid(LARGURA_FAIXA) * LARGURA_FAIXA
}

/// Um pokémon de um lado da partida.
#[derive(Debug, Clone, PartialEq)]
pub struct Lutador {
    pub especie: i64,
    pub nome: String,
    pub nivel: i64,
    pub alcance: i64,
    pub casa: i64,
}

/// Uma partida vista por uma das contas do dono.
#[derive(Debug, Clone, PartialEq)]
pub struct Partida {
    pub id: i64,
    pub em_ms: i64,
    pub pontos: i64,
    pub venci: bool,
    pub meu: Vec<Lutador>,
    pub oponente: Vec<Lutador>,
}

fn lutadores(v: Option<&Value>) -> Vec<Lutador> {
    v.and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|a| {
            Some(Lutador {
                especie: a.get("especie")?.as_i64()?,
                nome: a
                    .get("nome")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned(),
                nivel: a.get("nivel").and_then(Value::as_i64).unwrap_or(0),
                alcance: a.get("alcance").and_then(Value::as_i64).unwrap_or(1),
                casa: a.get("casa").and_then(Value::as_i64)?,
            })
        })
        .collect()
}

impl Partida {
    /// O `dados` de um evento `pvp.partida`.
    pub fn de_evento(em_ms: i64, d: &Value) -> Option<Self> {
        Some(Self {
            id: d.get("id")?.as_i64()?,
            em_ms,
            pontos: d.get("antes").and_then(Value::as_i64).unwrap_or(0),
            venci: d.get("venci").and_then(Value::as_bool)?,
            meu: lutadores(d.get("meu")),
            oponente: lutadores(d.get("oponente")),
        })
        .filter(|p| !p.meu.is_empty() && !p.oponente.is_empty())
    }

    /// Os dois times como (time, venceu).
    fn lados(&self) -> [(&[Lutador], bool); 2] {
        [(&self.meu, self.venci), (&self.oponente, !self.venci)]
    }
}

/// Usos e vitórias.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub struct Placar {
    pub usos: u32,
    pub vitorias: u32,
}

impl Placar {
    fn somar(&mut self, venceu: bool) {
        self.usos += 1;
        self.vitorias += u32::from(venceu);
    }

    /// Taxa de vitória com suavização (2 vitórias e 2 derrotas "de mentira"): pouca amostra
    /// fica perto de 50%.
    pub fn taxa(&self) -> f64 {
        (f64::from(self.vitorias) + 2.0) / (f64::from(self.usos) + 4.0)
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Especie {
    pub especie: i64,
    pub nome: String,
    pub usos: u32,
    pub vitorias: u32,
    pub taxa: f64,
    /// Alcance de ataque (casas) visto no replay.
    pub alcance: i64,
    /// Nível médio com que a espécie aparece.
    pub nivel_medio: i64,
    /// Casas mais usadas (casa, usos, taxa), da mais usada para a menos.
    pub casas: Vec<(i64, u32, f64)>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CasaAlcance {
    pub alcance: i64,
    /// (casa, usos, taxa), da melhor taxa para a pior (mínimo de 3 usos).
    pub casas: Vec<(i64, u32, f64)>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TimeComum {
    pub especies: Vec<i64>,
    pub nomes: Vec<String>,
    pub vezes: u32,
    pub taxa: f64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Meta {
    pub faixa: Option<i64>,
    pub partidas: u32,
    pub especies: Vec<Especie>,
    pub alcances: Vec<CasaAlcance>,
    /// Times dos oponentes que mais aparecem.
    pub times: Vec<TimeComum>,
}

fn ordenar_casas(m: &BTreeMap<i64, Placar>, minimo: u32, por_taxa: bool) -> Vec<(i64, u32, f64)> {
    let mut v: Vec<(i64, u32, f64)> = m
        .iter()
        .filter(|(_, p)| p.usos >= minimo)
        .map(|(c, p)| (*c, p.usos, p.taxa()))
        .collect();
    if por_taxa {
        v.sort_by(|a, b| b.2.total_cmp(&a.2).then(b.1.cmp(&a.1)));
    } else {
        v.sort_by(|a, b| b.1.cmp(&a.1).then(b.2.total_cmp(&a.2)));
    }
    v
}

/// O que se soma de uma espécie enquanto as partidas são lidas.
struct Acumulado {
    nome: String,
    alcance: i64,
    placar: Placar,
    casas: BTreeMap<i64, Placar>,
    niveis: i64,
}

/// Junta as partidas (cada `id` uma vez) da faixa pedida (ou de todas, com `None`).
pub fn agregar(partidas: &[Partida], faixa_pedida: Option<i64>) -> Meta {
    let mut vistas = HashSet::new();
    let mut esp: HashMap<i64, Acumulado> = HashMap::new();
    let mut alc: BTreeMap<i64, BTreeMap<i64, Placar>> = BTreeMap::new();
    let mut times: HashMap<Vec<i64>, (Vec<String>, Placar)> = HashMap::new();
    let mut n = 0;
    for p in partidas {
        if faixa_pedida.is_some_and(|f| faixa(p.pontos) != f) || !vistas.insert(p.id) {
            continue;
        }
        n += 1;
        for (time, venceu) in p.lados() {
            for l in time {
                let e = esp.entry(l.especie).or_insert_with(|| Acumulado {
                    nome: l.nome.clone(),
                    alcance: l.alcance,
                    placar: Placar::default(),
                    casas: BTreeMap::new(),
                    niveis: 0,
                });
                e.placar.somar(venceu);
                e.niveis += l.nivel;
                e.casas.entry(l.casa).or_default().somar(venceu);
                alc.entry(l.alcance)
                    .or_default()
                    .entry(l.casa)
                    .or_default()
                    .somar(venceu);
            }
        }
        let mut chave: Vec<(i64, String)> = p
            .oponente
            .iter()
            .map(|l| (l.especie, l.nome.clone()))
            .collect();
        chave.sort();
        let t = times
            .entry(chave.iter().map(|x| x.0).collect())
            .or_insert_with(|| (chave.into_iter().map(|x| x.1).collect(), Placar::default()));
        t.1.somar(!p.venci);
    }
    let mut especies: Vec<Especie> = esp
        .into_iter()
        .map(|(id, a)| Especie {
            especie: id,
            nome: a.nome,
            alcance: a.alcance,
            nivel_medio: a.niveis / i64::from(a.placar.usos.max(1)),
            usos: a.placar.usos,
            vitorias: a.placar.vitorias,
            taxa: a.placar.taxa(),
            casas: ordenar_casas(&a.casas, 1, false)
                .into_iter()
                .take(5)
                .collect(),
        })
        .collect();
    especies.sort_by(|a, b| b.usos.cmp(&a.usos).then(b.taxa.total_cmp(&a.taxa)));
    let alcances = alc
        .iter()
        .map(|(a, casas)| CasaAlcance {
            alcance: *a,
            casas: ordenar_casas(casas, 3, true),
        })
        .collect();
    let mut times: Vec<TimeComum> = times
        .into_iter()
        .map(|(especies, (nomes, pl))| TimeComum {
            especies,
            nomes,
            vezes: pl.usos,
            taxa: pl.taxa(),
        })
        .collect();
    times.sort_by(|a, b| b.vezes.cmp(&a.vezes).then(b.taxa.total_cmp(&a.taxa)));
    times.truncate(10);
    Meta {
        faixa: faixa_pedida,
        partidas: n,
        especies,
        alcances,
        times,
    }
}

#[cfg(test)]
pub(crate) mod testes {
    use super::*;
    use serde_json::json;

    pub fn partida(
        id: i64,
        pontos: i64,
        venci: bool,
        meu: &[(i64, i64, i64)],
        op: &[(i64, i64, i64)],
    ) -> Partida {
        let l = |x: &[(i64, i64, i64)]| -> Vec<Lutador> {
            x.iter()
                .map(|(e, a, c)| Lutador {
                    especie: *e,
                    nome: format!("E{e}"),
                    nivel: 100,
                    alcance: *a,
                    casa: *c,
                })
                .collect()
        };
        Partida {
            id,
            em_ms: 0,
            pontos,
            venci,
            meu: l(meu),
            oponente: l(op),
        }
    }

    #[test]
    fn le_o_evento_do_app() {
        let d = json!({"id": 7, "antes": 461, "venci": true,
            "meu": [{"especie": 127, "nome": "Pinsir", "nivel": 100, "alcance": 1, "casa": 2}],
            "oponente": [{"especie": 6, "nome": "Charizard", "nivel": 362, "alcance": 3, "casa": 17}]});
        let p = Partida::de_evento(9, &d).unwrap_or_else(|| partida(0, 0, false, &[], &[]));
        assert_eq!((p.id, p.pontos, p.venci), (7, 461, true));
        assert_eq!(p.oponente[0].casa, 17);
        assert!(Partida::de_evento(9, &json!({"id": 1, "venci": true})).is_none());
    }

    #[test]
    fn agrega_por_faixa_sem_repetir_partida() {
        let ps = vec![
            partida(
                1,
                450,
                true,
                &[(1, 1, 2), (2, 3, 17)],
                &[(3, 1, 2), (4, 3, 18)],
            ),
            partida(
                1,
                450,
                true,
                &[(1, 1, 2), (2, 3, 17)],
                &[(3, 1, 2), (4, 3, 18)],
            ),
            partida(2, 470, false, &[(1, 1, 3)], &[(3, 1, 2), (4, 3, 18)]),
            partida(3, 650, true, &[(9, 1, 1)], &[(8, 1, 1)]),
        ];
        let m = agregar(&ps, Some(400));
        assert_eq!(m.partidas, 2);
        let e1 = m
            .especies
            .iter()
            .find(|e| e.especie == 1)
            .map(|e| (e.usos, e.vitorias));
        assert_eq!(e1, Some((2, 1)));
        assert_eq!(m.times[0].especies, vec![3, 4]);
        assert_eq!(m.times[0].vezes, 2);
        // Alcance 1: casa 2 tem 3 usos (2 do oponente, 1 meu).
        let a1 = m
            .alcances
            .iter()
            .find(|a| a.alcance == 1)
            .map(|a| a.casas.clone());
        assert_eq!(a1.map(|c| c[0].0), Some(2));
        assert_eq!(agregar(&ps, None).partidas, 3);
        assert_eq!(faixa(-1), -100);
    }
}
