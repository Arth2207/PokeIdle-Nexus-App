//! Fichas dos bosses de raid (`raid.fichas`): pede a cada 6 h na conta dona. A resposta
//! (`raidFichas`) vira evento `boss.fichas` pelo coletor, sem passar por aqui.

use serde_json::{json, Value};

use super::{Entrega, Fluxo};

pub const INTERVALO_MS: i64 = 6 * 3_600_000;

pub struct Raid {
    pedido_em: Option<i64>,
}

impl Raid {
    pub fn novo() -> Self {
        Self { pedido_em: None }
    }
}

impl Default for Raid {
    fn default() -> Self {
        Self::novo()
    }
}

impl Fluxo for Raid {
    fn nome(&self) -> &'static str {
        "raid"
    }

    fn receber(&mut self, _msg: &Value, _agora: i64) {}

    fn pedir(&mut self, agora: i64) -> Option<Value> {
        if self.pedido_em.is_some_and(|t| agora - t < INTERVALO_MS) {
            return None;
        }
        self.pedido_em = Some(agora);
        Some(json!({"t": "raid.fichas"}))
    }

    fn entregar(&mut self, _agora: i64) -> Vec<Entrega> {
        Vec::new()
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn pede_a_cada_seis_horas() {
        let mut r = Raid::novo();
        assert!(r.pedir(0).is_some());
        assert!(r.pedir(INTERVALO_MS - 1).is_none());
        assert!(r.pedir(INTERVALO_MS).is_some());
    }
}
