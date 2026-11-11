//! Curva de mercado: quanto o log do preço base sobe com PvE, nível e nota.
//!
//! Porte direto de `pav`, `at`, `marketLine` e `marketFit` do legado, com as mesmas
//! operações na mesma ordem (a soma de floats não é associativa; mudar a ordem muda o
//! último dígito e a comparação com o legado deixa de ser exata).

use crate::js;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bloco {
    pub lo: f64,
    pub hi: f64,
    pub y: f64,
}

/// Regressão isotônica (pool adjacent violators): curva não decrescente.
pub fn pav(pontos: &[(f64, f64)]) -> Vec<Bloco> {
    let mut ordenados = pontos.to_vec();
    ordenados.sort_by(|a, b| a.0.total_cmp(&b.0));
    // (soma, n, lo, hi)
    let blocos =
        ordenados
            .into_iter()
            .fold(Vec::<(f64, f64, f64, f64)>::new(), |mut blocos, (x, y)| {
                blocos.push((y, 1.0, x, x));
                while blocos.len() > 1 {
                    let n = blocos.len();
                    let (bs, bn, _, bhi) = blocos[n - 1];
                    let (as_, an, _, _) = blocos[n - 2];
                    if as_ / an <= bs / bn {
                        break;
                    }
                    let a = &mut blocos[n - 2];
                    a.0 = as_ + bs;
                    a.1 = an + bn;
                    a.3 = bhi;
                    blocos.pop();
                }
                blocos
            });
    blocos
        .into_iter()
        .map(|(soma, n, lo, hi)| Bloco {
            lo,
            hi,
            y: soma / n,
        })
        .collect()
}

/// Valor da curva em `x`, interpolando entre os centros dos blocos.
pub fn em(curva: &[Bloco], x: f64) -> Option<f64> {
    let meio = |c: &Bloco| (c.lo + c.hi) / 2.0;
    let (primeiro, ultimo) = (curva.first()?, curva.last()?);
    if x <= meio(primeiro) {
        return Some(primeiro.y);
    }
    if x >= meio(ultimo) {
        return Some(ultimo.y);
    }
    let (mut lo, mut hi) = (1usize, curva.len() - 1);
    while lo < hi {
        let m = (lo + hi) >> 1;
        if meio(&curva[m]) < x {
            lo = m + 1;
        } else {
            hi = m;
        }
    }
    let (a, b) = (&curva[lo - 1], &curva[lo]);
    let t = (x - meio(a)) / (meio(b) - meio(a));
    Some(a.y + t * (b.y - a.y))
}

/// Um termo do modelo aditivo, já centrado (`g(x) - m`).
#[derive(Debug, Clone, PartialEq)]
pub enum Termo {
    Zero,
    Reta { k: f64, mx: f64, desloc: f64 },
    Curva { blocos: Vec<Bloco>, desloc: f64 },
}

impl Termo {
    fn bruto(&self, x: f64) -> f64 {
        match self {
            Termo::Zero => 0.0,
            Termo::Reta { k, mx, .. } => k * (x - mx),
            Termo::Curva { blocos, .. } => em(blocos, x).unwrap_or(f64::NAN),
        }
    }

    fn valor(&self, x: f64) -> f64 {
        match self {
            Termo::Zero => 0.0,
            Termo::Reta { desloc, .. } | Termo::Curva { desloc, .. } => self.bruto(x) - desloc,
        }
    }
}

/// Uma venda vista pelo ajuste.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PontoAjuste {
    pub log_pve: f64,
    pub log_nivel: f64,
    pub nota: Option<f64>,
    pub log_base: f64,
}

/// Modelo ajustado: log do preço base esperado para (log PvE, log nível, nota).
#[derive(Debug, Clone, PartialEq)]
pub enum Ajuste {
    /// Poucas vendas com nota: plano em (log PvE, log nível), sem nota.
    Plano {
        my: f64,
        mx: f64,
        ml: f64,
        b: f64,
        c: f64,
    },
    /// Modelo aditivo: reta em PvE + curvas monótonas em nota e nível.
    Aditivo {
        a0: f64,
        pve: Termo,
        nota: Termo,
        nivel: Termo,
        nota_media: f64,
    },
}

const MIN_MERCADO: usize = 30;

impl Ajuste {
    pub fn avaliar(&self, log_pve: f64, log_nivel: f64, nota: Option<f64>) -> f64 {
        match self {
            Ajuste::Plano { my, mx, ml, b, c } => my + b * (log_pve - mx) + c * (log_nivel - ml),
            Ajuste::Aditivo {
                a0,
                pve,
                nota: termo_nota,
                nivel,
                nota_media,
            } => {
                a0 + pve.valor(log_pve)
                    + termo_nota.valor(nota.filter(|n| n.is_finite()).unwrap_or(*nota_media))
                    + nivel.valor(log_nivel)
            }
        }
    }

