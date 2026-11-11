//! Fotografia do mercado: vendas da janela agrupadas por ticker.
//!
//! O legado monta isso a cada 3 minutos (cache com TTL) e todas as rotas de leitura saem
//! daqui. O cache fica no `nexus-api`; este módulo só sabe montar.

use std::collections::{HashMap, HashSet};

use super::catalogo::Catalogo;
use super::repo::{ErroRepo, RepoMercado};
use super::ticker::TickerId;
use super::venda::{Ponto, VendaBruta};
use super::{DIA_MS, JANELA_DIAS, MIN_VENDAS, SERIE_MAX_PONTOS};

#[derive(Debug, Clone, Default)]
pub struct Mercado {
    pub gerado_em: i64,
    /// Pontos de cada ticker, em ordem de tempo.
    grupos: HashMap<TickerId, Vec<Ponto>>,
    /// Tickers com vendas suficientes na janela. Só eles "existem" para as rotas.
    tickers: HashSet<TickerId>,
}

/// Lê as vendas e o catálogo e monta o mercado.
///
/// `<R: RepoMercado>` é um genérico com trait bound: a função aceita qualquer tipo `R`,
/// desde que ele implemente `RepoMercado`. O compilador gera uma versão da função para cada
/// tipo usado (o Postgres em produção, o repositório em memória nos testes), sem custo de
/// chamada dinâmica. Em JS seria "qualquer objeto com os métodos `vendas` e
/// `catalogo_itens`", mas checado na compilação.
pub async fn carregar<R: RepoMercado>(repo: &R, agora: i64) -> Result<Mercado, ErroRepo> {
    let desde = agora - JANELA_DIAS * DIA_MS;
    let catalogo = Catalogo::novo(repo.catalogo_itens().await?);
    let vendas = repo.vendas(desde, agora).await?;
    Ok(Mercado::montar(vendas, &catalogo, agora))
}

impl Mercado {
    pub fn montar(vendas: Vec<VendaBruta>, catalogo: &Catalogo, agora: i64) -> Self {
        // Iteradores em vez de `for`: `filter_map` descarta as linhas que não viram ticker
        // (como `.map().filter(Boolean)` no JS) e `fold` acumula num HashMap (o `reduce`).
        // Nada roda até o `fold` pedir os itens: os adaptadores são preguiçosos.
        let mut grupos = vendas.iter().filter_map(|v| v.classificar(catalogo)).fold(
            HashMap::<TickerId, Vec<Ponto>>::new(),
            |mut grupos, (id, ponto)| {
                grupos.entry(id).or_default().push(ponto);
                grupos
            },
        );
        // Ordenação estável, como o `Array.sort` do JS: empates mantêm a ordem do banco.
        grupos
            .values_mut()
            .for_each(|pontos| pontos.sort_by_key(|p| p.em));

        let tickers = grupos
            .iter()
            .filter(|(_, pontos)| vendas_validas(pontos, agora) >= MIN_VENDAS)
            .map(|(id, _)| id.clone())
            .collect();

        Self {
            gerado_em: agora,
            grupos,
            tickers,
        }
    }

    pub fn existe(&self, id: &TickerId) -> bool {
        self.tickers.contains(id)
    }

    pub fn total_tickers(&self) -> usize {
        self.tickers.len()
    }
}

/// Os últimos 5000 pontos de um ticker, como `(momento, preço unitário)`.
/// `None` se o ticker não existe.
///
/// Lifetime explícito `'m`: o iterador devolvido aponta para dentro do `mercado`, sem copiar,
/// então só pode ser usado enquanto o mercado existir. Como a função recebe duas
/// referências, o compilador não adivinha de qual delas o resultado depende; o `'m` diz
/// "do mercado, não do `id`". Por isso quem chama pode descartar o `id` logo depois.
///
/// `impl Iterator<...>` no retorno: "devolvo algum iterador desses itens", sem expor o tipo
/// concreto (que seria enorme). Quem chama decide se faz `collect`, conta, etc.
pub fn serie<'m>(
    mercado: &'m Mercado,
    id: &TickerId,
) -> Option<impl Iterator<Item = (i64, f64)> + 'm> {
    if !mercado.existe(id) {
        return None;
    }
    let pontos = mercado.grupos.get(id)?;
    let inicio = pontos.len().saturating_sub(SERIE_MAX_PONTOS);
    Some(pontos[inicio..].iter().map(|p| (p.em, p.unidade)))
}

/// Mesma regra do `analisar` do legado: vendas na janela `(agora - 30d, agora]` com preço > 0.
fn vendas_validas(pontos: &[Ponto], agora: i64) -> usize {
    let inicio = agora - JANELA_DIAS * DIA_MS;
    pontos
        .iter()
        .filter(|p| p.em <= agora && p.em > inicio && p.unidade > 0.0)
        .count()
}

#[cfg(test)]
mod testes {
    use std::sync::Mutex;

    use proptest::prelude::*;

    use super::*;
    use crate::bolsa::catalogo::ItemCatalogo;

    const AGORA: i64 = 1_800_000_000_000;

