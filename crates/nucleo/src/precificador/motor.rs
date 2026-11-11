//! Decomposição das vendas (`describe`/`prepare`) e normalização pela curva de mercado.

use std::collections::HashMap;

use super::ajuste::{Ajuste, PontoAjuste};
use super::cotacao::Cotacoes;
use super::especie::Central;
use super::ficha::Ficha;
use super::{AOE_FRAGMENTO, MEGA, MEGA_SHINY, POR_ADICIONAL, Z_FRAGMENTO};
use crate::js;

/// Um adicional que encarece a carta (Z-Move, AoE, Mega), em fragmentos de item.
#[derive(Debug, Clone, PartialEq)]
pub struct Adicional {
    pub item_id: i32,
    pub rotulo: String,
}

/// O que o precificador entende de uma carta (`describe`).
#[derive(Debug, Clone, PartialEq)]
pub struct Descricao {
    pub pve: f64,
    pub familia: i32,
    pub familia_nome: String,
    pub shiny: bool,
    pub nivel: f64,
    pub adicionais: Vec<Adicional>,
}

/// `describe(central, {...ficha, refino: {}, speciesId})`.
pub fn descrever(central: &Central, ficha: &Ficha, especie: i32) -> Option<Descricao> {
    let atributos = ficha.atributos()?;
    let e = central.especie(especie)?;
    let base = e.base_da_mega().filter(|b| central.especie(*b).is_some());
    let mega = base.is_some();
    // Os adicionais não entram no PvE: a carta é avaliada "limpa".
    let final_ = central.melhor_final(base.unwrap_or(e.id), &atributos)?;
    let pve = final_.pve.filter(|p| *p > 0.0)?;

    let mut adicionais = Vec::new();
    if let Some(tipo) = &ficha.tm_elemental {
        adicionais.push(Adicional {
            item_id: Z_FRAGMENTO,
            rotulo: format!("Z-Move {}", tipo.to_uppercase()),
        });
    }
    if ficha.tm_aoe {
        adicionais.push(Adicional {
            item_id: AOE_FRAGMENTO,
            rotulo: "AoE".into(),
        });
    }
    if mega {
        adicionais.push(if ficha.shiny {
            Adicional {
                item_id: MEGA_SHINY,
                rotulo: "Mega Shiny".into(),
            }
        } else {
            Adicional {
                item_id: MEGA,
                rotulo: "Mega".into(),
            }
        });
    }
    Some(Descricao {
        pve,
        familia: final_.id,
        familia_nome: final_.nome,
        shiny: ficha.shiny,
        nivel: ficha.nivel.unwrap_or(f64::NAN),
        adicionais,
    })
}

/// Uma venda decomposta (linha do "motor" no legado).
#[derive(Debug, Clone, PartialEq)]
pub struct LinhaMotor {
    pub em: i64,
    pub preco: f64,
    pub base: f64,
    pub log_base: f64,
    pub adicionais: f64,
    pub mega: bool,
    pub pve: f64,
    pub familia: i32,
    pub familia_nome: String,
    pub shiny: bool,
    pub nivel: f64,
    pub especie: i32,
    pub nome: Option<String>,
    pub nota: Option<f64>,
    pub incerta: bool,
    pub refinada: bool,
}

impl LinhaMotor {
    pub fn ponto_ajuste(&self) -> PontoAjuste {
        PontoAjuste {
            log_pve: self.pve.ln(),
            log_nivel: self.nivel.ln(),
            nota: self.nota,
            log_base: self.log_base,
        }
    }
}

/// Venda de pokémon com ficha, pronta para o `prepare`.
#[derive(Debug, Clone, PartialEq)]
pub struct VendaFicha {
    pub em: i64,
    pub lido_em: i64,
    pub preco: f64,
    pub especie: Option<i32>,
    pub ficha: Ficha,
}

const DIAS_ITEM_LONGE: i64 = 5;
const LEITURA_TARDIA_MS: i64 = 2 * 3_600_000;

