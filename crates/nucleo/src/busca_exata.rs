//! Busca exata da equipe de maior dano (branch-and-bound), com orçamento opcional.
//!
//! O dano da equipe é `Σ dano(carta, mult)`, com `mult = 3^máx(0, 5 − força)`. Como `mult ≥ 1`
//! e o dano de uma carta só cai quando `mult` sobe, `dano(carta, 1)` é um limite superior
//! para cada carta. Com as cartas em ordem decrescente desse limite, uma equipe parcial `P`
//! com `r` vagas livres nunca passa de:
//!
//! `Σ_{x∈P} dano(x, m_min) + soma dos r maiores limites ainda disponíveis`,
//!
//! onde `m_min` é o menor multiplicador possível (todas as vagas livres com força 1). Se
//! esse teto não supera a melhor equipe já achada, o ramo inteiro é podado.
//!
//! A busca para num limite de nós. Nesse caso a resposta traz a **distância até o ótimo**:
//! quanto o maior teto dos ramos não explorados passa da melhor equipe achada. A equipe da
//! heurística entra como ponto de partida, então a busca exata nunca perde para ela.

use crate::js;
use crate::otimizador::Candidato;

const TEMPO_MAXIMO: f64 = 300.0;
const EPS: f64 = 1e-9;

/// Resultado da busca exata.
#[derive(Debug, Clone, PartialEq)]
pub struct Exata {
    /// Índices em `candidatos`.
    pub pick: Vec<usize>,
    pub d: f64,
    pub total: f64,
    /// A busca terminou: `pick` é o ótimo.
    pub provado: bool,
    /// Teto do ótimo (igual a `d` quando provado).
    pub teto: f64,
    pub nos: u64,
}

impl Exata {
    /// `(teto − d) / teto`: 0 quando provado.
    pub fn distancia(&self) -> f64 {
        if self.teto > 0.0 {
            ((self.teto - self.d) / self.teto).max(0.0)
        } else {
            0.0
        }
    }
}

fn dano(x: &Candidato, mult: f64) -> f64 {
    x.dps_out
        * if x.dps_in > 0.0 {
            TEMPO_MAXIMO.min(x.hp / (x.dps_in * mult))
        } else {
            TEMPO_MAXIMO
        }
}

/// Dano da equipe com a penalidade de força.
pub fn dano_equipe(c: &[Candidato], pick: &[usize], n: usize, nivel_boss: f64) -> f64 {
    let forca = pick
        .iter()
        .fold(0.0, |s, &i| s + (c[i].lvl / nivel_boss).min(1.0));
    let mult = js::pow(3.0, (n as f64 - forca).max(0.0));
    pick.iter().map(|&i| dano(&c[i], mult)).sum()
}

struct Estado<'a> {
    c: &'a [Candidato],
    ordem: Vec<usize>,
    ub: Vec<f64>,
    n: usize,
    l: f64,
    orcamento: Option<f64>,
    limite_nos: u64,
    nos: u64,
    melhor: Option<(Vec<usize>, f64)>,
    teto_restante: f64,
}

impl Estado<'_> {
    fn valido_adicionar(&self, pick: &[usize], x: usize, gasto: f64) -> bool {
        let cx = &self.c[x];
        if pick.iter().any(|&y| self.c[y].id == cx.id) {
            return false;
        }
        if pick.iter().filter(|&&y| self.c[y].sp == cx.sp).count() >= 2 {
            return false;
        }
        self.orcamento.is_none_or(|b| gasto + cx.unit <= b + EPS)
    }

    /// Soma dos `r` maiores limites a partir da posição `k` da ordem (relaxação: ignora as
    /// restrições de espécie e orçamento).
    fn resto(&self, k: usize, r: usize) -> f64 {
        self.ub.iter().skip(k).take(r).sum()
    }

    fn dfs(&mut self, k0: usize, pick: &mut Vec<usize>, gasto: f64, forca: f64) {
        let r = self.n - pick.len();
        if r == 0 {
            let d = dano_equipe(self.c, pick, self.n, self.l);
            if self.melhor.as_ref().is_none_or(|(_, m)| d > *m + EPS) {
                self.melhor = Some((pick.clone(), d));
            }
            return;
        }
        let m_min = js::pow(3.0, (self.n as f64 - forca - r as f64).max(0.0));
        let parcial: f64 = pick.iter().map(|&i| dano(&self.c[i], m_min)).sum();
        let melhor = self.melhor.as_ref().map_or(0.0, |(_, m)| *m);
        for k in k0..self.ordem.len() {
            let teto = parcial + self.resto(k, r);
            if teto <= melhor + EPS {
                break;
            }
            if self.nos >= self.limite_nos {
                self.teto_restante = self.teto_restante.max(teto);
                break;
            }
            let x = self.ordem[k];
            if !self.valido_adicionar(pick, x, gasto) {
                continue;
            }
            self.nos += 1;
            pick.push(x);
            let fx = (self.c[x].lvl / self.l).min(1.0);
            self.dfs(k + 1, pick, gasto + self.c[x].unit, forca + fx);
            pick.pop();
            // A melhor equipe pode ter mudado: o laço volta a comparar com ela.
            let melhor_agora = self.melhor.as_ref().map_or(0.0, |(_, m)| *m);
            if parcial + self.resto(k + 1, r) <= melhor_agora + EPS {
                break;
            }
        }
    }
}

