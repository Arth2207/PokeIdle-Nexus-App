//! Catálogo de itens do jogo (id, nome, ícone...). Usado para achar o id de um item a
//! partir do nome, porque a maioria das vendas de item chega sem `item_id`.

use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct ItemCatalogo {
    pub id: i32,
    pub nome: String,
    pub categoria: Option<String>,
    pub raro: bool,
    pub preco_npc: Option<i64>,
    pub icone: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Catalogo {
    por_nome: HashMap<String, i32>,
}

impl Catalogo {
    /// `impl IntoIterator<Item = ...>` aceita `Vec`, array, ou qualquer coisa que dê para
    /// percorrer, parecido com uma função JS que aceita qualquer iterável.
    ///
    /// Se dois itens tiverem o mesmo nome, vale o último, como no `Map.set` do legado.
    /// `collect()` para `HashMap` faz exatamente isso: a chave repetida sobrescreve.
    pub fn novo(itens: impl IntoIterator<Item = ItemCatalogo>) -> Self {
        let por_nome = itens
            .into_iter()
            .map(|item| (item.nome.to_lowercase(), item.id))
            .collect();
        Self { por_nome }
    }

    /// Recebe o nome já em minúsculas.
    pub fn id_por_nome(&self, nome_minusculo: &str) -> Option<i32> {
        self.por_nome.get(nome_minusculo).copied()
    }
}
