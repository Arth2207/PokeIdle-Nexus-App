//! Anúncio do jogo (`market` aba `vitrine` ou `item`) → oferta no formato que a Bolsa recebe.
//! Mesmas regras do motor antigo: só anúncio aberto, sem diamante, sem vendedor/comprador e sem
//! apelido; o preço vai como total (unitário × quantidade).

use serde_json::{json, Value};

/// Ofertas válidas de uma resposta `market` (vitrine de pokémon ou anúncios de um item).
pub fn ofertas_da_resposta(m: &Value) -> Vec<Value> {
    let aba = m.get("aba").and_then(Value::as_str).unwrap_or("");
    if aba != "vitrine" && aba != "item" {
        return Vec::new();
    }
    let linhas = m.get("linhas").and_then(Value::as_array);
    linhas
        .into_iter()
        .flatten()
        .filter_map(|a| oferta(a, m, aba == "item"))
        .collect()
}

fn oferta(a: &Value, m: &Value, de_item: bool) -> Option<Value> {
    a.get("id").filter(|v| !v.is_null())?;
    if a.get("estado")
        .and_then(Value::as_str)
        .is_some_and(|e| e != "aberto")
    {
        return None;
    }
    let tipo = a
        .get("tipo")
        .and_then(Value::as_str)
        .unwrap_or(if de_item { "item" } else { "" });
    if tipo == "diamante" {
        return None;
    }
    let unit = a.get("preco").and_then(Value::as_f64).unwrap_or(0.0);
    if de_item && unit <= 0.0 {
        return None;
    }
    let qtd = a.get("qtd").and_then(Value::as_f64).unwrap_or(1.0).max(1.0);
    let mut ficha = a.get("ficha").cloned().unwrap_or(Value::Null);
    if let Some(f) = ficha.as_object_mut() {
        f.remove("apelido");
    }
    let de = |k: &str| a.get(k).cloned().filter(|v| !v.is_null());
    let item_id = de("itemId").or_else(|| de_item.then(|| m.get("itemId").cloned()).flatten());
    let moeda = de("moeda").or_else(|| m.get("moeda").cloned());
    Some(json!({
        "id": a["id"],
        "tipo": tipo,
        "moeda": moeda,
        "preco": unit * qtd,
        "quantidade": qtd,
        "nome": de("nome").or_else(|| ficha.get("nome").cloned()),
        "speciesId": de("speciesId").or_else(|| ficha.get("speciesId").cloned()),
        "itemId": item_id,
        "ficha": ficha,
        "expiraEm": de("expiraEm"),
    }))
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn vitrine_vira_oferta_sem_vendedor_nem_apelido() {
        let m = json!({"t": "market", "aba": "vitrine", "linhas": [
            {"id": 7, "estado": "aberto", "tipo": "pokemon", "moeda": "orb", "preco": 200, "qtd": 1,
             "vendedor": "x", "ficha": {"nome": "Rhyperior", "speciesId": 464, "apelido": "y"}},
            {"id": 8, "estado": "vendido", "tipo": "pokemon", "preco": 1},
            {"id": 9, "tipo": "diamante", "preco": 1}
        ]});
        let o = ofertas_da_resposta(&m);
        assert_eq!(o.len(), 1);
        assert_eq!(o[0]["nome"], "Rhyperior");
        assert_eq!(o[0]["speciesId"], 464);
        assert_eq!(o[0]["preco"], 200.0);
        assert!(o[0].get("vendedor").is_none());
        assert!(o[0]["ficha"].get("apelido").is_none());
    }

    #[test]
    fn anuncio_de_item_herda_item_e_moeda_e_preco_e_total() {
        let m = json!({"t": "market", "aba": "item", "itemId": 23, "moeda": "gold",
            "linhas": [{"id": 1, "preco": 10, "qtd": 3}, {"id": 2, "preco": 0}]});
        let o = ofertas_da_resposta(&m);
        assert_eq!(o.len(), 1);
        assert_eq!(o[0]["tipo"], "item");
        assert_eq!(o[0]["itemId"], 23);
        assert_eq!(o[0]["moeda"], "gold");
        assert_eq!(o[0]["preco"], 30.0);
        assert_eq!(o[0]["quantidade"], 3.0);
    }
}
