//! O que as contas aprendem juntas: a nota média de cada espécie em cada casa, somando as
//! lutas de treino de todas as contas. A formação de uma conta não passa para outra (os ids
//! dos pokémon são dela), mas "Scizor rende mais na casa 17" vale para qualquer uma: cada
//! conta testa primeiro as casas que foram melhor para aquela espécie em alguma conta.

use std::collections::HashMap;

use serde_json::{json, Value};

/// Nota de quem nunca lutou numa casa (o meio da escala: vitória vale 1, a margem ±0,5).
const NEUTRA: f64 = 0.5;
/// Peso da nota neutra na média (lutas "de mentira"): poucas lutas não decidem sozinhas.
const PESO_NEUTRO: f64 = 2.0;

#[derive(Default)]
pub struct Comum {
    /// (espécie, casa) → (soma das notas, lutas).
    por: HashMap<(i64, i64), (f64, u32)>,
    mudou: bool,
}

impl Comum {
    /// Uma luta: as (espécie, casa) do time e a nota dela.
    pub fn registrar(&mut self, pares: &[(i64, i64)], nota: f64) {
        for p in pares {
            let e = self.por.entry(*p).or_default();
            e.0 += nota;
            e.1 += 1;
        }
        self.mudou |= !pares.is_empty();
    }

    /// Média suavizada (sem lutas, a neutra).
    pub fn nota(&self, especie: i64, casa: i64) -> f64 {
        let (s, n) = self.por.get(&(especie, casa)).copied().unwrap_or_default();
        (s + NEUTRA * PESO_NEUTRO) / (f64::from(n) + PESO_NEUTRO)
    }

    /// As casas candidatas, as melhores para a espécie primeiro (empate: a ordem que veio).
    pub fn ordenar(&self, especie: Option<i64>, mut casas: Vec<i64>) -> Vec<i64> {
        if let Some(sp) = especie {
            casas.sort_by(|a, b| self.nota(sp, *b).total_cmp(&self.nota(sp, *a)));
        }
        casas
    }

    pub fn mudou(&mut self) -> bool {
        std::mem::take(&mut self.mudou)
    }

    pub fn para_json(&self) -> Value {
        let mut v: Vec<_> = self.por.iter().collect();
        v.sort_by_key(|(k, _)| **k);
        Value::from(
            v.into_iter()
                .map(|((sp, c), (s, n))| json!([sp, c, s, n]))
                .collect::<Vec<_>>(),
        )
    }

    pub fn restaurar(&mut self, v: &Value) {
        for x in v.as_array().into_iter().flatten() {
            let n = |i: usize| x.get(i).and_then(Value::as_f64);
            if let (Some(sp), Some(c), Some(s), Some(l)) = (n(0), n(1), n(2), n(3)) {
                self.por.insert((sp as i64, c as i64), (s, l as u32));
            }
        }
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn ordena_pela_nota_de_todas_as_contas() {
        let mut c = Comum::default();
        // Duas contas: Scizor (212) foi melhor na 18 que na 17.
        c.registrar(&[(212, 17)], 0.0);
        c.registrar(&[(212, 18)], 1.4);
        c.registrar(&[(212, 18)], 1.2);
        assert_eq!(c.ordenar(Some(212), vec![17, 16, 18]), vec![18, 16, 17]);
        assert_eq!(c.ordenar(None, vec![17, 16, 18]), vec![17, 16, 18]);
        let mut d = Comum::default();
        d.restaurar(&c.para_json());
        assert_eq!(d.nota(212, 18), c.nota(212, 18));
    }
}
