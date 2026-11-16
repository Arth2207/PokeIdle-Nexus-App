//! Top Catch: semana de Brasília, elegibilidade conservadora e marcos anunciados.
use crate::combate::especie::EspecieCombate;
use serde_json::{json, Value};

const DIA: i64 = 86_400_000;
pub const SEMANA: i64 = 7 * DIA;

/// Domingo 00h de Brasília (UTC−3), independente do fuso do processo.
pub fn inicio_semana(agora: i64) -> i64 {
    let local = agora - 3 * 3_600_000;
    let dia = local.div_euclid(DIA);
    let domingo = dia - (dia + 4).rem_euclid(7); // 1970-01-01 foi quinta-feira.
    domingo * DIA + 3 * 3_600_000
}

/// Espécies comuns capturáveis: não assume elegibilidade de Outlands/Beast Ball.
pub fn elegivel(sp: &EspecieCombate) -> bool {
    sp.categoria.as_deref() == Some("pokebola") && !(2000..3000).contains(&sp.id)
}

pub const MARCOS: &[(u64, &str)] = &[
    (10, "XP Boost Trainer · 30 min"),
    (25, "XP Boost Pokémon · 30 min"),
    (50, "XP Trainer + Pokémon · 1 h"),
    (100, "Shiny Secret Lure · 30 min"),
    (150, "XP Trainer + Pokémon + Lure · 30 min"),
    (200, "XP Trainer + Pokémon + Lure · 1 h"),
    (300, "XP Trainer + Pokémon + Lure · 2 h"),
    (400, "Lure · 12 h"),
    (600, "1.000 Beast Balls + Lure · 24 h"),
];

/// Acrescenta os marcos à estimativa local, sem afirmar resgate/pagamento pelo servidor.
pub fn com_marcos(mut dados: Value) -> Value {
    if let Some(tc) = dados.get_mut("top_catch") {
        let especies = tc["especies"].as_u64().unwrap_or(0);
        let ritmo = tc["por_hora"]
            .as_f64()
            .filter(|r| r.is_finite() && *r > 0.0);
        let marcos: Vec<Value> = MARCOS
            .iter()
            .map(|(meta, premio)| {
                let faltam = meta.saturating_sub(especies);
                json!({"meta":meta, "premio":premio, "atingido":faltam == 0,
                "faltam":faltam, "horas":ritmo.map(|r| faltam as f64 / r)})
            })
            .collect();
        tc["proximo"] = marcos
            .iter()
            .find(|m| m["atingido"] == false)
            .cloned()
            .unwrap_or(Value::Null);
        tc["marcos"] = json!(marcos);
    }
    dados
}

#[cfg(test)]
mod testes {
    use super::*;
    #[test]
    fn reset_domingo_em_brasilia_e_nao_no_fuso_do_host() {
        let domingo = crate::js::dia_da_data("2026-11-15").unwrap_or(0) * DIA + 3 * 3_600_000;
        assert_eq!(inicio_semana(domingo), domingo);
        assert_eq!(inicio_semana(domingo - 1), domingo - SEMANA);
        assert_eq!(inicio_semana(domingo + SEMANA - 1), domingo);
        assert_eq!(inicio_semana(domingo + SEMANA), domingo + SEMANA);
    }
    #[test]
    fn marcos_nao_sao_pagamentos_confirmados() {
        let d = com_marcos(json!({"top_catch":{"especies":150,"por_hora":10}}));
        assert_eq!(d["top_catch"]["proximo"]["meta"], 200);
        assert_eq!(d["top_catch"]["proximo"]["horas"], 5.0);
        assert_eq!(d["top_catch"]["marcos"][4]["atingido"], true);
        let d = com_marcos(json!({"top_catch":{"especies":600,"por_hora":0}}));
        assert!(d["top_catch"]["proximo"].is_null());
        assert!(d["top_catch"]["marcos"][0]["horas"].is_null());
    }
}
