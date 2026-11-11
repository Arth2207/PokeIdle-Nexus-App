//! Importação, para o schema `nexus`, de dados que o legado guarda em arquivos.
//! Cada importação substitui o conteúdo anterior numa transação.

use nucleo::bolsa::catalogo::ItemCatalogo;
use serde::Deserialize;
use serde_json::Value;
use sqlx::PgPool;

/// Formato do catálogo de espécies (`PI_ESPECIES`, exportado como JSON).
#[derive(Debug, Deserialize)]
pub struct EspecieArquivo {
    pub id: i32,
    pub n: String,
    #[serde(default)]
    pub t: Vec<String>,
    pub b: Vec<i32>,
    #[serde(default)]
    pub m: Value,
    #[serde(default)]
    pub evo: Vec<i32>,
    #[serde(rename = "megaBase")]
    pub mega_base: Option<i32>,
    #[serde(rename = "spriteId")]
    pub sprite_id: Option<i32>,
    pub desempate: Option<i32>,
}

/// Formato da base do precificador (`precos.json`).
#[derive(Debug, Deserialize)]
pub struct BaseArquivo {
    pub vendas: Vec<VendaBaseArquivo>,
    #[serde(default)]
    pub itens: Vec<ItemPrecoArquivo>,
    pub pesos: Option<[f64; 5]>,
    pub bias: Option<f64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VendaBaseArquivo {
    pub at: i64,
    pub price: i64,
    pub base: f64,
    pub addons: f64,
    pub pve: f64,
    pub family: i32,
    pub family_name: String,
    pub shiny: bool,
    pub level: i32,
    pub species_id: i32,
    pub name: String,
    pub uncertain: bool,
    pub nota: Option<f64>,
    pub mega: bool,
    pub refined: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemPrecoArquivo {
    pub day: String,
    pub item_id: i32,
    pub currency: String,
    pub price: f64,
}

/// Substitui o catálogo de itens. Um id repetido faz a gravação inteira falhar, e o
/// catálogo anterior fica como estava.
pub async fn catalogo(db: &PgPool, itens: &[ItemCatalogo]) -> Result<u64, sqlx::Error> {
    // Uma coluna por array: o Postgres "desenrola" os arrays em linhas com UNNEST.
    let ids: Vec<i32> = itens.iter().map(|i| i.id).collect();
    let ordens: Vec<i32> = (0..).take(itens.len()).collect();
    let nomes: Vec<String> = itens.iter().map(|i| i.nome.clone()).collect();
    let categorias: Vec<Option<String>> = itens.iter().map(|i| i.categoria.clone()).collect();
    let raros: Vec<bool> = itens.iter().map(|i| i.raro).collect();
    let npcs: Vec<Option<i64>> = itens.iter().map(|i| i.preco_npc).collect();
    let icones: Vec<Option<String>> = itens.iter().map(|i| i.icone.clone()).collect();

    let mut tx = db.begin().await?;
    sqlx::query!("DELETE FROM nexus.item_catalogo")
        .execute(&mut *tx)
        .await?;
    let gravadas = sqlx::query!(
        r#"
        INSERT INTO nexus.item_catalogo (id, ordem, nome, categoria, raro, preco_npc, icone)
        SELECT * FROM UNNEST($1::int[], $2::int[], $3::text[], $4::text[], $5::bool[], $6::bigint[], $7::text[])
        "#,
        &ids,
        &ordens,
        &nomes,
        &categorias as &[Option<String>],
        &raros,
        &npcs as &[Option<i64>],
        &icones as &[Option<String>],
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();
    tx.commit().await?;
    Ok(gravadas)
}

/// Substitui o catálogo de espécies, preservando a ordem da lista.
pub async fn especies(db: &PgPool, lista: &[EspecieArquivo]) -> Result<u64, sqlx::Error> {
    let mut tx = db.begin().await?;
    sqlx::query!("DELETE FROM nexus.especie")
        .execute(&mut *tx)
        .await?;
    // Os golpes vão como um único JSON (array de arrays), desenrolado no Postgres.
    let linhas: Vec<Value> = lista
        .iter()
        .enumerate()
        .map(|(ordem, e)| {
            serde_json::json!({
                "id": e.id, "ordem": ordem, "nome": e.n, "tipos": e.t, "base": e.b,
                "golpes": if e.m.is_array() { e.m.clone() } else { Value::Array(vec![]) },
                "evolucoes": e.evo, "mega_base": e.mega_base, "sprite_id": e.sprite_id,
                "desempate": e.desempate,
            })
        })
        .collect();
    let gravadas = sqlx::query!(
        r#"
        INSERT INTO nexus.especie (id, ordem, nome, tipos, base, golpes, evolucoes, mega_base, sprite_id, desempate)
        SELECT id, ordem, nome,
               ARRAY(SELECT jsonb_array_elements_text(tipos)),
               ARRAY(SELECT jsonb_array_elements_text(base)::int),
               golpes,
               ARRAY(SELECT jsonb_array_elements_text(evolucoes)::int),
               mega_base, sprite_id, desempate
        FROM jsonb_to_recordset($1::jsonb) AS x(id int, ordem int, nome text, tipos jsonb, base jsonb,
             golpes jsonb, evolucoes jsonb, mega_base int, sprite_id int, desempate int)
        "#,
        Value::Array(linhas),
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();
    tx.commit().await?;
    Ok(gravadas)
}

/// Substitui a base do precificador (vendas antigas, cotações de itens e parâmetros).
pub async fn base(db: &PgPool, b: &BaseArquivo) -> Result<(u64, u64), sqlx::Error> {
    let mut tx = db.begin().await?;
    sqlx::query!("DELETE FROM nexus.venda_base")
        .execute(&mut *tx)
        .await?;
    sqlx::query!("DELETE FROM nexus.item_preco_base")
        .execute(&mut *tx)
        .await?;
    let vendas: Vec<Value> = b
        .vendas
        .iter()
        .map(|v| {
            serde_json::json!({
                "em": v.at, "preco": v.price, "base": v.base, "adicionais": v.addons, "pve": v.pve,
                "familia": v.family, "familia_nome": v.family_name, "shiny": v.shiny,
                "nivel": v.level, "especie": v.species_id, "nome": v.name, "incerta": v.uncertain,
                "nota": v.nota, "mega": v.mega, "refinada": v.refined,
            })
        })
        .collect();
    let n_vendas = sqlx::query!(
        r#"
        INSERT INTO nexus.venda_base (em, preco, base, adicionais, pve, familia, familia_nome, shiny,
                                      nivel, especie, nome, incerta, nota, mega, refinada)
        SELECT * FROM jsonb_to_recordset($1::jsonb) AS x(em bigint, preco bigint, base float8,
             adicionais float8, pve float8, familia int, familia_nome text, shiny bool, nivel int,
             especie int, nome text, incerta bool, nota float8, mega bool, refinada bool)
        "#,
        Value::Array(vendas),
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();
    let itens: Vec<Value> = b
        .itens
        .iter()
        .filter(|i| i.currency == "orb")
        .map(|i| serde_json::json!({ "dia": i.day, "item_id": i.item_id, "preco": i.price }))
        .collect();
    let n_itens = sqlx::query!(
        r#"
        INSERT INTO nexus.item_preco_base (dia, item_id, preco)
        SELECT dia::date, item_id, preco
        FROM jsonb_to_recordset($1::jsonb) AS x(dia text, item_id int, preco float8)
        "#,
        Value::Array(itens),
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();
    for (chave, valor) in [
        (
            "precificador.pesos_confianca",
            b.pesos.map(|p| serde_json::json!(p)),
        ),
        ("precificador.vies", b.bias.map(|v| serde_json::json!(v))),
    ] {
        if let Some(valor) = valor {
            sqlx::query!(
                "INSERT INTO nexus.parametro (chave, valor) VALUES ($1, $2)
                 ON CONFLICT (chave) DO UPDATE SET valor = EXCLUDED.valor, atualizado_em = now()",
                chave,
                valor,
            )
            .execute(&mut *tx)
            .await?;
        }
    }
    tx.commit().await?;
    Ok((n_vendas, n_itens))
}
