//! Fila de saída do relay: junta ofertas e vendas em lotes maiores, guarda o que falhou e
//! tenta de novo com espera crescente. Estados que se substituem (equipe da mesma conta,
//! inventário somado, raros) ficam só com o mais novo.

use std::collections::VecDeque;

use super::Entrega;

/// Ofertas e vendas esperam juntar este tanto (ou [`ESPERA_LOTE_MS`]) antes de sair.
pub const LOTE: usize = 500;
pub const ESPERA_LOTE_MS: i64 = 30_000;
/// Espera depois de uma falha: 5 s, dobrando até 5 min.
const ESPERA_MIN_MS: i64 = 5_000;
const ESPERA_MAX_MS: i64 = 5 * 60_000;
/// Itens guardados no máximo (o mais velho sai primeiro).
const MAX_FILA: usize = 400;

pub struct Saida {
    fila: VecDeque<(Entrega, i64)>,
    falhas: u32,
    livre_em: i64,
}

impl Default for Saida {
    fn default() -> Self {
        Self::nova()
    }
}

/// Mesmo "lugar" (o novo substitui o velho)?
fn substitui(velho: &Entrega, novo: &Entrega) -> bool {
    match (velho, novo) {
        (Entrega::Equipe(a), Entrega::Equipe(b)) => a.get("nick") == b.get("nick"),
        (Entrega::MeusItens(_), Entrega::MeusItens(_)) | (Entrega::Raros(_), Entrega::Raros(_)) => {
            true
        }
        (Entrega::Diario { conta: a, .. }, Entrega::Diario { conta: b, .. })
        | (Entrega::Janela { conta: a, .. }, Entrega::Janela { conta: b, .. }) => a == b,
        _ => false,
    }
}

fn tamanho(e: &Entrega) -> usize {
    match e {
        Entrega::Ofertas(v) | Entrega::Vendas(v) => v.len(),
        _ => 0,
    }
}

impl Saida {
    pub fn nova() -> Self {
        Self {
            fila: VecDeque::new(),
            falhas: 0,
            livre_em: i64::MIN / 2,
        }
    }

    pub fn len(&self) -> usize {
        self.fila.len()
    }

    pub fn is_empty(&self) -> bool {
        self.fila.is_empty()
    }

    pub fn guardar(&mut self, e: Entrega, agora: i64) {
        // Ofertas e vendas entram no lote aberto do mesmo tipo (o último da fila), até LOTE.
        if let Some((ultimo, _)) = self.fila.back_mut() {
            match (ultimo, &e) {
                (Entrega::Ofertas(a), Entrega::Ofertas(b))
                | (Entrega::Vendas(a), Entrega::Vendas(b))
                    if a.len() + b.len() <= LOTE =>
                {
                    a.extend(b.iter().cloned());
                    return;
                }
                _ => {}
            }
        }
        if let Some(i) = self.fila.iter().position(|(v, _)| substitui(v, &e)) {
            self.fila[i] = (e, agora);
            return;
        }
        self.fila.push_back((e, agora));
        while self.fila.len() > MAX_FILA {
            self.fila.pop_front();
        }
    }

    /// O próximo a enviar, se for a hora (não tira da fila: confirme com [`Saida::ok`]).
    pub fn proxima(&self, agora: i64) -> Option<&Entrega> {
        if agora < self.livre_em {
            return None;
        }
        let (e, desde) = self.fila.front()?;
        // Lote pequeno de ofertas/vendas espera juntar mais, se for o único na fila.
        let pequeno = matches!(e, Entrega::Ofertas(_) | Entrega::Vendas(_)) && tamanho(e) < LOTE;
        if pequeno && self.fila.len() == 1 && agora - desde < ESPERA_LOTE_MS {
            return None;
        }
        Some(e)
    }

    pub fn ok(&mut self) {
        self.fila.pop_front();
        self.falhas = 0;
        self.livre_em = i64::MIN / 2;
    }

    /// Falhou: fica na fila e espera mais a cada falha. `descartar`: erro do pedido (4xx), não
    /// adianta tentar de novo.
    pub fn falhou(&mut self, agora: i64, descartar: bool) {
        if descartar {
            self.fila.pop_front();
            return;
        }
        self.falhas = self.falhas.saturating_add(1);
        let espera = ESPERA_MIN_MS
            .saturating_mul(1i64 << self.falhas.min(10).saturating_sub(1))
            .min(ESPERA_MAX_MS);
        self.livre_em = agora + espera;
    }
}

#[cfg(test)]
mod testes {
    use super::*;
    use serde_json::json;

    fn of(n: usize) -> Entrega {
        Entrega::Ofertas((0..n).map(|i| json!(i)).collect())
    }

    #[test]
    fn junta_lote_e_espera_juntar() {
        let mut s = Saida::nova();
        s.guardar(of(20), 0);
        s.guardar(of(30), 1);
        assert_eq!(s.len(), 1);
        assert_eq!(s.proxima(10_000), None, "lote pequeno espera");
        assert_eq!(s.proxima(ESPERA_LOTE_MS).map(tamanho), Some(50));
        s.guardar(of(LOTE), 2);
        assert_eq!(s.len(), 2, "não passa do tamanho do lote");
        assert!(s.proxima(3).is_some(), "com mais na fila, sai já");
    }

    #[test]
    fn falha_espera_cresce_e_ok_libera() {
        let mut s = Saida::nova();
        s.guardar(Entrega::Raros(json!(1)), 0);
        s.falhou(0, false);
        assert!(s.proxima(4_999).is_none());
        assert!(s.proxima(5_000).is_some());
        s.falhou(5_000, false);
        assert!(s.proxima(14_999).is_none());
        assert!(s.proxima(15_000).is_some());
        s.ok();
        assert!(s.is_empty());
    }

    #[test]
    fn substitui_estado_velho_e_descarta_erro_de_pedido() {
        let mut s = Saida::nova();
        s.guardar(Entrega::Equipe(json!({"nick": "a", "v": 1})), 0);
        s.guardar(Entrega::Equipe(json!({"nick": "b"})), 0);
        s.guardar(Entrega::Equipe(json!({"nick": "a", "v": 2})), 0);
        assert_eq!(s.len(), 2);
        assert_eq!(
            s.proxima(0),
            Some(&Entrega::Equipe(json!({"nick": "a", "v": 2})))
        );
        s.falhou(0, true);
        assert_eq!(s.len(), 1);
    }
}
