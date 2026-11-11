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
    por_id: HashMap<i32, ItemCatalogo>,
}

impl Catalogo {
    /// `impl IntoIterator<Item = ...>` aceita `Vec`, array, ou qualquer coisa que dê para
    /// percorrer, parecido com uma função JS que aceita qualquer iterável.
    ///
    /// Se dois itens tiverem o mesmo nome, vale o último, como no `Map.set` do legado.
    /// `collect()` para `HashMap` faz exatamente isso: a chave repetida sobrescreve.
    pub fn novo(itens: impl IntoIterator<Item = ItemCatalogo>) -> Self {
        let itens: Vec<ItemCatalogo> = itens.into_iter().collect();
        let por_nome = itens
            .iter()
            .map(|item| (item.nome.to_lowercase(), item.id))
            .collect();
        let por_id = itens.into_iter().map(|i| (i.id, i)).collect();
        Self { por_nome, por_id }
    }

    /// Recebe o nome já em minúsculas.
    pub fn id_por_nome(&self, nome_minusculo: &str) -> Option<i32> {
        self.por_nome.get(nome_minusculo).copied()
    }

    pub fn item(&self, id: i32) -> Option<&ItemCatalogo> {
        self.por_id.get(&id)
    }

    pub fn icone(&self, id: i32) -> Option<String> {
        self.item(id).and_then(|i| i.icone.clone())
    }

    pub fn len(&self) -> usize {
        self.por_id.len()
    }

    pub fn is_empty(&self) -> bool {
        self.por_id.is_empty()
    }
}
