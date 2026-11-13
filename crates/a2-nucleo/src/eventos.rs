//! Fila de eventos do jogo para o servidor. O lote em voo é reenviado idêntico (mesmo
//! `seq_cliente`) até o `evento.ack` com `ok`; o servidor também deduplica por conteúdo.

use contrato::ws::{EventoAck, EventoLote};
use serde_json::Value;
use std::collections::{HashSet, VecDeque};

/// Quantos eventos já enviados lembrar para não reenfileirar o mesmo.
const MEMORIA: usize = 10_000;

#[derive(Debug)]
pub struct FilaEventos {
    conta: Option<String>,
    pendentes: VecDeque<(String, Value)>,
    em_voo: Option<EventoLote>,
    proximo_seq: u64,
    vistos: HashSet<String>,
    ordem_vistos: VecDeque<String>,
}

impl FilaEventos {
    /// `seq_inicial` deve crescer entre execuções do app (ex.: o relógio em ms).
    pub fn nova(conta: Option<String>, seq_inicial: u64) -> Self {
        Self {
            conta,
            pendentes: VecDeque::new(),
            em_voo: None,
            proximo_seq: seq_inicial,
            vistos: HashSet::new(),
            ordem_vistos: VecDeque::new(),
        }
    }

    /// `false` se o evento já foi enfileirado ou enviado (não duplica).
    pub fn adicionar(&mut self, ev: Value) -> bool {
        let chave = ev.to_string();
        if !self.vistos.insert(chave.clone()) {
            return false;
        }
        self.ordem_vistos.push_back(chave.clone());
        while self.ordem_vistos.len() > MEMORIA {
            if let Some(velha) = self.ordem_vistos.pop_front() {
                self.vistos.remove(&velha);
            }
        }
        self.pendentes.push_back((chave, ev));
        true
    }

    pub fn tem_trabalho(&self) -> bool {
        self.em_voo.is_some() || !self.pendentes.is_empty()
    }

    pub fn em_voo(&self) -> bool {
        self.em_voo.is_some()
    }

    /// Lote a enviar: o que está em voo (reenvio idêntico) ou um novo de até `max` eventos.
    pub fn proximo_lote(&mut self, max: usize) -> Option<EventoLote> {
        if let Some(l) = &self.em_voo {
            return Some(l.clone());
        }
        if self.pendentes.is_empty() || max == 0 {
            return None;
        }
        let n = max.min(self.pendentes.len());
        let eventos: Vec<Value> = self.pendentes.drain(..n).map(|(_, v)| v).collect();
        let lote = EventoLote {
            seq_cliente: self.proximo_seq,
            conta: self.conta.clone(),
            eventos,
        };
        self.proximo_seq += 1;
        self.em_voo = Some(lote.clone());
        Some(lote)
    }

    /// Trata o `evento.ack`. `true` se encerrou o lote em voo; ack de outro lote ou com
    /// `ok=false` mantém o lote para reenvio.
    pub fn confirmar(&mut self, ack: &EventoAck) -> bool {
        match &self.em_voo {
            Some(l) if l.seq_cliente == ack.seq_cliente && ack.ok => {
                self.em_voo = None;
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod testes {
    use super::*;
    use serde_json::json;

    fn ack(seq: u64, ok: bool) -> EventoAck {
        EventoAck {
            seq_cliente: seq,
            ok,
            duplicados: 0,
            erros: 0,
        }
    }

    #[test]
    fn evento_repetido_nao_entra() {
        let mut f = FilaEventos::nova(None, 1);
        assert!(f.adicionar(json!({"t":"kill","n":1})));
        assert!(!f.adicionar(json!({"t":"kill","n":1})));
        assert!(f.adicionar(json!({"t":"kill","n":2})));
        assert_eq!(f.proximo_lote(10).map(|l| l.eventos.len()), Some(2));
    }

    #[test]
    fn reenvio_e_identico_ate_ack_ok() {
        let mut f = FilaEventos::nova(Some("a".into()), 100);
        f.adicionar(json!(1));
        f.adicionar(json!(2));
        f.adicionar(json!(3));
        let l1 = f.proximo_lote(2);
        assert_eq!(f.proximo_lote(2), l1, "sem ack, mesmo lote");
        assert!(!f.confirmar(&ack(100, false)));
        assert!(!f.confirmar(&ack(7, true)));
        assert_eq!(f.proximo_lote(2), l1);
        assert!(f.confirmar(&ack(100, true)));
        let l2 = f.proximo_lote(2);
        assert_eq!(l2.as_ref().map(|l| l.seq_cliente), Some(101));
        assert_eq!(l2.map(|l| l.eventos), Some(vec![json!(3)]));
        assert!(!f.adicionar(json!(1)), "já enviado não volta");
    }
}
