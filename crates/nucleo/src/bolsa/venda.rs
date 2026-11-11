//! Vendas cruas da tabela e o tratamento da descrição dos itens.

/// Uma linha de `fato_venda_mercado`, só com o que a bolsa usa.
#[derive(Debug, Clone, PartialEq)]
pub struct VendaBruta {
    pub id: i64,
    pub em: i64,
    pub lido_em: Option<i64>,
    /// Ficha do pokémon vendido (JSON cru).
    pub ficha: Option<serde_json::Value>,
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

    proptest! {
        #[test]
        fn nunca_entra_em_panico(texto in ".*") {
            let _ = nome_sem_quantidade(&texto);
        }
    }
}
