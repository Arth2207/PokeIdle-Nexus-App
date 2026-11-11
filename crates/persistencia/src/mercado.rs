//! Repositório da bolsa em Postgres.

use nucleo::bolsa::catalogo::ItemCatalogo;
use nucleo::bolsa::repo::{ErroRepo, RepoMercado};
use nucleo::bolsa::venda::VendaBruta;
use sqlx::PgPool;

#[derive(Clone)]
pub struct PgRepoMercado {
    db: PgPool,
}

impl PgRepoMercado {
    pub fn novo(db: PgPool) -> Self {
        Self { db }
    }

    /// Substitui o conteúdo do catálogo pelo recebido, numa transação. Um id repetido na
    /// entrada faz a gravação inteira falhar, e o catálogo anterior fica como estava.
    pub async fn gravar_catalogo(&self, itens: &[ItemCatalogo]) -> Result<u64, sqlx::Error> {
        // Uma coluna por array: o Postgres "desenrola" os arrays em linhas com UNNEST.
        // `unzip` não serve para 7 colunas, então cada uma sai de um `map` próprio.
        let ids: Vec<i32> = itens.iter().map(|i| i.id).collect();
        let ordens: Vec<i32> = (0..).take(itens.len()).collect();
        let nomes: Vec<String> = itens.iter().map(|i| i.nome.clone()).collect();
        let categorias: Vec<Option<String>> = itens.iter().map(|i| i.categoria.clone()).collect();
        let raros: Vec<bool> = itens.iter().map(|i| i.raro).collect();
        let npcs: Vec<Option<i64>> = itens.iter().map(|i| i.preco_npc).collect();
        let icones: Vec<Option<String>> = itens.iter().map(|i| i.icone.clone()).collect();

        let mut tx = self.db.begin().await?;
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
}

fn erro(e: sqlx::Error) -> ErroRepo {
    ErroRepo(Box::new(e))
}

impl RepoMercado for PgRepoMercado {
    async fn vendas(&self, desde_ms: i64, ate_ms: i64) -> Result<Vec<VendaBruta>, ErroRepo> {
        // A espécie vem da coluna ou, se nula, da ficha (`species_id ?? ficha.speciesId`).
        // `shiny` exige o booleano `true` no JSON (`ficha.shiny === true`).
        let linhas = sqlx::query!(
            r#"
            SELECT em, tipo, moeda, preco, quantidade, descricao, item_id,
                   COALESCE(species_id,
                            CASE WHEN jsonb_typeof(ficha->'speciesId') = 'number'
                                 THEN (ficha->>'speciesId')::numeric::int END) AS especie,
                   COALESCE(ficha->'shiny' = 'true'::jsonb, false) AS "shiny!"
            FROM public.fato_venda_mercado
            WHERE em >= $1 AND em <= $2
            ORDER BY em, id
            "#,
            desde_ms,
            ate_ms,
        )
        .fetch_all(&self.db)
        .await
        .map_err(erro)?;

        Ok(linhas
            .into_iter()
            .map(|l| VendaBruta {
                em: l.em,
                tipo: l.tipo.unwrap_or_default(),
                moeda: l.moeda.unwrap_or_default(),
                preco: l.preco,
                quantidade: l.quantidade,
                descricao: l.descricao,
                item_id: l.item_id,
                especie: l.especie,
                shiny: l.shiny,
            })
            .collect())
    }

    async fn catalogo_itens(&self) -> Result<Vec<ItemCatalogo>, ErroRepo> {
        let linhas = sqlx::query!(
            "SELECT id, nome, categoria, raro, preco_npc, icone FROM nexus.item_catalogo ORDER BY ordem"
        )
        .fetch_all(&self.db)
        .await
        .map_err(erro)?;

        Ok(linhas
            .into_iter()
            .map(|l| ItemCatalogo {
                id: l.id,
                nome: l.nome,
                categoria: l.categoria,
                raro: l.raro,
                preco_npc: l.preco_npc,
                icone: l.icone,
            })
            .collect())
    }
}
