//! Vendas: do formato da tabela para um ponto (momento, preço unitário) de um ticker.

use super::catalogo::Catalogo;
use super::ticker::{ChaveItem, Moeda, TickerId};

/// Uma linha de `fato_venda_mercado`, só com o que a bolsa usa.
#[derive(Debug, Clone, PartialEq)]
pub struct VendaBruta {
    pub em: i64,
    pub tipo: String,
    pub moeda: String,
    pub preco: Option<i64>,
    pub quantidade: Option<i32>,
    pub descricao: Option<String>,
    pub item_id: Option<i32>,
    /// `species_id` da tabela ou, se nulo, o `speciesId` da ficha.
    pub especie: Option<i32>,
    /// `ficha.shiny === true`.
    pub shiny: bool,
}

/// Um ponto da série de um ticker.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ponto {
    pub em: i64,
    pub unidade: f64,
}

impl VendaBruta {
    /// Descobre o ticker e o preço unitário. `None` para linhas que não viram ticker
    /// (moeda desconhecida, pokémon sem espécie).
    ///
    /// Pokémon: o preço é o da carta, sem dividir. Item: preço dividido pela quantidade.
    ///
    /// Pendente: o legado ainda normaliza o preço de pokémon pelo precificador (carta
    /// "típica" da espécie). Até esse porte, as séries de pokémon saem com o preço bruto.
    pub fn classificar(&self, catalogo: &Catalogo) -> Option<(TickerId, Ponto)> {
        let moeda: Moeda = self.moeda.parse().ok()?;
        let preco = self.preco.unwrap_or(0);

        if self.tipo == "pokemon" {
            let id = TickerId::Pokemon {
                especie: self.especie?,
                shiny: self.shiny,
                moeda,
            };
            return Some((id, ponto(self.em, preco, 1)));
        }

        let descricao = self.descricao.as_deref().unwrap_or("");
        let nome = nome_sem_quantidade(descricao).unwrap_or(descricao).trim();
        let nome = nome.to_lowercase();
        // `item_id` 0 conta como ausente, como o `||` do JS.
        let item = match self.item_id.filter(|&id| id != 0) {
            Some(id) => ChaveItem::Id(id),
            None => catalogo
                .id_por_nome(&nome)
                .map_or(ChaveItem::Nome(nome), ChaveItem::Id),
        };
        // `+r.quantidade || 1`, depois `Math.max(1, ...)`.
        let quantidade = self.quantidade.unwrap_or(1).max(1);
        Some((
            TickerId::Item { item, moeda },
            ponto(self.em, preco, quantidade),
        ))
    }
}

fn ponto(em: i64, preco: i64, quantidade: i32) -> Ponto {
    // Conversão para f64: igual ao `Number` do JS. Com quantidade >= 1 o resultado é
    // sempre finito.
    Ponto {
        em,
        unidade: preco as f64 / f64::from(quantidade),
    }
}

/// Tira o prefixo de quantidade da descrição: `"3 × Rare Candy"` vira `"Rare Candy"`.
///
/// Equivale ao regex `/^\d+\s*[×x]\s*(.+)$/` do legado, sem depender de crate de regex.
/// Devolve um pedaço da própria string recebida, sem copiar nada.
pub fn nome_sem_quantidade(descricao: &str) -> Option<&str> {
    let sem_digitos = descricao.trim_start_matches(|c: char| c.is_ascii_digit());
    if sem_digitos.len() == descricao.len() {
        return None; // não começa com dígito
    }
    let depois = sem_digitos.trim_start().strip_prefix(['×', 'x'])?;
    let resto = depois.trim_start();
    // No regex, `.` não casa quebra de linha e `(.+)` precisa de ao menos um caractere.
    // Se depois do "×" só houver espaço, o `\s*` "devolve" o último espaço para o `.+`:
    // casa, e o nome fica vazio depois do trim (desde que esse espaço não seja quebra).
    let quebra = |c: char| matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}');
    if resto.is_empty() {
        let ultimo_ok = depois.chars().next_back().is_some_and(|c| !quebra(c));
        return ultimo_ok.then_some("");
    }
    (!resto.contains(quebra)).then_some(resto)
}

