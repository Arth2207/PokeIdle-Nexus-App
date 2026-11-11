//! Cotação diária de itens (em orbs), usada para descontar os adicionais do preço.

use std::collections::HashMap;

use super::DIA_MS;

/// Uma cotação: dia (número do dia em UTC−3) e preço mediano.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PrecoItemDia {
    pub dia: i64,
    pub item_id: i32,
    pub preco: f64,
}

/// Cotação mais próxima de um instante (`itemQuotes` do legado).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cotacao {
    pub preco: f64,
    /// Mais de 3 dias de distância: a decomposição fica marcada como incerta.
    pub longe: bool,
}

const DIAS_ITEM: i64 = 3;

#[derive(Debug, Clone, Default)]
pub struct Cotacoes {
    /// Por item: (instante ao meio-dia de SP, preço), em ordem de tempo.
    por_item: HashMap<i32, Vec<(i64, f64)>>,
}

impl Cotacoes {
    pub fn novo(precos: impl IntoIterator<Item = PrecoItemDia>) -> Self {
        let mut por_item = precos.into_iter().filter(|p| p.preco > 0.0).fold(
            HashMap::<i32, Vec<(i64, f64)>>::new(),
            |mut m, p| {
                // `Date.parse(dia + 'T12:00:00-03:00')`: meio-dia em SP = 15h UTC.
                m.entry(p.item_id)
                    .or_default()
                    .push((p.dia * DIA_MS + 15 * 3_600_000, p.preco));
                m
            },
        );
        por_item
            .values_mut()
            .for_each(|l| l.sort_by_key(|(t, _)| *t));
        Self { por_item }
    }

    /// A cotação mais próxima de `em`, até `max_dias` de distância.
    pub fn perto(&self, item_id: i32, em: i64, max_dias: i64) -> Option<Cotacao> {
        let lista = self.por_item.get(&item_id)?;
        let i = lista.partition_point(|(t, _)| *t < em);
        let depois = lista.get(i).map(|(t, p)| (t - em, *p));
        let antes = i
            .checked_sub(1)
            .and_then(|j| lista.get(j))
            .map(|(t, p)| (em - t, *p));
        let alcance = max_dias * DIA_MS;
        // Empate de distância: fica o de depois, como no `sort` estável do legado.
        [depois, antes]
            .into_iter()
            .flatten()
            .filter(|(d, _)| *d <= alcance)
            .min_by_key(|(d, _)| *d)
            .map(|(d, preco)| Cotacao {
                preco,
                longe: d > DIAS_ITEM * DIA_MS,
            })
    }

    /// `quote(itemId, at)`: só a até 3 dias.
    pub fn preco(&self, item_id: i32, em: i64) -> Option<f64> {
        self.perto(item_id, em, DIAS_ITEM).map(|c| c.preco)
    }
}