/// Melhor equipe de `n` cartas por dano (com no máximo 2 por espécie e ids distintos), sob o
/// orçamento, se houver. `partida`: uma equipe conhecida (a da heurística) para podar desde o
/// início. `limite_nos`: quantos nós a busca pode abrir antes de parar com uma estimativa.
pub fn melhor_equipe(
    c: &[Candidato],
    n: usize,
    nivel_boss: f64,
    orcamento: Option<f64>,
    partida: Option<&[usize]>,
    limite_nos: u64,
) -> Option<Exata> {
    let mut ordem: Vec<usize> = (0..c.len())
        .filter(|&i| c[i].unit.is_finite() && c[i].unit > 0.0 && dano(&c[i], 1.0) > 0.0)
        .filter(|&i| orcamento.is_none_or(|b| c[i].unit <= b + EPS))
        .collect();
    ordem.sort_by(|&a, &b| {
        dano(&c[b], 1.0)
            .total_cmp(&dano(&c[a], 1.0))
            .then(c[a].unit.total_cmp(&c[b].unit))
            .then(c[a].id.cmp(&c[b].id))
    });
    let ub: Vec<f64> = ordem.iter().map(|&i| dano(&c[i], 1.0)).collect();
    let melhor = partida.filter(|p| {
        p.len() == n
            && orcamento.is_none_or(|b| p.iter().map(|&i| c[i].unit).sum::<f64>() <= b + EPS)
    });
    let mut e = Estado {
        c,
        ordem,
        ub,
        n,
        l: nivel_boss,
        orcamento,
        limite_nos,
        nos: 0,
        melhor: melhor.map(|p| (p.to_vec(), dano_equipe(c, p, n, nivel_boss))),
        teto_restante: 0.0,
    };
    let mut pick = Vec::with_capacity(n);
    e.dfs(0, &mut pick, 0.0, 0.0);
    let (pick, d) = e.melhor?;
    let provado = e.teto_restante <= d + EPS;
    Some(Exata {
        total: pick.iter().map(|&i| c[i].unit).sum(),
        pick,
        d,
        provado,
        teto: if provado { d } else { e.teto_restante },
        nos: e.nos,
    })
}

/// Um ponto da curva custo × poder.
#[derive(Debug, Clone, PartialEq)]
pub struct PontoCurva {
    pub orcamento: f64,
    pub total: f64,
    pub d: f64,
    pub pick: Vec<usize>,
}

/// Fronteira de Pareto de equipes completas: a melhor equipe para orçamentos crescentes,
/// sem pontos dominados (mais cara e não mais forte).
pub fn curva_custo_poder(
    c: &[Candidato],
    n: usize,
    nivel_boss: f64,
    pontos: usize,
    limite_nos: u64,
) -> Vec<PontoCurva> {
    let mut precos: Vec<f64> = c
        .iter()
        .map(|x| x.unit)
        .filter(|u| u.is_finite() && *u > 0.0)
        .collect();
    precos.sort_by(f64::total_cmp);
    if precos.len() < n {
        return Vec::new();
    }
    let minimo: f64 = precos.iter().take(n).sum();
    let maximo: f64 = precos.iter().rev().take(n).sum();
    // Orçamentos em escala geométrica: a curva costuma mudar mais nos valores baixos.
    let k = pontos.max(2);
    let orcamentos: Vec<f64> = (0..k)
        .map(|i| minimo * (maximo / minimo).powf(i as f64 / (k - 1) as f64))
        .collect();
    let busca =
        |&b: &f64| melhor_equipe(c, n, nivel_boss, Some(b), None, limite_nos).map(|r| (b, r));
    // Cada orçamento é independente: com a feature `paralelo`, rodam em paralelo. O `collect`
    // mantém a ordem, então a curva sai igual nos dois casos.
    #[cfg(feature = "paralelo")]
    let resultados: Vec<_> = {
        use rayon::prelude::*;
        orcamentos.par_iter().filter_map(busca).collect()
    };
    #[cfg(not(feature = "paralelo"))]
    let resultados: Vec<_> = orcamentos.iter().filter_map(busca).collect();
    let mut out: Vec<PontoCurva> = Vec::new();
    for (b, r) in resultados {
        if out.last().is_none_or(|p| r.d > p.d + EPS) {
            out.push(PontoCurva {
                orcamento: b,
                total: r.total,
                d: r.d,
                pick: r.pick,
            });
        }
    }
    out
}

