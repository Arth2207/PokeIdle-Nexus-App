//! Sugestão de time e casas para o PvP: os 5 pokémon da coleção que melhor enfrentam o meta
//! da faixa (força, vantagem de tipo contra as espécies mais usadas e taxa de vitória da
//! espécie) e, para cada um, a casa que mais vence para o alcance dele.

use std::collections::{HashMap, HashSet};
use std::str::FromStr;

use serde::Serialize;
use serde_json::Value;

use super::meta::Meta;
use crate::combate::especie::Pokedex;
use crate::combate::tipos::{efetividade_bruta, Tipo};

/// Nível mínimo do PvP (regra do jogo).
pub const NIVEL_MIN_PADRAO: i64 = 150;
/// Pokémon no tabuleiro.
pub const TAMANHO_TIME: usize = 5;

/// Casas padrão por alcance quando o meta ainda não tem amostra (frente → fundo).
pub fn casas_padrao(alcance: i64) -> &'static [i64] {
    match alcance {
        i64::MIN..=1 => &[2, 3, 1, 4, 5],
        2 => &[8, 7, 9, 6, 10],
        3 => &[17, 18, 16, 19, 20],
        _ => &[23, 22, 24, 21, 25],
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Escolha {
    pub id: i64,
    pub especie: i64,
    pub nome: String,
    pub nivel: i64,
    pub alcance: i64,
    pub casa: i64,
    pub nota: f64,
    pub motivo: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Sugestao {
    pub faixa: Option<i64>,
    pub partidas_no_meta: u32,
    pub time: Vec<Escolha>,
    /// `{pokemonId: casa}` pronto para o `pvp.time.salvar`.
    pub casas: serde_json::Map<String, Value>,
}

fn tipos_de(v: &Value) -> Vec<Tipo> {
    v.get("tipos")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|t| t.as_str().and_then(|s| Tipo::from_str(s).ok()))
        .collect()
}

/// Vantagem de tipo média contra o meta (log: 0 = neutro; > 0 = vantagem).
fn vantagem(meus: &[Tipo], meta: &[(Vec<Tipo>, f64)]) -> f64 {
    if meus.is_empty() || meta.is_empty() {
        return 0.0;
    }
    let (mut soma, mut peso) = (0.0, 0.0);
    for (deles, w) in meta {
        if deles.is_empty() {
            continue;
        }
        let ataque = meus
            .iter()
            .map(|t| efetividade_bruta(*t, deles))
            .fold(0.0_f64, f64::max)
            .max(0.25);
        let defesa = deles
            .iter()
            .map(|t| efetividade_bruta(*t, meus))
            .fold(0.0_f64, f64::max)
            .max(0.25);
        soma += w * (ataque.ln() - defesa.ln());
        peso += w;
    }
    if peso > 0.0 {
        soma / peso
    } else {
        0.0
    }
}

/// Monta a sugestão. `fichas`: a coleção da conta (campos do `/api/equipes/ingest`).
pub fn sugerir(fichas: &[Value], meta: &Meta, dex: Option<&Pokedex>, nivel_min: i64) -> Sugestao {
    // Tipos e peso das 20 espécies mais usadas no meta.
    let meta_tipos: Vec<(Vec<Tipo>, f64)> = meta
        .especies
        .iter()
        .take(20)
        .filter_map(|e| Some((dex?.por_id(e.especie)?.tipos.clone(), f64::from(e.usos))))
        .collect();
    let por_especie: HashMap<i64, &super::meta::Especie> =
        meta.especies.iter().map(|e| (e.especie, e)).collect();
    let n = |p: &Value, k: &str| p.get(k).and_then(Value::as_f64).unwrap_or(0.0);
    let candidatos: Vec<&Value> = fichas
        .iter()
        .filter(|p| n(p, "level") as i64 >= nivel_min && p.get("id").is_some())
        .collect();
    let max_poder = candidatos
        .iter()
        .map(|p| n(p, "poder"))
        .fold(1.0_f64, f64::max);
    let mut notas: Vec<(f64, &Value, String)> = candidatos
        .into_iter()
        .map(|p| {
            let mut tipos = tipos_de(p);
            let especie = n(p, "speciesId") as i64;
            if tipos.is_empty() {
                tipos = dex
                    .and_then(|d| d.por_id(especie))
                    .map(|e| e.tipos.clone())
                    .unwrap_or_default();
            }
            let forca = n(p, "poder") / max_poder;
            let v = vantagem(&tipos, &meta_tipos);
            let mut nota = forca * (0.3 * v).exp();
            let mut motivo = format!("força {:.0}%", forca * 100.0);
            if v.abs() > 0.05 {
                motivo += &format!(
                    ", tipo {:+.0}% contra o meta",
                    ((0.3 * v).exp() - 1.0) * 100.0
                );
            }
            if let Some(e) = por_especie.get(&especie).filter(|e| e.usos >= 5) {
                nota *= 0.8 + 0.4 * e.taxa;
                motivo += &format!(", vence {:.0}% no meta", e.taxa * 100.0);
            }
            (nota, p, motivo)
        })
        .collect();
    notas.sort_by(|a, b| b.0.total_cmp(&a.0));

    let mut usadas = HashSet::new();
    let mut time = Vec::new();
    for (nota, p, motivo) in notas.into_iter().take(TAMANHO_TIME) {
        let especie = n(p, "speciesId") as i64;
        let alcance = por_especie.get(&especie).map_or(1, |e| e.alcance);
        let do_meta: Vec<i64> = meta
            .alcances
            .iter()
            .find(|a| a.alcance == alcance)
            .map(|a| a.casas.iter().map(|c| c.0).collect())
            .unwrap_or_default();
        let casa = do_meta
            .iter()
            .copied()
            .chain(casas_padrao(alcance).iter().copied())
            .chain(1..=25)
            .find(|c| !usadas.contains(c))
            .unwrap_or(1);
        usadas.insert(casa);
        time.push(Escolha {
            id: p.get("id").and_then(Value::as_i64).unwrap_or(0),
            especie,
            nome: p
                .get("nome")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned(),
            nivel: n(p, "level") as i64,
            alcance,
            casa,
            nota: (nota * 1000.0).round() / 1000.0,
            motivo,
        });
    }
    let casas = time
        .iter()
        .map(|e| (e.id.to_string(), Value::from(e.casa)))
        .collect();
    Sugestao {
        faixa: meta.faixa,
        partidas_no_meta: meta.partidas,
        time,
        casas,
    }
}

#[cfg(test)]
mod testes {
    use super::*;
    use crate::pvp::meta::{agregar, testes::partida};
    use serde_json::json;

    #[test]
    fn escolhe_os_fortes_acima_do_nivel_e_casas_sem_repetir() {
        // Espécie 2 (alcance 3) vence quase sempre na casa 18.
        let ps: Vec<_> = (0..6)
            .map(|i| partida(i, 450, true, &[(2, 3, 18), (5, 1, 2)], &[(9, 1, 2)]))
            .collect();
        let meta = agregar(&ps, Some(400));
        let fichas: Vec<Value> = vec![
            json!({"id": 1, "speciesId": 2, "nome": "A", "level": 200, "poder": 900, "tipos": ["FIRE"]}),
            json!({"id": 2, "speciesId": 5, "nome": "B", "level": 200, "poder": 1000, "tipos": ["WATER"]}),
            json!({"id": 3, "speciesId": 7, "nome": "C", "level": 100, "poder": 5000}),
            json!({"id": 4, "speciesId": 5, "nome": "D", "level": 300, "poder": 800}),
        ];
        let s = sugerir(&fichas, &meta, None, NIVEL_MIN_PADRAO);
        let ids: Vec<i64> = s.time.iter().map(|e| e.id).collect();
        assert!(!ids.contains(&3), "nível abaixo do mínimo");
        assert_eq!(s.time.len(), 3);
        let a = s
            .time
            .iter()
            .find(|e| e.id == 1)
            .map(|e| (e.alcance, e.casa));
        assert_eq!(a, Some((3, 18)));
        let casas: HashSet<i64> = s.time.iter().map(|e| e.casa).collect();
        assert_eq!(casas.len(), 3, "casas distintas");
        assert_eq!(s.casas.len(), 3);
    }

    #[test]
    fn vantagem_de_tipo() {
        let fogo = [Tipo::from_str("FIRE").unwrap_or(Tipo::Normal)];
        let planta = vec![(vec![Tipo::from_str("GRASS").unwrap_or(Tipo::Normal)], 1.0)];
        assert!(vantagem(&fogo, &planta) > 0.0);
        let agua = vec![(vec![Tipo::from_str("WATER").unwrap_or(Tipo::Normal)], 1.0)];
        assert!(vantagem(&fogo, &agua) < 0.0);
    }
}
