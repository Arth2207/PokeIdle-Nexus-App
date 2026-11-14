//! Vendas do mercado (`market.historicoGlobal`) para o Dossiê, com o ritmo do motor do legado:
//! página 0 a cada 3 min; enquanto a página vier quase toda nova, volta uma página a cada 15 s
//! (até 36 h atrás ou 200 páginas). Uma volta em cada três pede todos os tipos; as outras, só
//! pokémon. Vendedor, comprador e apelido saem aqui (o servidor tira de novo).

use std::collections::{HashSet, VecDeque};

use serde_json::{json, Value};

use super::{Entrega, Fluxo};

pub const NORMAL_MS: i64 = 3 * 60_000;
pub const RETRO_MS: i64 = 15_000;
pub const MAX_PAGINAS: u32 = 200;
pub const RETRO_MAX_MS: i64 = 36 * 3_600_000;
/// Sem resposta em 20 s, a página é dada como perdida.
const ESPERA_MS: i64 = 20_000;
const LOTE: usize = 1000;
const MEMORIA_IDS: usize = 50_000;
const MAX_FILA: usize = 20_000;

#[derive(Debug, Clone, Copy, PartialEq)]
struct Pagina {
    pagina: u32,
    n: usize,
    novas: usize,
    tem_mais: bool,
    mais_antiga: i64,
}

pub struct Vendas {
    vistos: HashSet<String>,
    fila: VecDeque<Value>,
    pag: u32,
    voltas: u32,
    tudo: bool,
    ultima: Option<Pagina>,
    pedido_em: Option<i64>,
    /// Último pedido feito (o ritmo das páginas antigas conta daqui).
    ultimo: i64,
    normal_em: i64,
}

impl Default for Vendas {
    fn default() -> Self {
        Self::novo()
    }
}