    fn venda(em: i64, item_id: i32, preco: i64) -> VendaBruta {
        VendaBruta {
            em,
            tipo: "item".into(),
            moeda: "orb".into(),
            preco: Some(preco),
            quantidade: Some(1),
            descricao: Some("Coisa".into()),
            item_id: Some(item_id),
            especie: None,
            shiny: false,
        }
    }

    fn id(texto: &str) -> TickerId {
        texto.parse().unwrap()
    }

    fn serie_vec(m: &Mercado, texto: &str) -> Option<Vec<(i64, f64)>> {
        serie(m, &id(texto)).map(Iterator::collect)
    }

    #[test]
    fn vazio_e_um_ponto_nao_viram_ticker() {
        let m = Mercado::montar(vec![], &Catalogo::default(), AGORA);
        assert_eq!(m.total_tickers(), 0);
        assert!(serie_vec(&m, "i:1:orb").is_none());

        let m = Mercado::montar(vec![venda(AGORA, 1, 10)], &Catalogo::default(), AGORA);
        assert!(serie_vec(&m, "i:1:orb").is_none());
    }

    #[test]
    fn seis_vendas_validas_viram_ticker_em_ordem() {
        let vendas = (0..6)
            .rev()
            .map(|i| venda(AGORA - i * 1000, 1, 10 + i))
            .collect();
        let m = Mercado::montar(vendas, &Catalogo::default(), AGORA);
        let s = serie_vec(&m, "i:1:orb").unwrap();
        assert_eq!(s.len(), 6);
        assert!(s.windows(2).all(|par| par[0].0 <= par[1].0));
    }

    #[test]
    fn preco_zero_nao_conta_mas_aparece_na_serie() {
        let mut vendas: Vec<_> = (0..5).map(|i| venda(AGORA - i, 1, 10)).collect();
        vendas.push(venda(AGORA - 9, 1, 0));
        let m = Mercado::montar(vendas.clone(), &Catalogo::default(), AGORA);
        assert!(!m.existe(&id("i:1:orb")));

        vendas.push(venda(AGORA - 10, 1, 7));
        let m = Mercado::montar(vendas, &Catalogo::default(), AGORA);
        assert_eq!(serie_vec(&m, "i:1:orb").unwrap().len(), 7);
    }

    #[test]
    fn serie_corta_nos_ultimos_5000() {
        let vendas = (0..6000).map(|i| venda(AGORA - 6000 + i, 1, 5)).collect();
        let m = Mercado::montar(vendas, &Catalogo::default(), AGORA);
        let s = serie_vec(&m, "i:1:orb").unwrap();
        assert_eq!(s.len(), SERIE_MAX_PONTOS);
        assert_eq!(s.last().map(|p| p.0), Some(AGORA - 1));
    }

    /// Repositório em memória: a outra implementação da porta. O teste exercita o
    /// `carregar` inteiro sem banco.
    struct RepoMemoria {
        vendas: Vec<VendaBruta>,
        consultas: Mutex<Vec<(i64, i64)>>,
    }

    impl RepoMercado for RepoMemoria {
        async fn vendas(&self, desde: i64, ate: i64) -> Result<Vec<VendaBruta>, ErroRepo> {
            self.consultas.lock().unwrap().push((desde, ate));
            Ok(self
                .vendas
                .iter()
                .filter(|v| v.em >= desde && v.em <= ate)
                .cloned()
                .collect())
        }

        async fn catalogo_itens(&self) -> Result<Vec<ItemCatalogo>, ErroRepo> {
            Ok(vec![ItemCatalogo {
                id: 9,
                nome: "Coisa".into(),
                categoria: None,
                raro: false,
                preco_npc: None,
                icone: None,
            }])
        }
    }

    #[tokio::test]
    async fn carregar_pede_a_janela_e_usa_o_catalogo() {
        let mut vendas: Vec<_> = (0..6).map(|i| venda(AGORA - i, 0, 10)).collect();
        vendas.push(venda(AGORA - 31 * DIA_MS, 0, 10)); // fora da janela
        let repo = RepoMemoria {
            vendas,
            consultas: Mutex::new(vec![]),
        };

        let m = carregar(&repo, AGORA).await.unwrap();

        assert_eq!(
            repo.consultas.lock().unwrap().as_slice(),
            &[(AGORA - 30 * DIA_MS, AGORA)]
        );
        // item_id 0 é tratado como ausente; o nome "Coisa" resolve para o id 9 no catálogo.
        assert_eq!(serie_vec(&m, "i:9:orb").unwrap().len(), 6);
    }

    proptest! {
        #[test]
        fn serie_nunca_tem_nan_nem_passa_do_limite(
            pontos in proptest::collection::vec((0i64..60 * DIA_MS, any::<i64>(), -2i32..50), 0..300)
        ) {
            let vendas = pontos
                .into_iter()
                .map(|(atraso, preco, qtd)| VendaBruta { quantidade: Some(qtd), ..venda(AGORA - atraso, 1, preco) })
                .collect();
            let m = Mercado::montar(vendas, &Catalogo::default(), AGORA);
            if let Some(s) = serie_vec(&m, "i:1:orb") {
                prop_assert!(s.len() >= MIN_VENDAS && s.len() <= SERIE_MAX_PONTOS);
                prop_assert!(s.iter().all(|(_, v)| v.is_finite()));
            }
        }
    }
}
