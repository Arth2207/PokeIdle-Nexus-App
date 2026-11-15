//! Extrato do Mercado de cada conta: a cada 30 min pede ao jogo as vendas (`market.historico`)
//! e as compras (`market.historicoCompras`) da primeira página e manda ao servidor como evento
//! `mercado.extrato`. É o que dá às Finanças o lado em orbs (e as vendas em gold), que o diário
//! do jogo não traz. Sai sem o nome do outro jogador.

use std::collections::{BTreeMap, VecDeque};

use serde_json::{json, Value};

use super::{Entrega, Fluxo};

/// Intervalo entre duas leituras do extrato da mesma conta.
pub const EXTRATO_MS: i64 = 30 * 60_000;

pub struct Extrato {
    ligado: bool,
    /// Conta → último pedido.
    contas: BTreeMap<String, Option<i64>>,
    fila: VecDeque<(String, &'static str)>,
    prontos: Vec<(String, Value)>,
}

impl Extrato {
    pub fn novo(ligado: bool) -> Self {
        Self {
            ligado,
            contas: BTreeMap::new(),
            fila: VecDeque::new(),
            prontos: Vec::new(),
        }
    }
}

/// Linha do extrato sem o nome do outro jogador e sem a ficha inteira do pokémon.
fn linha(l: &Value) -> Value {
    let mut o = serde_json::Map::new();
    for k in ["em", "id", "tipo", "bruto", "valor", "moeda", "descricao"] {
        if let Some(v) = l.get(k) {
            o.insert(k.into(), v.clone());
        }
    }
    if let Some(f) = l.get("ficha").filter(|f| f.is_object()) {
        o.insert(
            "especie".into(),
            f.get("speciesId").cloned().unwrap_or(Value::Null),
        );
    }
    Value::Object(o)
}

impl Fluxo for Extrato {
    fn nome(&self) -> &'static str {
        "extrato"
    }

    fn receber(&mut self, _msg: &Value, _agora: i64) {}

    fn receber_conta(&mut self, conta: &str, msg: &Value, agora: i64) {
        if !self.ligado {
            return;
        }
        match msg.get("t").and_then(Value::as_str) {
            Some("welcome") => {
                self.contas.entry(conta.to_owned()).or_insert(None);
            }
            Some("market") => {
                let aba = msg.get("aba").and_then(Value::as_str).unwrap_or("");
                if aba != "historico" && aba != "historicoCompras" {
                    return;
                }
                let linhas: Vec<Value> = msg
                    .get("linhas")
                    .and_then(Value::as_array)
                    .map(|l| l.iter().map(linha).collect())
                    .unwrap_or_default();
                let lado = if aba == "historico" {
                    "venda"
                } else {
                    "compra"
                };
                self.prontos.push((
                    conta.to_owned(),
                    json!({"k": "mercado.extrato", "ts": agora, "lado": lado, "linhas": linhas}),
                ));
            }
            _ => {}
        }
    }

    fn pedir(&mut self, agora: i64) -> Option<Value> {
        if !self.ligado {
            return None;
        }
        if self.fila.is_empty() {
            for (c, ultimo) in &mut self.contas {
                if ultimo.is_none_or(|t| agora - t >= EXTRATO_MS) {
                    *ultimo = Some(agora);
                    self.fila.push_back((c.clone(), "market.historico"));
                    self.fila.push_back((c.clone(), "market.historicoCompras"));
                }
            }
        }
        let (conta, t) = self.fila.pop_front()?;
        Some(json!({"t": t, "pagina": 0, "_conta": conta}))
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

    #[test]
    fn pede_vendas_e_compras_de_cada_conta_a_cada_30_min() {
        let mut x = Extrato::novo(true);
        assert_eq!(x.pedir(0), None, "sem conta logada");
        x.receber_conta("Sikien", &json!({"t": "welcome"}), 0);
        assert_eq!(
            x.pedir(0),
            Some(json!({"t": "market.historico", "pagina": 0, "_conta": "Sikien"}))
        );
        assert_eq!(
            x.pedir(1).unwrap_or_default()["t"],
            "market.historicoCompras"
        );
        assert_eq!(x.pedir(2), None);
        assert_eq!(x.pedir(EXTRATO_MS - 1), None);
        assert_eq!(
            x.pedir(EXTRATO_MS).unwrap_or_default()["t"],
            "market.historico"
        );
    }

    #[test]
    fn resposta_vira_evento_sem_o_outro_jogador() {
        let mut x = Extrato::novo(true);
        x.receber_conta(
            "Sikien",
            &json!({"t": "market", "aba": "historico", "linhas": [
                {"em": 5, "moeda": "orb", "valor": 76, "bruto": 90, "comprador": "Gato",
                 "descricao": "30× Bronze Boss Token", "ficha": {"speciesId": 123, "apelido": "x"}}]}),
            7,
        );
        let e = x.entregar(7);
        let Some(Entrega::Eventos { conta, eventos }) = e.first() else {
            panic!("sem entrega");
        };
        assert_eq!(conta, "Sikien");
        assert_eq!(eventos[0]["lado"], "venda");
        assert_eq!(
            eventos[0]["linhas"][0],
            json!({"em": 5, "moeda": "orb", "valor": 76, "bruto": 90,
                   "descricao": "30× Bronze Boss Token", "especie": 123})
        );
        assert!(Extrato::novo(false).pedir(0).is_none());
    }
}
