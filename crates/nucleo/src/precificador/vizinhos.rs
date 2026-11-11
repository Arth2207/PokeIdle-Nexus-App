//! Valor justo de uma carta pelas vendas mais parecidas (`neighbors` + `priceSheet`).

use std::collections::HashMap;

use super::ajuste::Ajuste;
use super::cotacao::Cotacoes;
use super::especie::Central;
use super::ficha::Ficha;
use super::motor::{descrever, LinhaMotor};
use super::{DIA_MS, MEGA, MEGA_SHINY, POR_ADICIONAL};
use crate::js;

/// Configuração dos vizinhos (`NEIGHBORS` no legado).
struct Cfg;
impl Cfg {
    const CAMADAS: [f64; 4] = [1.0, 0.2, 0.05, 0.01];
    const SIGMA_NIVEL: f64 = 1.5;
    const SIGMA_PVE: f64 = 0.15;
    const SIGMA_NOTA: f64 = 0.3;
    const MEIA_VIDA_DIAS: f64 = 14.0;
    const PESO_MINIMO: f64 = 1e-5;
    const PESO_REFINADA: f64 = 1.0;
    const INCERTA_SIMPLES: f64 = 1.0;
}

/// Pesos padrão da regressão logística da confiança.
pub const PESOS_CONFIANCA: [f64; 5] = [-2.64, 0.66, 0.68, 1.53, 0.85];
/// Viés padrão (log de 1,09): o mercado vende um pouco abaixo do anunciado.
pub fn vies_padrao() -> f64 {
    js::ln(1.09)
}

const CHAVES: [&str; 4] = ["proximidade", "certeza", "consistencia", "ampliacao"];

#[derive(Debug, Clone, PartialEq)]
pub struct Confianca {
    pub chance: f64,
    pub percent: f64,
    pub lowest: &'static str,
}

