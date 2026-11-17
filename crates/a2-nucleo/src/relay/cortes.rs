//! Cortes do ranking do PvP: a cada [`CORTES_MS`] pede as páginas 1 e 5 da tabela semanal
//! (`pvp.tabela`) e manda só os pontos das posições 10, 50, 100 e 500 (evento `pvp.cortes`); e o
//! ranking mensal (`pvp.mensal`) de cada conta: os números dela, os cortes (10, 50, 100, 500 e
//! 2000) e as faixas de prêmio (evento `pvp.mensal`). Nunca vai nick de ninguém.

use std::collections::{BTreeMap, VecDeque};

use serde_json::{json, Value};

use super::{Entrega, Fluxo};

/// Intervalo entre duas leituras dos cortes.
pub const CORTES_MS: i64 = 3_600_000;
/// Posições acompanhadas.
pub const POSICOES: [i64; 4] = [10, 50, 100, 500];
/// Posições acompanhadas no mensal (as faixas de prêmio terminam em 100, 500 e 2000).
pub const POSICOES_MES: [i64; 5] = [10, 50, 100, 500, 2000];

pub struct Cortes {
    ligado: bool,
    pedido_em: Option<i64>,
    fila: VecDeque<i64>,
    /// Contas logadas (o mensal é pedido em cada uma: a parte `eu` é dela).
    contas: Vec<String>,
    /// Pedidos do mensal: (conta, página); conta vazia = a janela dona.
    fila_mes: VecDeque<(String, i64)>,
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
            contas: Vec::new(),
            fila_mes: VecDeque::new(),
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
        if !self.ligado {
            return;
        }
        match msg.get("t").and_then(Value::as_str) {
            Some("welcome") => {
                if !self.contas.iter().any(|c| c == conta) {
                    self.contas.push(conta.to_owned());
                }
                return;
            }
            Some("pvpMensal") => {
                self.prontos.push((conta.to_owned(), mensal(msg, agora)));
                return;
            }
            Some("pvpTabela") => {}
            _ => return,
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
        if self.fila.is_empty()
            && self.fila_mes.is_empty()
            && self.pedido_em.is_none_or(|t| agora - t >= CORTES_MS)
        {
            self.pedido_em = Some(agora);
            self.fila = VecDeque::from([1, 5]);
            // Mensal: a página 0 em cada conta (traz o `eu` dela) e as páginas 4 e 19 uma vez
            // (posições 500 e 2000; a página começa em 0).
            self.fila_mes = self.contas.iter().map(|c| (c.clone(), 0)).collect();
            self.fila_mes.push_back((String::new(), 4));
            self.fila_mes.push_back((String::new(), 19));
        }
        if let Some(pagina) = self.fila.pop_front() {
            return Some(json!({"t": "pvp.tabela", "pagina": pagina}));
        }
        let (conta, pagina) = self.fila_mes.pop_front()?;
        let mut p = json!({"t": "pvp.mensal", "pagina": pagina});
        if !conta.is_empty() {
            p["_conta"] = Value::from(conta);
        }
        Some(p)
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

/// Evento `pvp.mensal`: o mês, os números da conta, o fim do mês, as faixas de prêmio e os
/// pontos das posições acompanhadas que vieram nesta página (sem nicks).
fn mensal(msg: &Value, agora: i64) -> Value {
    let cortes: serde_json::Map<String, Value> = msg
        .pointer("/tabela/linhas")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|l| Some((l.get(0)?.as_i64()?, l.get(3)?.clone())))
        .filter(|(p, _)| POSICOES_MES.contains(p))
        .map(|(p, v)| (p.to_string(), v))
        .collect();
    json!({"k": "pvp.mensal", "ts": agora, "mes": msg.get("mes"), "fimEm": msg.get("fimEm"),
           "eu": msg.get("eu"), "faixas": msg.get("faixas"),
           "total": msg.pointer("/tabela/total"), "cortes": cortes})
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
            json!({"10": 9990, "50": 9950, "100": 9900, "500": 9500})
        );
        // Depois vêm as páginas do mensal; esgotadas, nada até a próxima hora.
        while c.pedir(4).is_some() {}
        assert_eq!(c.pedir(5), None);
        assert_eq!(c.pedir(CORTES_MS).unwrap_or_default()["pagina"], 1);
    }

    #[test]
    fn mensal_de_cada_conta_sem_nicks() {
        let mut c = Cortes::novo(true);
        c.receber_conta("Sikien", &json!({"t": "welcome"}), 0);
        let pedidos: Vec<Value> = std::iter::from_fn(|| c.pedir(0)).collect();
        assert_eq!(
            pedidos.len(),
            5,
            "2 semanais + mensal da conta + páginas 4 e 19"
        );
        assert_eq!(
            pedidos[2],
            json!({"t": "pvp.mensal", "pagina": 0, "_conta": "Sikien"})
        );
        c.receber_conta("Sikien", &json!({"t": "pvpMensal", "mes": "2026-11", "eu": {"pontos": 1832},
            "tabela": {"total": 2498, "linhas": [[10, 10, "Fulano", 18600, 1, 1, 2, ""], [11, 11, "X", 18500, 1, 1, 2, ""]]}}), 1);
        let e = c.entregar(1);
        let Some(Entrega::Eventos { eventos, .. }) = e.first() else {
            panic!("sem evento");
        };
        assert_eq!(eventos[0]["cortes"], json!({"10": 18600}));
        assert_eq!(eventos[0]["eu"]["pontos"], 1832);
        assert!(!eventos[0].to_string().contains("Fulano"));
    }
}