    /// `marketFit(same)`.
    pub fn novo(mesmos: &[PontoAjuste]) -> Self {
        let com_nota: Vec<&PontoAjuste> = mesmos.iter().filter(|p| p.nota.is_some()).collect();
        if com_nota.len() < MIN_MERCADO {
            return plano(mesmos);
        }
        let y: Vec<f64> = com_nota.iter().map(|p| p.log_base).collect();
        let xs: [Vec<f64>; 3] = [
            com_nota.iter().map(|p| p.log_pve).collect(),
            com_nota
                .iter()
                .map(|p| p.nota.unwrap_or(f64::NAN))
                .collect(),
            com_nota.iter().map(|p| p.log_nivel).collect(),
        ];
        let a0 = js::media(&y);
        let mut termos = [Termo::Zero, Termo::Zero, Termo::Zero];

        // Backfitting: 6 passadas, ajustando um termo por vez sobre o resíduo dos outros.
        // Ordem fixa: pve (reta), nota (curva), nível (curva).
        (0..6).for_each(|_| {
            (0..3).for_each(|k| {
                let residuo: Vec<f64> = (0..y.len())
                    .map(|i| {
                        let outros = (0..3)
                            .filter(|&j| j != k)
                            .fold(0.0, |s, j| s + termos[j].valor(xs[j][i]));
                        y[i] - a0 - outros
                    })
                    .collect();
                let bruto = if k == 0 {
                    reta(&xs[k], &residuo)
                } else {
                    curva(&xs[k], &residuo)
                };
                let m = js::media(&xs[k].iter().map(|x| bruto.bruto(*x)).collect::<Vec<_>>());
                termos[k] = match bruto {
                    Termo::Reta { k, mx, .. } => Termo::Reta { k, mx, desloc: m },
                    Termo::Curva { blocos, .. } => Termo::Curva { blocos, desloc: m },
                    Termo::Zero => Termo::Zero,
                };
            });
        });
        let nota_media = js::media(&xs[1]);
        let [pve, nota, nivel] = termos;
        Ajuste::Aditivo {
            a0,
            pve,
            nota,
            nivel,
            nota_media,
        }
    }
}

/// Reta pela origem dos resíduos, com inclinação não negativa (`line` do legado).
fn reta(xs: &[f64], ys: &[f64]) -> Termo {
    let (mx, my) = (js::media(xs), js::media(ys));
    let (sxy, sxx) = xs.iter().zip(ys).fold((0.0, 0.0), |(sxy, sxx), (x, y)| {
        (sxy + (x - mx) * (y - my), sxx + (x - mx).powi(2))
    });
    let k = if sxx > 1e-9 {
        (sxy / sxx).max(0.0)
    } else {
        0.0
    };
    Termo::Reta { k, mx, desloc: 0.0 }
}

fn curva(xs: &[f64], ys: &[f64]) -> Termo {
    let pontos: Vec<(f64, f64)> = xs.iter().copied().zip(ys.iter().copied()).collect();
    Termo::Curva {
        blocos: pav(&pontos),
        desloc: 0.0,
    }
}

/// `marketLine`: mínimos quadrados em duas variáveis, com coeficientes não negativos.
fn plano(pontos: &[PontoAjuste]) -> Ajuste {
    let n = pontos.len() as f64;
    let media = |f: fn(&PontoAjuste) -> f64| pontos.iter().fold(0.0, |s, p| s + f(p)) / n;
    let (mx, ml, my) = (
        media(|p| p.log_pve),
        media(|p| p.log_nivel),
        media(|p| p.log_base),
    );
    let (xx, ll, xl, xy, ly) = pontos.iter().fold((0.0, 0.0, 0.0, 0.0, 0.0), |acc, p| {
        let (dx, dl, dy) = (p.log_pve - mx, p.log_nivel - ml, p.log_base - my);
        (
            acc.0 + dx * dx,
            acc.1 + dl * dl,
            acc.2 + dx * dl,
            acc.3 + dx * dy,
            acc.4 + dl * dy,
        )
    });
    let det = xx * ll - xl * xl;
    let mut b = if det > 1e-9 {
        (xy * ll - ly * xl) / det
    } else {
        0.0
    };
    let mut c = if det > 1e-9 {
        (ly * xx - xy * xl) / det
    } else {
        0.0
    };
    if b < 0.0 {
        b = 0.0;
        c = if ll > 1e-9 { (ly / ll).max(0.0) } else { 0.0 };
    }
    if c < 0.0 {
        c = 0.0;
        b = if xx > 1e-9 { (xy / xx).max(0.0) } else { 0.0 };
    }
    Ajuste::Plano { my, mx, ml, b, c }
}

#[cfg(test)]
mod testes {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn pav_junta_violacoes() {
        let c = pav(&[(1.0, 1.0), (2.0, 3.0), (3.0, 2.0), (4.0, 4.0)]);
        let ys: Vec<f64> = c.iter().map(|b| b.y).collect();
        assert_eq!(ys, vec![1.0, 2.5, 4.0]);
        assert_eq!(em(&c, 0.0), Some(1.0));
        assert_eq!(em(&c, 2.5), Some(2.5));
        assert_eq!(em(&c, 9.0), Some(4.0));
    }

    proptest! {
        #[test]
        fn pav_e_monotona(pontos in proptest::collection::vec((-10f64..10.0, -10f64..10.0), 1..60)) {
            let c = pav(&pontos);
            prop_assert!(c.windows(2).all(|w| w[0].y <= w[1].y + 1e-12));
        }

        #[test]
        fn ajuste_e_sempre_finito(
            pontos in proptest::collection::vec((0.1f64..12.0, 0.0f64..9.0, proptest::option::of(0.0f64..10.0), -2f64..14.0), 3..80),
            alvo in (0.1f64..12.0, 0.0f64..9.0),
        ) {
            let p: Vec<PontoAjuste> = pontos.into_iter()
                .map(|(log_pve, log_nivel, nota, log_base)| PontoAjuste { log_pve, log_nivel, nota, log_base })
                .collect();
            let a = Ajuste::novo(&p);
            prop_assert!(a.avaliar(alvo.0, alvo.1, None).is_finite());
        }
    }
}