/// `confidence(parts, weights)`.
pub fn confianca(partes: [f64; 4], pesos: &[f64; 5]) -> Confianca {
    let z = partes
        .iter()
        .enumerate()
        .fold(pesos[0], |s, (i, v)| s + pesos[i + 1] * v);
    let chance = 1.0 / (1.0 + js::exp(-z));
    // Maior perda primeiro; no empate, a primeira chave (ordenação estável).
    let lowest = (0..4)
        .map(|i| (CHAVES[i], pesos[i + 1] * (1.0 - partes[i])))
        .fold(
            None::<(&'static str, f64)>,
            |melhor, (k, perda)| match melhor {
                Some((_, p)) if p >= perda => melhor,
                _ => Some((k, perda)),
            },
        )
        .map_or("proximidade", |(k, _)| k);
    Confianca {
        chance,
        percent: (js::round(chance * 10.0) * 10.0).clamp(10.0, 90.0),
        lowest,
    }
}

/// Índice das vendas de um lado (shiny ou não), montado uma vez por mercado.
#[derive(Debug)]
pub struct Indice {
    mesmos: Vec<LinhaMotor>,
    por_familia: HashMap<i32, usize>,
    /// (log PvE, posição em `mesmos`), ordenado por log PvE.
    ordenados: Vec<(f64, usize)>,
    max_em: f64,
    pub ajuste: Ajuste,
}

impl Indice {
    /// `None` com menos de 3 vendas.
    pub fn novo(linhas: &[LinhaMotor], shiny: bool) -> Option<Self> {
        let mesmos: Vec<LinhaMotor> = linhas
            .iter()
            .filter(|r| r.shiny == shiny)
            .cloned()
            .collect();
        if mesmos.len() < 3 {
            return None;
        }
        let por_familia = mesmos.iter().fold(HashMap::new(), |mut m, r| {
            *m.entry(r.familia).or_insert(0) += 1;
            m
        });
        let mut ordenados: Vec<(f64, usize)> = mesmos
            .iter()
            .enumerate()
            .map(|(i, r)| (js::ln(r.pve), i))
            .collect();
        ordenados.sort_by(|a, b| a.0.total_cmp(&b.0));
        let max_em = mesmos
            .iter()
            .fold(f64::NEG_INFINITY, |m, r| m.max(r.em as f64));
        let pontos: Vec<_> = mesmos.iter().map(|r| r.ponto_ajuste()).collect();
        Some(Self {
            ajuste: Ajuste::novo(&pontos),
            mesmos,
            por_familia,
            ordenados,
            max_em,
        })
    }

    pub fn total(&self) -> usize {
        self.mesmos.len()
    }
}

/// A carta a avaliar.
#[derive(Debug, Clone, PartialEq)]
pub struct Alvo {
    pub pve: f64,
    pub nivel: f64,
    pub nota: Option<f64>,
    pub familia: i32,
    pub familia_nome: String,
    pub em: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Estimativa {
    pub low: f64,
    pub median: f64,
    pub high: f64,
    pub familia_total: usize,
    pub mercado_total: usize,
    pub confianca: Confianca,
}

struct Ponto {
    v: f64,
    w: f64,
    camada: usize,
    incerta: bool,
}

/// Quantil ponderado por peso, com interpolação (`weightedQuantile`).
fn quantil_ponderado(pontos: &[Ponto], q: f64) -> f64 {
    let mut s: Vec<(f64, f64)> = pontos.iter().map(|p| (p.v, p.w)).collect();
    s.sort_by(|a, b| a.0.total_cmp(&b.0));
    let total = s.iter().fold(0.0, |t, p| t + p.1);
    let alvo = q * total;
    let mut acc = 0.0;
    let mut anterior: Option<f64> = None;
    for &(v, w) in &s {
        let acc2 = acc + w;
        if acc2 >= alvo {
            return match anterior {
                None => v,
                Some(pv) => pv + ((alvo - acc) / w).clamp(0.0, 1.0) * (v - pv),
            };
        }
        acc = acc2;
        anterior = Some(v);
    }
    s.last().map_or(f64::NAN, |p| p.0)
}

/// `neighbors(target, rows, {...})`.
pub fn vizinhos(
    alvo: &Alvo,
    indice: &Indice,
    adicionais: f64,
    tipos_de: impl Fn(i32) -> Vec<String>,
    unidade_mega: impl Fn(bool, i64) -> f64,
    pesos: &[f64; 5],
    vies: f64,
) -> Option<Estimativa> {
    let ajuste = &indice.ajuste;
    let meus = tipos_de(alvo.familia);
    let camada_de = |r: &LinhaMotor| -> usize {
        if r.familia == alvo.familia {
            return 0;
        }
        let t = tipos_de(r.familia);
        if !meus.is_empty() && t.len() == meus.len() && meus.iter().all(|x| t.contains(x)) {
            1
        } else if meus.iter().any(|x| t.contains(x)) {
            2
        } else {
            3
        }
    };
    let em = alvo.em as f64;
    let meia_vida = Cfg::MEIA_VIDA_DIAS * DIA_MS as f64;
    let peso_max = Cfg::CAMADAS
        .iter()
        .fold(f64::NEG_INFINITY, |m, c| m.max(*c))
        * Cfg::PESO_REFINADA.max(1.0)
        * js::pow(0.5, (em - indice.max_em) / meia_vida).max(1.0);
    let raio = Cfg::SIGMA_PVE * (2.0 * js::ln(peso_max / Cfg::PESO_MINIMO)).max(0.0).sqrt();
    let lp = js::ln(alvo.pve);
    let limite = |x: f64| indice.ordenados.partition_point(|(v, _)| *v < x);
    let (ini, fim) = (limite(lp - raio - 1e-12), limite(lp + raio + 1e-12));
    let mut posicoes: Vec<usize> = indice.ordenados[ini..fim.max(ini)]
        .iter()
        .map(|(_, i)| *i)
        .collect();
    posicoes.sort_unstable();

    let peso_nota = |r: &LinhaMotor| match (r.nota, alvo.nota) {
        (Some(a), Some(b)) => js::exp(-(((a - b) / Cfg::SIGMA_NOTA).powi(2)) / 2.0),
        _ => 1.0,
    };
    let mut pontos: Vec<Ponto> = posicoes
        .into_iter()
        .map(|i| &indice.mesmos[i])
        .flat_map(|r| {
            let camada = camada_de(r);
            let w = (if r.refinada { Cfg::PESO_REFINADA } else { 1.0 })
                * Cfg::CAMADAS[camada]
                * js::exp(-((js::ln(r.nivel / alvo.nivel) / Cfg::SIGMA_NIVEL).powi(2)) / 2.0)
                * js::exp(-((js::ln(r.pve / alvo.pve) / Cfg::SIGMA_PVE).powi(2)) / 2.0)
                * peso_nota(r)
                * js::pow(0.5, (em - r.em as f64) / meia_vida);
            let mut saida = Vec::new();
            if w.is_nan() || w < Cfg::PESO_MINIMO {
                return saida;
            }
            let esperado = ajuste.avaliar(js::ln(r.pve), js::ln(r.nivel), r.nota);
            if !r.incerta {
                saida.push(Ponto {
                    v: r.log_base - esperado,
                    w,
                    camada,
                    incerta: false,
                });
                return saida;
            }
            // Venda incerta: parte do peso vai para o preço "sem adicionais" da Mega.
            let simples = (r.preco
                - if r.mega {
                    POR_ADICIONAL * unidade_mega(r.shiny, r.em)
                } else {
                    0.0
                })
            .max(1.0);
            let up = Cfg::INCERTA_SIMPLES;
            saida.push(Ponto {
                v: r.log_base - esperado,
                w: w * (1.0 - up),
                camada,
                incerta: true,
            });
            saida.push(Ponto {
                v: js::ln(simples) - esperado,
                w: w * up,
                camada,
                incerta: true,
            });
            saida
        })
        .collect();
    if pontos.is_empty() {
        return None;
    }

    // Recorte por MAD: resíduos além de mediana ± 4·MAD vão para a borda.
    let vs = js::ordenados(pontos.iter().map(|p| p.v));
    let med = vs[vs.len() >> 1];
    let mad = js::ordenados(vs.iter().map(|v| (v - med).abs()))[vs.len() >> 1];
    if mad > 0.0 {
        let (lo, hi) = (med - 4.0 * mad, med + 4.0 * mad);
        pontos.iter_mut().for_each(|p| p.v = p.v.max(lo).min(hi));
    }

    let meio = quantil_ponderado(&pontos, 0.5);
    let lo = quantil_ponderado(&pontos, 0.25) - meio;
    let hi = quantil_ponderado(&pontos, 0.75) - meio;
    let centro = ajuste.avaliar(js::ln(alvo.pve), js::ln(alvo.nivel), alvo.nota) + meio - vies;
    let preco = |x: f64| js::round(js::exp(x) + adicionais).max(1.0);
    let soma =
        |f: &dyn Fn(&Ponto) -> bool| pontos.iter().filter(|p| f(p)).fold(0.0, |s, p| s + p.w);
    let total = soma(&|_| true);
    let (cw, cw2) = pontos
        .iter()
        .filter(|p| p.camada <= 1)
        .fold((0.0, 0.0), |(a, b), p| (a + p.w, b + p.w * p.w));
    let neff = if cw != 0.0 { cw.powi(2) / cw2 } else { 0.0 };
    let partes = [
        neff / (neff + 5.0),
        soma(&|p| !p.incerta) / total,
        js::exp(-(hi - lo)),
        soma(&|p| p.camada == 0) / total,
    ];
    Some(Estimativa {
        low: preco(centro + lo),
        median: preco(centro),
        high: preco(centro + hi),
        familia_total: indice.por_familia.get(&alvo.familia).copied().unwrap_or(0),
        mercado_total: indice.total(),
        confianca: confianca(partes, pesos),
    })
}

/// Resultado de `priceSheet(..., { details: false })` para uma oferta.
#[derive(Debug, Clone, PartialEq)]
pub struct ValorFicha {
    pub estimativa: Estimativa,
    pub pve: f64,
    pub nivel: f64,
    pub familia_nome: String,
    pub faltam_adicionais: bool,
}

/// Contexto fixo do mercado para avaliar fichas.
pub struct Avaliador<'m> {
    pub central: &'m Central,
    pub cotacoes: &'m Cotacoes,
    /// Preço atual dos itens (menor anúncio em orbs), tem prioridade sobre a cotação.
    pub itens_atuais: &'m HashMap<i32, f64>,
    pub indices: &'m [Option<Indice>; 2],
    pub pesos: [f64; 5],
    pub vies: f64,
}

impl Avaliador<'_> {
    /// Valor justo de uma ficha em orbs. `None` se a espécie, a ficha ou o mercado não
    /// permitirem estimar.
    pub fn avaliar(&self, ficha: &Ficha, agora: i64) -> Option<ValorFicha> {
        // No legado a espécie de uma oferta é achada só pelo nome.
        let especie = self.central.achar(None, ficha.nome.as_deref())?;
        ficha.ivs?;
        let limpa = Ficha {
            shiny_frouxo: ficha.shiny,
            ..ficha.clone()
        };
        let d = descrever(self.central, &limpa, especie)?;
        let totais: Vec<Option<f64>> = d
            .adicionais
            .iter()
            .map(|a| {
                let atual = self
                    .itens_atuais
                    .get(&a.item_id)
                    .copied()
                    .filter(|p| *p > 0.0);
                atual
                    .or_else(|| self.cotacoes.preco(a.item_id, agora))
                    .map(|u| js::round(u * POR_ADICIONAL))
            })
            .collect();
        let conhecidos = totais.iter().flatten().fold(0.0, |s, t| s + t);
        let indice = self.indices[usize::from(d.shiny)].as_ref()?;
        let alvo = Alvo {
            pve: d.pve,
            nivel: d.nivel,
            nota: ficha.nota,
            familia: d.familia,
            familia_nome: d.familia_nome.clone(),
            em: agora,
        };
        let cotacoes = self.cotacoes;
        let estimativa = vizinhos(
            &alvo,
            indice,
            conhecidos,
            |id| self.central.tipos(id).to_vec(),
            |shiny, em| {
                cotacoes
                    .preco(if shiny { MEGA_SHINY } else { MEGA }, em)
                    .unwrap_or(0.0)
            },
            &self.pesos,
            self.vies,
        )?;
        Some(ValorFicha {
            estimativa,
            pve: d.pve,
            nivel: d.nivel,
            familia_nome: d.familia_nome,
            faltam_adicionais: totais.iter().any(Option::is_none),
        })
    }
}
