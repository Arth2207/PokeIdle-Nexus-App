//! Shiny do servidor inteiro: o jogo avisa todo mundo a cada shiny capturado (`shiny.captura`).
//! Cada conta do dono recebe o mesmo aviso, então a contagem usa o id do pokémon (`pid`).

use std::collections::{BTreeMap, HashMap, HashSet};

use serde::Serialize;
use serde_json::Value;

use crate::js::data_sp;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PorEspecie {
    pub especie: i64,
    pub nome: String,
    pub vezes: u32,
    pub nivel_medio: f64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ShinyServidor {
    pub total: u32,
    /// Shiny por hora no período (entre o primeiro e o último aviso).
    pub por_hora: f64,
    pub especies: Vec<PorEspecie>,
    /// Dia (UTC−3) → quantidade.
    pub dias: BTreeMap<String, u32>,
    /// Hora do dia (UTC−3, 0–23) → quantidade.
    pub horas: [u32; 24],
}

/// `avisos`: (instante em ms, `dados` do evento `shiny.servidor`).
pub fn resumir(avisos: &[(i64, Value)]) -> ShinyServidor {
    let mut vistos = HashSet::new();
    let mut esp: HashMap<i64, (String, u32, f64)> = HashMap::new();
    let mut dias = BTreeMap::new();
    let mut horas = [0u32; 24];
    let (mut total, mut primeiro, mut ultimo) = (0u32, i64::MAX, i64::MIN);
    for (em, d) in avisos {
        let pid = d.get("pid").and_then(Value::as_i64);
        if pid.is_some_and(|p| !vistos.insert(p)) {
            continue;
        }
        let Some(especie) = d.get("especie").and_then(Value::as_i64) else {
            continue;
        };
        total += 1;
        primeiro = primeiro.min(*em);
        ultimo = ultimo.max(*em);
        let e = esp.entry(especie).or_insert_with(|| {
            (
                d.get("nome")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned(),
                0,
                0.0,
            )
        });
        e.1 += 1;
        e.2 += d.get("nivel").and_then(Value::as_f64).unwrap_or(0.0);
        *dias.entry(data_sp(*em)).or_insert(0) += 1;
        let h = (em - 3 * 3_600_000).rem_euclid(86_400_000) / 3_600_000;
        horas[h as usize] += 1;
    }
    let horas_periodo = ((ultimo - primeiro) as f64 / 3_600_000.0).max(1.0);
    let mut especies: Vec<PorEspecie> = esp
        .into_iter()
        .map(|(especie, (nome, vezes, soma))| PorEspecie {
            especie,
            nome,
            vezes,
            nivel_medio: (soma / f64::from(vezes) * 10.0).round() / 10.0,
        })
        .collect();
    especies.sort_by(|a, b| b.vezes.cmp(&a.vezes).then(a.especie.cmp(&b.especie)));
    ShinyServidor {
        total,
        por_hora: if total > 0 {
            (f64::from(total) / horas_periodo * 100.0).round() / 100.0
        } else {
            0.0
        },
        especies,
        dias,
        horas,
    }
}

#[cfg(test)]
mod testes {
    use super::*;
    use serde_json::json;

    #[test]
    fn conta_cada_shiny_uma_vez() {
        let t = 1_794_632_400_000; // 14/11 02:00 em UTC−3
        let a = |em: i64, pid: i64, esp: i64| {
            (
                em,
                json!({"pid": pid, "especie": esp, "nome": format!("E{esp}"), "nivel": 40}),
            )
        };
        let r = resumir(&[
            a(t, 1, 397),
            a(t + 1, 1, 397),
            a(t + 7_200_000, 2, 397),
            a(t + 3_600_000, 3, 10),
        ]);
        assert_eq!(r.total, 3);
        assert_eq!(r.especies[0].especie, 397);
        assert_eq!(r.especies[0].vezes, 2);
        assert_eq!(r.por_hora, 1.5);
        assert_eq!(r.dias.get("2026-11-14"), Some(&3));
        assert_eq!(r.horas.iter().sum::<u32>(), 3);
        assert_eq!((r.horas[2], r.horas[3], r.horas[4]), (1, 1, 1));
    }
}
