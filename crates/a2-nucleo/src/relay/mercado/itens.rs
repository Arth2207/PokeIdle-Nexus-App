//! Anúncios de itens: `market.itens` traz o resumo (preço mínimo por moeda); depois
//! `market.item` para cada item e moeda com anúncio (resposta aba `item`).

use super::oferta::ofertas_da_resposta;
use serde_json::{json, Value};

/// Sem resumo nesse prazo, a fase de itens termina (tenta na próxima volta).
pub const ESPERA_RESUMO_MS: i64 = 15_000;
pub const ESPERA_ITEM_MS: i64 = 10_000;

#[derive(Debug, Default)]
pub struct VarreduraItens {
    resumo_pedido: Option<i64>,
    fila: Option<Vec<(u64, &'static str)>>,
    aguardando: Option<i64>,
    acabou: bool,
    pub ofertas: Vec<Value>,
}

impl VarreduraItens {
    pub fn nova() -> Self {
        Self::default()
    }

    pub fn concluida(&self) -> bool {
        self.acabou
    }

    pub fn pedir(&mut self, agora: i64) -> Option<Value> {
        if self.acabou {
            return None;
        }
        let Some(fila) = &mut self.fila else {
            return match self.resumo_pedido {
                None => {
                    self.resumo_pedido = Some(agora);
                    Some(json!({"t": "market.itens", "categoria": "", "elementos": []}))
                }
                Some(t) if agora - t >= ESPERA_RESUMO_MS => {
                    self.acabou = true;
                    None
                }
                Some(_) => None,
            };
        };
        if self.aguardando.is_some_and(|t| agora - t < ESPERA_ITEM_MS) {
            return None;
        }
        match fila.pop() {
            Some((id, moeda)) => {
                self.aguardando = Some(agora);
                Some(json!({"t": "market.item", "itemId": id, "moeda": moeda}))
            }
            None => {
                self.acabou = true;
                None
            }
        }
    }

    pub fn receber(&mut self, m: &Value) {
        match m.get("aba").and_then(Value::as_str) {
            Some("itens") if self.fila.is_none() => {
                if let Some(r) = m.get("resumo").and_then(Value::as_object) {
                    let mut fila = Vec::new();
                    for (id, x) in r {
                        let Ok(id) = id.parse::<u64>() else { continue };
                        for (moeda, campo) in [("orb", "minOrb"), ("gold", "minGold")] {
                            if x.get(campo).and_then(Value::as_f64).unwrap_or(0.0) > 0.0 {
                                fila.push((id, moeda));
                            }
                        }
                    }
                    fila.reverse();
                    self.fila = Some(fila);
                }
            }
            Some("item") => {
                self.ofertas.extend(ofertas_da_resposta(m));
                self.aguardando = None;
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn resumo_vira_fila_por_moeda_e_acaba() {
        let mut v = VarreduraItens::nova();
        assert_eq!(
            v.pedir(0).map(|p| p["t"].clone()),
            Some(json!("market.itens"))
        );
        v.receber(&json!({"t": "market", "aba": "itens",
            "resumo": {"23": {"minOrb": 5, "minGold": 0}, "40": {"minOrb": 1, "minGold": 9}}}));
        let mut pedidos = Vec::new();
        while let Some(p) = v.pedir(0) {
            pedidos.push((p["itemId"].clone(), p["moeda"].clone()));
            v.receber(
                &json!({"t": "market", "aba": "item", "itemId": p["itemId"], "moeda": p["moeda"],
                "linhas": [{"id": 1, "preco": 2}]}),
            );
        }
        assert_eq!(pedidos.len(), 3);
        assert!(v.concluida());
        assert_eq!(v.ofertas.len(), 3);
    }

    #[test]
    fn sem_resumo_desiste() {
        let mut v = VarreduraItens::nova();
        assert!(v.pedir(0).is_some());
        assert!(v.pedir(ESPERA_RESUMO_MS).is_none());
        assert!(v.concluida());
    }
}