#[cfg(test)]
mod testes {
    use proptest::prelude::*;

    use super::*;
    use crate::bolsa::catalogo::ItemCatalogo;

    fn item(tipo: &str, descricao: &str, item_id: Option<i32>, preco: i64, qtd: i32) -> VendaBruta {
        VendaBruta {
            em: 1,
            tipo: tipo.into(),
            moeda: "gold".into(),
            preco: Some(preco),
            quantidade: Some(qtd),
            descricao: Some(descricao.into()),
            item_id,
            especie: None,
            shiny: false,
        }
    }

    fn catalogo() -> Catalogo {
        Catalogo::novo([ItemCatalogo {
            id: 42,
            nome: "Rare Candy".into(),
            categoria: None,
            raro: false,
            preco_npc: None,
            icone: None,
        }])
    }

    #[test]
    fn tira_quantidade_da_descricao() {
        assert_eq!(nome_sem_quantidade("3 × Rare Candy"), Some("Rare Candy"));
        assert_eq!(nome_sem_quantidade("10x Potion"), Some("Potion"));
        assert_eq!(nome_sem_quantidade("10 x  Potion "), Some("Potion "));
        assert_eq!(nome_sem_quantidade("Potion"), None);
        assert_eq!(nome_sem_quantidade("3 × "), Some(""));
        assert_eq!(nome_sem_quantidade("3 ×"), None);
        assert_eq!(nome_sem_quantidade("3 × \n"), None);
        assert_eq!(nome_sem_quantidade("3 Potion"), None);
    }

    #[test]
    fn item_sem_id_usa_o_catalogo() {
        let (id, p) = item("item", "4 × Rare Candy", None, 400, 4)
            .classificar(&catalogo())
            .unwrap();
        assert_eq!(id.to_string(), "i:42:gold");
        assert_eq!(p.unidade, 100.0);
    }

    #[test]
    fn item_fora_do_catalogo_usa_o_nome() {
        let (id, _) = item("item", "2x Coisa Nova", None, 10, 2)
            .classificar(&catalogo())
            .unwrap();
        assert_eq!(id.to_string(), "i:n:coisa nova:gold");
    }

    #[test]
    fn quantidade_zero_ou_negativa_vale_um() {
        let (_, p) = item("item", "x", Some(7), 50, 0)
            .classificar(&catalogo())
            .unwrap();
        assert_eq!(p.unidade, 50.0);
        let (_, p) = item("item", "x", Some(7), 50, -3)
            .classificar(&catalogo())
            .unwrap();
        assert_eq!(p.unidade, 50.0);
    }

    #[test]
    fn pokemon_nao_divide_e_exige_especie() {
        let mut v = item("pokemon", "Pikachu Nv 50", None, 900, 3);
        assert!(v.classificar(&catalogo()).is_none());
        v.especie = Some(25);
        v.shiny = true;
        let (id, p) = v.classificar(&catalogo()).unwrap();
        assert_eq!(id.to_string(), "p:25:1:gold");
        assert_eq!(p.unidade, 900.0);
    }

    proptest! {
        #[test]
        fn preco_unitario_sempre_finito(
            preco in proptest::option::of(any::<i64>()),
            quantidade in proptest::option::of(any::<i32>()),
            descricao in ".*",
            tipo in prop_oneof![Just("item"), Just("pokemon")],
        ) {
            let v = VendaBruta {
                em: 0, tipo: tipo.into(), moeda: "orb".into(), preco, quantidade,
                descricao: Some(descricao), item_id: None, especie: Some(1), shiny: false,
            };
            if let Some((_, p)) = v.classificar(&catalogo()) {
                prop_assert!(p.unidade.is_finite());
            }
        }
    }
}
