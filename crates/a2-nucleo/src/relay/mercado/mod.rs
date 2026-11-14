//! Mercado: ciclo pokémon → itens → pausa, como o motor antigo (uma volta a cada ~5 min).

pub mod itens;
pub mod oferta;
pub mod pokemon;

use super::{Entrega, Fluxo};
use itens::VarreduraItens;
use pokemon::{Alvo, VarreduraPokemon};
use serde_json::Value;

/// Pausa entre uma volta completa e a próxima.
pub const PAUSA_MS: i64 = 5 * 60_000;
/// Ofertas por envio ao servidor (o relay antigo mandava 500).
pub const LOTE_OFERTAS: usize = 500;
/// Alvos vencidos voltam à varredura por faixas.
pub const ALVOS_VALIDADE_MS: i64 = 6 * 3_600_000;

#[derive(Debug)]
enum Fase {
    Pokemon(VarreduraPokemon),
    Itens(VarreduraItens),
    Pausa { ate: i64 },
}

pub struct Mercado {
    fase: Fase,
    alvos: Vec<Alvo>,
    alvos_em: i64,
    prontas: Vec<Value>,
    /// Voltas completas (para o diário/telemetria).
    pub voltas: u64,
}

impl Default for Mercado {
    fn default() -> Self {
        Self::novo()
    }
}

impl Mercado {
    pub fn novo() -> Self {
        Self {
            fase: Fase::Pausa { ate: 0 },
            alvos: Vec::new(),
            alvos_em: 0,
            prontas: Vec::new(),
            voltas: 0,
        }
    }

    fn recolher(&mut self) {
        let o = match &mut self.fase {
            Fase::Pokemon(v) => std::mem::take(&mut v.ofertas),
            Fase::Itens(v) => std::mem::take(&mut v.ofertas),
            Fase::Pausa { .. } => Vec::new(),
        };
        self.prontas.extend(o);
    }
}

impl Fluxo for Mercado {
    fn nome(&self) -> &'static str {
        "mercado"
    }

    fn receber(&mut self, msg: &Value, _agora: i64) {
        if msg.get("t").and_then(Value::as_str) != Some("market") {
            return;
        }
        match &mut self.fase {
            Fase::Pokemon(v) => v.receber(msg),
            Fase::Itens(v) => v.receber(msg),
            // Fora da varredura (o dono navegando), a vitrine também vale como oferta.
            Fase::Pausa { .. } => self.prontas.extend(oferta::ofertas_da_resposta(msg)),
        }
        self.recolher();
    }

    fn pedir(&mut self, agora: i64) -> Option<Value> {
        loop {
            match &mut self.fase {
                Fase::Pausa { ate } if agora < *ate => return None,
                Fase::Pausa { .. } => {
                    let validos = agora - self.alvos_em < ALVOS_VALIDADE_MS;
                    let alvos = if validos { &self.alvos[..] } else { &[] };
                    self.fase = Fase::Pokemon(VarreduraPokemon::nova(alvos));
                }
                Fase::Pokemon(v) if v.concluida() => {
                    self.recolher();
                    self.fase = Fase::Itens(VarreduraItens::nova());
                }
                Fase::Itens(v) if v.concluida() => {
                    self.recolher();
                    self.voltas += 1;
                    self.fase = Fase::Pausa {
                        ate: agora + PAUSA_MS,
                    };
                }
                Fase::Pokemon(v) => {
                    if let Some(p) = v.pedir(agora) {
                        return Some(p);
                    }
                    if !v.concluida() {
                        return None;
                    }
                }
                Fase::Itens(v) => {
                    if let Some(p) = v.pedir(agora) {
                        return Some(p);
                    }
                    if !v.concluida() {
                        return None;
                    }
                }
            }
        }
    }

    /// Espécies alvo do servidor; valem a partir da próxima volta.
    fn definir_alvos(&mut self, alvos: &[Alvo], agora: i64) {
        self.alvos = alvos.to_vec();
        self.alvos_em = agora;
    }

    fn entregar(&mut self, _agora: i64) -> Vec<Entrega> {
        let todas = std::mem::take(&mut self.prontas);
        todas
            .chunks(LOTE_OFERTAS)
            .map(|c| Entrega::Ofertas(c.to_vec()))
            .collect()
    }
}

#[cfg(test)]
mod testes {
    use super::*;
    use serde_json::json;

    #[test]
    fn volta_completa_entrega_e_pausa() {
        let mut m = Mercado::novo();
        let mut t = 0;
        let mut n = 0;
        while let Some(p) = m.pedir(t) {
            n += 1;
            let resp = match p["t"].as_str() {
                Some("market.listar") => json!({"t": "market", "aba": "vitrine",
                    "pagina": p["pagina"], "linhas": [{"id": n, "preco": 1}]}),
                _ => json!({"t": "market", "aba": "itens", "resumo": {}}),
            };
            m.receber(&resp, t);
            t += 1;
            assert!(n < 100, "não terminou");
        }
        assert_eq!(m.voltas, 1);
        assert_eq!(m.pedir(t + 1), None);
        let e = m.entregar(0);
        assert!(matches!(&e[..], [Entrega::Ofertas(o)] if o.len() == 20));
        assert!(m.pedir(t + PAUSA_MS + 1).is_some());
    }
}
