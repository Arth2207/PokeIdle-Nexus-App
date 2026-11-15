//! Cortes do ranking do PvP: a cada [`CORTES_MS`] pede as páginas 1 e 5 da tabela
//! (`pvp.tabela`) e manda ao servidor só os pontos das posições 10, 100 e 500 (evento
//! `pvp.cortes`, sem nicks). É o que a tela de PvP usa para dizer quanto falta para cada corte.

use std::collections::{BTreeMap, VecDeque};

use serde_json::{json, Value};

use super::{Entrega, Fluxo};

/// Intervalo entre duas leituras dos cortes.
pub const CORTES_MS: i64 = 3_600_000;
/// Posições acompanhadas.
pub const POSICOES: [i64; 3] = [10, 100, 500];

pub struct Cortes {
    ligado: bool,
    pedido_em: Option<i64>,
    fila: VecDeque<i64>,
    /// Posição → pontos, da leitura atual.
    vistos: BTreeMap<i64, i64>,
    semana: String,
    total: i64,
    prontos: Vec<(String, Value)>,
}

impl Cortes {
    pub fn novo(ligado: bool) -> Self {
        Self {
            ligado,
            pedido_em: None,
            fila: VecDeque::new(),
            vistos: BTreeMap::new(),
            semana: String::new(),
            total: 0,
            prontos: Vec::new(),
        }
    }
}

impl Fluxo for Cortes {
    fn nome(&self) -> &'static str {
        "cortes"
    }

    fn receber(&mut self, _msg: &Value, _agora: i64) {}

    fn receber_conta(&mut self, conta: &str, msg: &Value, agora: i64) {
        if !self.ligado || msg.get("t").and_then(Value::as_str) != Some("pvpTabela") {
            return;
        }
        self.semana = msg
            .get("semana")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        self.total = msg.get("total").and_then(Value::as_i64).unwrap_or(0);
        for l in msg
            .get("linhas")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let (Some(pos), Some(pts)) = (
                l.get(0).and_then(Value::as_i64),
                l.get(4).and_then(Value::as_i64),
            ) {
                if POSICOES.contains(&pos) {
                    self.vistos.insert(pos, pts);
                }
            }
        }
        // Fim da leitura (as duas páginas pedidas já voltaram): manda o que viu.
        if self.fila.is_empty() && self.pedido_em.is_some() {
            let cortes: serde_json::Map<String, Value> = self
                .vistos
                .iter()
                .map(|(p, v)| (p.to_string(), json!(v)))
                .collect();
            self.prontos.push((
                conta.to_owned(),
                json!({"k": "pvp.cortes", "ts": agora, "semana": self.semana,
                       "total": self.total, "cortes": cortes}),
            ));
            self.vistos.clear();
        }
    }

    fn pedir(&mut self, agora: i64) -> Option<Value> {
        if !self.ligado {
            return None;
        }
        if self.fila.is_empty() && self.pedido_em.is_none_or(|t| agora - t >= CORTES_MS) {
            self.pedido_em = Some(agora);
            self.fila = VecDeque::from([1, 5]);
        }
        let pagina = self.fila.pop_front()?;
        Some(json!({"t": "pvp.tabela", "pagina": pagina}))
    }

    fn entregar(&mut self, _agora: i64) -> Vec<Entrega> {
        std::mem::take(&mut self.prontos)
            .into_iter()
            .map(|(conta, ev)| Entrega::Eventos {
                conta,
                eventos: vec![ev],
            })
            .collect()
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    fn linhas(de: i64, ate: i64) -> Vec<Value> {
        (de..=ate)
            .map(|p| json!([p, "x", "ouro", "", 10_000 - p, 1, 1]))
            .collect()
    }

    #[test]
    fn le_paginas_1_e_5_e_manda_os_cortes() {
        let mut c = Cortes::novo(true);
        assert_eq!(c.pedir(0), Some(json!({"t": "pvp.tabela", "pagina": 1})));
        c.receber_conta("Sikien", &json!({"t": "pvpTabela", "semana": "2026-11-15", "total": 1000, "linhas": linhas(1, 100)}), 1);
        assert!(c.entregar(1).is_empty(), "falta a página 5");
        assert_eq!(c.pedir(2), Some(json!({"t": "pvp.tabela", "pagina": 5})));
        c.receber_conta("Sikien", &json!({"t": "pvpTabela", "semana": "2026-11-15", "total": 1000, "linhas": linhas(401, 500)}), 3);
        let e = c.entregar(3);
        let Some(Entrega::Eventos { eventos, .. }) = e.first() else {
            panic!("sem evento");
        };
        assert_eq!(
            eventos[0]["cortes"],
            json!({"10": 9990, "100": 9900, "500": 9500})
        );
        assert_eq!(c.pedir(4), None);
        assert_eq!(c.pedir(CORTES_MS).unwrap_or_default()["pagina"], 1);
    }
}
