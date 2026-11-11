//! Carga das fontes do mercado, incremental.
//!
//! O legado relê 30 dias de vendas a cada 3 minutos. Aqui as vendas ficam em memória e cada
//! ciclo busca só as das últimas 12 horas (a captura atrasa no máximo ~9 h), usando o índice
//! de `em`. Uma recarga completa a cada 30 minutos pega qualquer coisa que tenha escapado.
//! Catálogos e base antiga mudam pouco e também são relidos a cada 30 minutos.

use std::collections::HashMap;
use std::sync::Arc;

use super::catalogo::Catalogo;
use super::mercado::{Fontes, OFERTA_FRESCA_MS, OFERTA_FRESCA_POKEMON_MS, RESUMO_FRESCO_MS};
use super::repo::{AgregadoItem, AgregadoPokemon, ErroRepo, Estaticos, RepoMercado};
use super::venda::VendaBruta;
use super::{DIA_MS, JANELA_DIAS};
use crate::precificador::especie::Central;

const MARGEM_INCREMENTAL_MS: i64 = 12 * 3_600_000;
const RECARGA_COMPLETA_MS: i64 = 30 * 60_000;
const RECARGA_ESTATICOS_MS: i64 = 30 * 60_000;
/// O histórico agregado (todas as vendas, sem filtro de data) muda pouco e custa uma
/// varredura da tabela: é recalculado a cada 15 minutos.
const RECARGA_HISTORICO_MS: i64 = 15 * 60_000;

#[derive(Debug, Clone)]
struct EstaticosProntos {
    lidos_em: i64,
    fontes: Fontes,
}

/// Estado da carga entre um ciclo e outro.
#[derive(Debug, Default)]
pub struct Carregador {
    vendas: HashMap<i64, VendaBruta>,
    max_em: Option<i64>,
    ultima_completa: Option<i64>,
    estaticos: Option<EstaticosProntos>,
    historico: Option<(i64, Vec<AgregadoItem>, Vec<AgregadoPokemon>)>,
    /// Quantas vendas vieram do banco no último ciclo (métrica).
    pub ultima_leitura: usize,
}

impl Carregador {
    /// Lê o que mudou e devolve as fontes completas para montar o mercado em `agora`.
    pub async fn fontes<R: RepoMercado>(
        &mut self,
        repo: &R,
        agora: i64,
    ) -> Result<Fontes, ErroRepo> {
        self.fontes_com(repo, agora, false).await
    }

    /// Como [`Self::fontes`]; com `historico_fresco`, recalcula o histórico agora (o
    /// comparador de paridade precisa dele no mesmo instante que o legado).
    pub async fn fontes_com<R: RepoMercado>(
        &mut self,
        repo: &R,
        agora: i64,
        historico_fresco: bool,
    ) -> Result<Fontes, ErroRepo> {
        let desde = agora - JANELA_DIAS * DIA_MS;
        let completa = self
            .ultima_completa
            .is_none_or(|t| agora - t >= RECARGA_COMPLETA_MS);
        let lidas = match (completa, self.max_em) {
            (false, Some(max)) => repo.vendas_recentes(max - MARGEM_INCREMENTAL_MS).await?,
            _ => {
                let todas = repo.vendas(desde, agora).await?;
                self.vendas.clear();
                self.ultima_completa = Some(agora);
                todas
            }
        };
        self.ultima_leitura = lidas.len();
        lidas.into_iter().for_each(|v| {
            self.vendas.insert(v.id, v);
        });
        self.vendas.retain(|_, v| v.em >= desde);
        self.max_em = self.vendas.values().map(|v| v.em).max().or(self.max_em);

        let estaticos_vencidos = self
            .estaticos
            .as_ref()
            .is_none_or(|e| agora - e.lidos_em >= RECARGA_ESTATICOS_MS);
        if estaticos_vencidos {
            self.estaticos = Some(EstaticosProntos {
                lidos_em: agora,
                fontes: preparar_estaticos(repo.estaticos().await?),
            });
        }
        let historico_vencido = historico_fresco
            || self
                .historico
                .as_ref()
                .is_none_or(|(t, _, _)| agora - t >= RECARGA_HISTORICO_MS);
        let mut agregados = repo
            .agregados(
                agora - OFERTA_FRESCA_MS,
                agora - OFERTA_FRESCA_POKEMON_MS,
                agora - RESUMO_FRESCO_MS,
                historico_vencido,
            )
            .await?;
        if historico_vencido {
            self.historico = Some((
                agora,
                agregados.hist_itens.clone(),
                agregados.hist_pokemon.clone(),
            ));
        } else if let Some((_, itens, pokemon)) = &self.historico {
            agregados.hist_itens = itens.clone();
            agregados.hist_pokemon = pokemon.clone();
        }

        let mut vendas: Vec<VendaBruta> = self.vendas.values().cloned().collect();
        vendas.sort_by_key(|v| (v.em, v.id));
        let base = self
            .estaticos
            .as_ref()
            .map(|e| e.fontes.clone())
            .unwrap_or_default();
        Ok(Fontes {
            vendas,
            agregados,
            ..base
        })
    }

    pub fn vendas_em_memoria(&self) -> usize {
        self.vendas.len()
    }
}

fn preparar_estaticos(e: Estaticos) -> Fontes {
    Fontes {
        vendas: Vec::new(),
        base: Arc::new(e.base),
        itens_base: Arc::new(e.itens_base),
        central: Arc::new(Central::novo(e.especies)),
        catalogo: Arc::new(Catalogo::novo(e.catalogo)),
        nomes_dim_item: Arc::new(e.nomes_dim_item.into_iter().collect()),
        pesos_confianca: e.pesos_confianca,
        vies: e.vies,
        agregados: Default::default(),
    }
}
