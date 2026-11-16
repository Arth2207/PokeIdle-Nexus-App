//! Catálogo de Outlands recebido do jogo; sem inferir tier pelo nível do treinador.
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Tier {
    pub tier: i64,
    pub numero: i64,
    pub nivel: f64,
    pub mult: f64,
    pub rotulo: String,
}

pub fn tiers(w: &Value) -> Vec<Tier> {
    w.get("outlandTiers")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|v| serde_json::from_value::<Tier>(v.clone()).ok())
        .filter(|t| {
            t.tier > 0
                && t.numero > 0
                && t.nivel.is_finite()
                && t.nivel > 0.0
                && t.mult.is_finite()
                && t.mult > 0.0
                && !t.rotulo.is_empty()
        })
        .collect()
}

pub fn atual(w: &Value, u: &Value) -> Option<Tier> {
    let id = u
        .get("outlandTier")
        .or_else(|| u.pointer("/io/player/outlandTier"))
        .and_then(Value::as_i64)?;
    tiers(w).into_iter().find(|t| t.tier == id)
}

pub fn chance_token(w: &Value) -> Option<f64> {
    w.get("chanceBossToken")
        .and_then(Value::as_f64)
        .filter(|p| p.is_finite() && *p >= 0.0 && *p <= 1.0)
}

#[cfg(test)]
mod testes {
    use super::*;
    use serde_json::json;
    #[test]
    fn id_nao_e_numero_e_nivel_nao_escolhe_tier() {
        let w = json!({"outlandTiers":[{"tier":104,"numero":4,"nivel":1000,"mult":1.25,"rotulo":"Outland 4"}],"chanceBossToken":0.0001});
        let u = json!({"outlandTier":104,"trLv":65000});
        assert_eq!(atual(&w, &u).map(|t| (t.numero, t.mult)), Some((4, 1.25)));
        assert_eq!(chance_token(&w), Some(0.0001));
        assert!(atual(&w, &json!({"outlandTier":4})).is_none());
        assert!(chance_token(&json!({"chanceBossToken":2})).is_none());
        assert!(tiers(&json!({"outlandTiers":[{"tier":0}]})).is_empty());
    }
}