fn id_de(v: &Value) -> Option<String> {
    match v.get("id")? {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn anonima(v: &Value) -> Value {
    let mut x = v.clone();
    if let Value::Object(o) = &mut x {
        o.remove("vendedor");
        o.remove("comprador");
        if let Some(Value::Object(f)) = o.get_mut("ficha") {
            f.remove("apelido");
        }
    }
    x
}

impl Vendas {
    pub fn novo() -> Self {
        Self {
            vistos: HashSet::new(),
            fila: VecDeque::new(),
            pag: 0,
            voltas: 0,
            tudo: false,
            ultima: None,
            pedido_em: None,
            ultimo: i64::MIN / 2,
            normal_em: i64::MIN / 2,
        }
    }

    /// A última página pede para continuar voltando?
    fn continua(&self, agora: i64) -> bool {
        let Some(u) = self.ultima else { return false };
        !self.tudo
            && u.pagina == self.pag
            && u.n > 0
            && u.novas >= u.n.min(25)
            && u.tem_mais
            && u.mais_antiga > agora - RETRO_MAX_MS
            && self.pag < MAX_PAGINAS
    }

    fn pedido(&self) -> Value {
        let mut o = json!({"t": "market.historicoGlobal", "pagina": self.pag});
        if !self.tudo {
            o["tipo"] = json!("pokemon");
        }
        o
    }
}

impl Fluxo for Vendas {
    fn nome(&self) -> &'static str {
        "vendas"
    }

    fn receber(&mut self, m: &Value, _agora: i64) {
        if m.get("t").and_then(Value::as_str) != Some("market")
            || m.get("aba").and_then(Value::as_str) != Some("historicoGlobal")
        {
            return;
        }
        let Some(linhas) = m.get("linhas").and_then(Value::as_array) else {
            return;
        };
        let (mut novas, mut mais_antiga) = (0, i64::MAX);
        for r in linhas {
            let Some(id) = id_de(r) else { continue };
            if let Some(em) = r.get("em").and_then(Value::as_f64) {
                mais_antiga = mais_antiga.min(em as i64);
            }
            if !self.vistos.insert(id) {
                continue;
            }
            novas += 1;
            self.fila.push_back(anonima(r));
        }
        if self.vistos.len() > MEMORIA_IDS {
            self.vistos.clear();
        }
        while self.fila.len() > MAX_FILA {
            self.fila.pop_front();
        }
        self.ultima = Some(Pagina {
            pagina: m.get("pagina").and_then(Value::as_u64).unwrap_or(0) as u32,
            n: linhas.len(),
            novas,
            tem_mais: m.get("temMais").and_then(Value::as_bool).unwrap_or(false),
            mais_antiga,
        });
        self.pedido_em = None;
    }

    fn pedir(&mut self, agora: i64) -> Option<Value> {
        if self.pedido_em.is_some_and(|t| agora - t < ESPERA_MS) {
            return None;
        }
        if self.continua(agora) {
            // Páginas antigas: uma a cada 15 s.
            if agora - self.ultimo < RETRO_MS {
                return None;
            }
            self.pag += 1;
        } else if agora - self.normal_em >= NORMAL_MS {
            self.pag = 0;
            self.voltas += 1;
            self.tudo = self.voltas.is_multiple_of(3);
            self.normal_em = agora;
        } else {
            return None;
        }
        self.ultima = None;
        self.pedido_em = Some(agora);
        self.ultimo = agora;
        Some(self.pedido())
    }

    fn entregar(&mut self, _agora: i64) -> Vec<Entrega> {
        let mut s = Vec::new();
        while !self.fila.is_empty() {
            let n = self.fila.len().min(LOTE);
            s.push(Entrega::Vendas(self.fila.drain(..n).collect()));
        }
        s
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    const T: i64 = 1_794_632_400_000;

    fn pagina(p: u32, ids: std::ops::Range<u32>, em: i64, mais: bool) -> Value {
        let linhas: Vec<Value> = ids
            .map(|i| {
                json!({"id": i, "em": em, "vendedor": "x", "comprador": "y",
                            "ficha": {"apelido": "z", "level": 5}})
            })
            .collect();
        json!({"t": "market", "aba": "historicoGlobal", "pagina": p, "linhas": linhas, "temMais": mais})
    }

    #[test]
    fn pagina_zero_volta_paginas_e_respeita_o_ritmo() {
        let mut v = Vendas::novo();
        assert_eq!(
            v.pedir(T),
            Some(json!({"t": "market.historicoGlobal", "pagina": 0, "tipo": "pokemon"}))
        );
        assert_eq!(v.pedir(T + 1_000), None, "esperando a resposta");
        v.receber(&pagina(0, 0..30, T - 60_000, true), T + 1_000);
        // Página toda nova: volta para a 1, mas só depois de 15 s.
        assert_eq!(v.pedir(T + 5_000), None);
        assert_eq!(
            v.pedir(T + 30_000).map(|p| p["pagina"].clone()),
            Some(json!(1))
        );
        // Página 1 só com repetidas: para e espera a próxima volta.
        v.receber(&pagina(1, 0..30, T - 60_000, true), T + 31_000);
        assert_eq!(v.pedir(T + 60_000), None);
        assert_eq!(
            v.pedir(T + NORMAL_MS).map(|p| p["pagina"].clone()),
            Some(json!(0))
        );
        let e = v.entregar(T);
        assert_eq!(e.len(), 1);
        let Entrega::Vendas(l) = &e[0] else {
            unreachable!("só vendas")
        };
        assert_eq!(l.len(), 30);
        assert_eq!(
            l[0],
            json!({"id": 0, "em": T - 60_000, "ficha": {"level": 5}})
        );
    }

    #[test]
    fn nao_volta_alem_de_36_horas_e_terceira_volta_pede_tudo() {
        let mut v = Vendas::novo();
        v.pedir(T);
        v.receber(&pagina(0, 0..30, T - RETRO_MAX_MS - 1, true), T);
        assert_eq!(v.pedir(T + 60_000), None);
        v.pedir(T + NORMAL_MS);
        v.receber(&pagina(0, 100..101, T, false), T + NORMAL_MS);
        let p = v.pedir(T + 2 * NORMAL_MS);
        assert_eq!(p, Some(json!({"t": "market.historicoGlobal", "pagina": 0})));
    }
}
