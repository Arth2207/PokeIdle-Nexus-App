//! Fila de eventos do jogo para o servidor. O lote em voo é reenviado idêntico (mesmo
//! `seq_cliente`) até o `evento.ack` com `ok`; o servidor também deduplica por conteúdo.

use contrato::ws::{EventoAck, EventoLote};
use serde_json::Value;
use std::collections::{HashSet, VecDeque};
use std::hash::{DefaultHasher, Hash, Hasher};

/// Quantos eventos já enviados lembrar para não reenfileirar o mesmo.
const MEMORIA: usize = 10_000;
/// Tamanho (JSON) máximo de um lote; o servidor aceita mensagens de até 4 MiB.
pub const LOTE_BYTES: usize = 512 * 1024;
/// Evento sozinho acima disto vira um marcador (`truncado`): nunca trava a fila.
pub const EVENTO_BYTES: usize = 3 * 1024 * 1024 + 128 * 1024;

#[derive(Debug)]
pub struct FilaEventos {
    conta: Option<String>,
    pendentes: VecDeque<(Option<String>, Value)>,
    em_voo: Option<EventoLote>,
    proximo_seq: u64,
    /// Hash do JSON de cada evento visto (o texto inteiro custaria memória demais).
    vistos: HashSet<u64>,
    ordem_vistos: VecDeque<u64>,
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
    ///
    /// Evento objeto com `_conta` (texto) é do lote dessa conta: o campo sai do evento antes de
    /// enviar. A chave de dedup inclui a conta. Sem `_conta`, vale a conta da fila.
    pub fn adicionar(&mut self, mut ev: Value) -> bool {
        let texto = ev.to_string();
        let mut h = DefaultHasher::new();
        texto.hash(&mut h);
        let chave = h.finish();
        if !self.vistos.insert(chave) {
            return false;
        }
        self.ordem_vistos.push_back(chave);
        while self.ordem_vistos.len() > MEMORIA {
            if let Some(velha) = self.ordem_vistos.pop_front() {
                self.vistos.remove(&velha);
            }
        }
        let conta = match ev.as_object_mut().and_then(|o| o.remove("_conta")) {
            Some(Value::String(c)) => Some(c),
            _ => self.conta.clone(),
        };
        if texto.len() > EVENTO_BYTES {
            let k = ev.get("k").cloned().unwrap_or(Value::Null);
            let ts = ev.get("ts").cloned().unwrap_or(Value::Null);
            ev = serde_json::json!({"k": k, "ts": ts, "truncado": true, "tam": texto.len()});
        }
        self.pendentes.push_back((conta, ev));
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
        let conta = self.pendentes.front().and_then(|(c, _)| c.clone());
        // Junta os eventos da conta da frente de qualquer ponto da fila (com várias contas os
        // eventos chegam intercalados; só os seguidos dava lotes de 1 ou 2), na ordem, até
        // `max` e [`LOTE_BYTES`] (sempre ao menos um). As outras contas ficam como estavam.
        let mut bytes = 0;
        let mut eventos = Vec::new();
        let mut resto = VecDeque::with_capacity(self.pendentes.len());
        let mut cheio = false;
        for (c, v) in self.pendentes.drain(..) {
            if cheio || c != conta {
                resto.push_back((c, v));
                continue;
            }
            let t = v.to_string().len() + 1;
            if !eventos.is_empty() && bytes + t > LOTE_BYTES {
                cheio = true;
                resto.push_back((c, v));
                continue;
            }
            bytes += t;
            eventos.push(v);
            cheio = eventos.len() >= max;
        }
        self.pendentes = resto;
        let lote = EventoLote {
            seq_cliente: self.proximo_seq,
            conta,
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
    fn lote_corta_por_tamanho_e_evento_enorme_vira_marcador() {
        let mut f = FilaEventos::nova(None, 1);
        let meio = "x".repeat(LOTE_BYTES / 2);
        for i in 0..3 {
            assert!(f.adicionar(json!({"k": "ws.bruto", "ts": i, "v": meio})));
        }
        let l = f.proximo_lote(50).map(|l| l.eventos.len());
        assert_eq!(l, Some(1));
        let mut g = FilaEventos::nova(None, 1);
        g.adicionar(json!({"k": "tracker.captura", "ts": 1, "v": "y".repeat(EVENTO_BYTES)}));
        let e = g.proximo_lote(50).map(|l| l.eventos[0].clone());
        assert_eq!(e.as_ref().map(|e| e["truncado"].clone()), Some(json!(true)));
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
    fn contas_intercaladas_saem_em_lotes_cheios_na_ordem() {
        let mut f = FilaEventos::nova(None, 1);
        for n in 0..12 {
            let c = ["a", "b", "c", "d"][n % 4];
            f.adicionar(json!({"k": "kill", "_conta": c, "n": n}));
        }
        let l = f.proximo_lote(10).unwrap_or_else(|| unreachable!());
        assert_eq!(l.conta.as_deref(), Some("a"));
        let ns: Vec<i64> = l.eventos.iter().filter_map(|e| e["n"].as_i64()).collect();
        assert_eq!(ns, vec![0, 4, 8]);
        f.confirmar(&EventoAck {
            seq_cliente: l.seq_cliente,
            ok: true,
            duplicados: 0,
            erros: 0,
        });
        let l = f.proximo_lote(2).unwrap_or_else(|| unreachable!());
        assert_eq!(l.conta.as_deref(), Some("b"));
        let ns: Vec<i64> = l.eventos.iter().filter_map(|e| e["n"].as_i64()).collect();
        assert_eq!(ns, vec![1, 5], "limite do lote; o 9 fica na fila");
        f.confirmar(&EventoAck {
            seq_cliente: l.seq_cliente,
            ok: true,
            duplicados: 0,
            erros: 0,
        });
        let l = f.proximo_lote(10).unwrap_or_else(|| unreachable!());
        assert_eq!(
            l.conta.as_deref(),
            Some("c"),
            "a ordem entre as contas segue a da fila"
        );
    }

    #[test]
    fn lote_agrupa_por_conta_do_evento() {
        let mut f = FilaEventos::nova(None, 1);
        f.adicionar(json!({"k":"kill","_conta":"a","n":1}));
        f.adicionar(json!({"k":"kill","_conta":"a","n":2}));
        f.adicionar(json!({"k":"kill","_conta":"b","n":3}));
        let l1 = f.proximo_lote(10);
        assert_eq!(l1.as_ref().and_then(|l| l.conta.clone()), Some("a".into()));
        assert_eq!(l1.as_ref().map(|l| l.eventos.len()), Some(2));
        assert!(l1.is_some_and(|l| l.eventos.iter().all(|e| e.get("_conta").is_none())));
        assert!(f.confirmar(&ack(1, true)));
        let l2 = f.proximo_lote(10);
        assert_eq!(l2.and_then(|l| l.conta), Some("b".into()));
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