#[cfg(test)]
mod testes {
    use super::*;
    use proptest::prelude::*;

    fn cand(id: usize, sp: i64, lvl: f64, unit: f64, dps_out: f64, dps_in: f64) -> Candidato {
        Candidato {
            id: id.to_string(),
            sp,
            lvl,
            unit,
            justo: f64::NAN,
            hp: 1000.0,
            dps_out,
            dps_in,
        }
    }

    /// Força bruta: todas as combinações de 5.
    fn bruta(c: &[Candidato], n: usize, l: f64, orc: Option<f64>) -> Option<f64> {
        let idx: Vec<usize> = (0..c.len()).collect();
        let mut melhor: Option<f64> = None;
        let mut pick = Vec::new();
        #[allow(clippy::too_many_arguments)]
        fn rec(
            c: &[Candidato],
            idx: &[usize],
            k: usize,
            n: usize,
            l: f64,
            orc: Option<f64>,
            pick: &mut Vec<usize>,
            melhor: &mut Option<f64>,
        ) {
            if pick.len() == n {
                let ok_sp = pick
                    .iter()
                    .all(|&i| pick.iter().filter(|&&j| c[j].sp == c[i].sp).count() <= 2);
                let total: f64 = pick.iter().map(|&i| c[i].unit).sum();
                if ok_sp && orc.is_none_or(|b| total <= b + EPS) {
                    let d = dano_equipe(c, pick, n, l);
                    if melhor.is_none_or(|m| d > m) {
                        *melhor = Some(d);
                    }
                }
                return;
            }
            for i in k..idx.len() {
                pick.push(idx[i]);
                rec(c, idx, i + 1, n, l, orc, pick, melhor);
                pick.pop();
            }
        }
        rec(c, &idx, 0, n, l, orc, &mut pick, &mut melhor);
        melhor
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]
        /// A busca exata acha o mesmo ótimo da força bruta, com e sem orçamento.
        #[test]
        fn igual_a_forca_bruta(
            cartas in prop::collection::vec((0i64..4, 50.0f64..400.0, 1.0f64..100.0, 1.0f64..50.0, 0.0f64..30.0), 5..11),
            orc in prop::option::of(20.0f64..400.0),
        ) {
            let c: Vec<Candidato> = cartas.iter().enumerate().map(|(i, &(sp, lvl, unit, o, d))| cand(i, sp, lvl, unit, o, d)).collect();
            let exata = melhor_equipe(&c, 5, 300.0, orc, None, u64::MAX);
            let esperado = bruta(&c, 5, 300.0, orc);
            match (exata, esperado) {
                (Some(e), Some(b)) => {
                    prop_assert!(e.provado);
                    prop_assert!((e.d - b).abs() <= 1e-6 * b.max(1.0), "exata {} ≠ bruta {}", e.d, b);
                    prop_assert!(orc.is_none_or(|o| e.total <= o + 1e-9));
                }
                (None, None) => {}
                (e, b) => prop_assert!(false, "exata {e:?} × bruta {b:?}"),
            }
        }
    }

    #[test]
    fn nunca_perde_para_a_partida_e_informa_a_distancia() {
        let c: Vec<Candidato> = (0..40)
            .map(|i| {
                cand(
                    i,
                    (i % 20) as i64,
                    200.0 + i as f64 * 5.0,
                    10.0 + i as f64,
                    10.0 + (i * 7 % 13) as f64,
                    5.0,
                )
            })
            .collect();
        let partida = [0, 1, 2, 3, 4];
        let d_partida = dano_equipe(&c, &partida, 5, 300.0);
        // Com pouquíssimos nós a busca não termina: devolve algo ≥ partida e um teto.
        let r = melhor_equipe(&c, 5, 300.0, None, Some(&partida), 3).unwrap();
        assert!(r.d >= d_partida - 1e-9);
        if !r.provado {
            assert!(r.teto >= r.d && r.distancia() > 0.0);
        }
        let completo = melhor_equipe(&c, 5, 300.0, None, Some(&partida), u64::MAX).unwrap();
        assert!(completo.provado && completo.distancia() == 0.0);
        assert!(completo.d >= r.d - 1e-9);
    }

    #[test]
    fn curva_sobe_com_o_orcamento() {
        let c: Vec<Candidato> = (0..15)
            .map(|i| {
                cand(
                    i,
                    i as i64,
                    300.0,
                    5.0 + i as f64 * 3.0,
                    5.0 + i as f64,
                    2.0,
                )
            })
            .collect();
        let curva = curva_custo_poder(&c, 5, 300.0, 8, u64::MAX);
        assert!(!curva.is_empty());
        for par in curva.windows(2) {
            assert!(par[1].d > par[0].d && par[1].total >= par[0].total - 1e-9);
        }
    }
}