/// `prepare`: decompõe as vendas. As que não dá para decompor ficam de fora.
pub fn preparar(central: &Central, vendas: &[VendaFicha], cotacoes: &Cotacoes) -> Vec<LinhaMotor> {
    vendas
        .iter()
        .filter_map(|s| {
            let especie = central.achar(s.especie, s.ficha.nome.as_deref())?;
            let d = descrever(central, &s.ficha, especie)?;
            // Soma das cotações dos adicionais; sem cotação de algum, a venda é descartada.
            let (adicionais, longe) =
                d.adicionais
                    .iter()
                    .try_fold((0.0, false), |(soma, longe), a| {
                        let q = cotacoes.perto(a.item_id, s.em, DIAS_ITEM_LONGE)?;
                        Some((soma + q.preco * POR_ADICIONAL, longe || q.longe))
                    })?;
            let base = s.preco - adicionais;
            if base.is_nan() || base <= 0.0 {
                return None;
            }
            let leitura_ok = s.lido_em >= s.em && s.lido_em - s.em <= LEITURA_TARDIA_MS;
            let tem_tm = s.ficha.tm_elemental.is_some() || s.ficha.tm_aoe;
            Some(LinhaMotor {
                em: s.em,
                preco: s.preco,
                base,
                log_base: base.ln(),
                adicionais,
                mega: d
                    .adicionais
                    .iter()
                    .any(|a| a.item_id == MEGA || a.item_id == MEGA_SHINY),
                pve: d.pve,
                familia: d.familia,
                familia_nome: d.familia_nome,
                shiny: d.shiny,
                nivel: d.nivel,
                especie,
                nome: s.ficha.nome.clone(),
                nota: s.ficha.nota,
                incerta: longe || (tem_tm && !leitura_ok),
                refinada: s.ficha.refinada,
            })
        })
        .collect()
}

/// Referência da carta típica de uma espécie (medianas do grupo).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Referencia {
    pub nivel: f64,
    pub pve: f64,
    pub nota: Option<f64>,
}

/// Uma venda de pokémon normalizada para a carta típica da espécie.
#[derive(Debug, Clone, PartialEq)]
pub struct VendaNormalizada {
    pub em: i64,
    pub unidade: f64,
    pub especie: i32,
    pub nome: Option<String>,
    pub nivel: f64,
    pub shiny: bool,
    pub pve: f64,
    pub referencia: Option<Referencia>,
}

/// `normalizarOrb`: cada venda vira o preço de uma carta típica da espécie (mesmo PvE,
/// nível e nota medianos), pela curva de mercado. Tira o ruído de misturar cartas boas e
/// ruins no mesmo ticker.
pub fn normalizar(linhas: &[LinhaMotor]) -> Vec<VendaNormalizada> {
    let ok: Vec<&LinhaMotor> = linhas
        .iter()
        .filter(|r| r.base > 0.0 && r.pve > 0.0 && r.nivel > 0.0)
        .collect();
    let ajuste_de = |shiny: bool| {
        let l: Vec<PontoAjuste> = ok
            .iter()
            .filter(|r| r.shiny == shiny)
            .map(|r| r.ponto_ajuste())
            .collect();
        (l.len() >= 3).then(|| Ajuste::novo(&l))
    };
    let ajustes = [ajuste_de(false), ajuste_de(true)];

    // Grupos por espécie+shiny, na ordem em que aparecem (como um Map do JS).
    let (ordem, grupos) = ok.iter().fold(
        (
            Vec::<(i32, bool)>::new(),
            HashMap::<(i32, bool), Vec<&LinhaMotor>>::new(),
        ),
        |(mut ordem, mut grupos), r| {
            let chave = (r.especie, r.shiny);
            grupos
                .entry(chave)
                .or_insert_with(|| {
                    ordem.push(chave);
                    Vec::new()
                })
                .push(r);
            (ordem, grupos)
        },
    );

    ordem
        .iter()
        .filter_map(|chave| grupos.get(chave))
        .flat_map(|l| {
            let ajuste = ajustes[usize::from(l[0].shiny)].as_ref();
            let notas: Vec<f64> = l.iter().filter_map(|r| r.nota).collect();
            let referencia = ajuste.map(|_| {
                (
                    js::mediana_meio(l.iter().map(|r| r.pve.ln())).unwrap_or(f64::NAN),
                    js::mediana_meio(l.iter().map(|r| r.nivel.ln())).unwrap_or(f64::NAN),
                    js::mediana_meio(notas.iter().copied()),
                )
            });
            l.iter().map(move |r| {
                let unidade = match (ajuste, referencia) {
                    (Some(a), Some((pve, nivel, nota))) => {
                        let adj = (a.avaliar(pve, nivel, nota) + r.log_base
                            - a.avaliar(r.pve.ln(), r.nivel.ln(), r.nota))
                        .exp();
                        if adj.is_finite() && adj > 0.0 {
                            adj
                        } else {
                            r.base
                        }
                    }
                    _ => r.base,
                };
                VendaNormalizada {
                    em: r.em,
                    unidade,
                    especie: r.especie,
                    nome: r.nome.clone(),
                    nivel: r.nivel,
                    shiny: r.shiny,
                    pve: r.pve,
                    referencia: referencia.map(|(pve, nivel, nota)| Referencia {
                        nivel: js::round(nivel.exp()),
                        pve: js::round(pve.exp()),
                        nota,
                    }),
                }
            })
        })
        .collect()
}
