//! Escrita da bolsa. `fato_oferta_mercado` e `fato_resumo_item` são do legado até o cutover:
//! estas funções só rodam com `NEXUS_ESCRITA_BOLSA=true` (em produção, desligado) e nos
//! testes de integração. O inventário (`nexus.meus_itens`) já é do Nexus.

use std::collections::BTreeSet;

use nucleo::bolsa::ingestao::{LinhaResumo, MeusItens, OfertaNova, RepoIngestao};
use nucleo::bolsa::repo::ErroRepo;
use serde_json::{json, Value};

use crate::mercado::{erro, PgRepoMercado};

const LIMPA_VISTO_MS: i64 = 6 * 3_600_000;
const CHAVE_TS_MEUS_ITENS: &str = "bolsa.meus_itens_ts";

impl RepoIngestao for PgRepoMercado {
    async fn gravar_ofertas(
        &self,
        ofertas: &[OfertaNova],
        agora: i64,
        desativar: Option<&BTreeSet<(String, String)>>,
    ) -> Result<u64, ErroRepo> {
        if ofertas.is_empty() {
            return Ok(0);
        }
        let linhas: Vec<Value> = ofertas
            .iter()
            .map(|o| {
                json!({
                    "id": o.id, "ts_primeiro": agora, "ts_visto": agora, "tipo": o.tipo,
                    "moeda": o.moeda, "preco": o.preco, "species_id": o.especie,
                    "item_id": o.item_id, "quantidade": o.quantidade, "nome": o.nome,
                    "ficha": o.ficha, "expira_em": o.expira_em,
                })
            })
            .collect();
        let mut tx = self.db.begin().await.map_err(erro)?;
        let gravadas = sqlx::query!(
            r#"
            INSERT INTO public.fato_oferta_mercado
                (id, ts_primeiro, ts_visto, tipo, moeda, preco, species_id, item_id, quantidade,
                 nome, ficha, expira_em, ativo)
            SELECT id, ts_primeiro, ts_visto, tipo, moeda, preco, species_id, item_id, quantidade,
                   nome, ficha, expira_em, TRUE
            FROM jsonb_to_recordset($1::jsonb) AS x(id bigint, ts_primeiro bigint, ts_visto bigint,
                 tipo text, moeda text, preco bigint, species_id int, item_id int, quantidade int,
                 nome text, ficha jsonb, expira_em bigint)
            ON CONFLICT (id) DO UPDATE SET
                ts_visto = EXCLUDED.ts_visto, preco = EXCLUDED.preco, tipo = EXCLUDED.tipo,
                moeda = EXCLUDED.moeda, species_id = EXCLUDED.species_id,
                item_id = EXCLUDED.item_id, quantidade = EXCLUDED.quantidade,
                nome = EXCLUDED.nome, ficha = EXCLUDED.ficha,
                expira_em = COALESCE(EXCLUDED.expira_em, fato_oferta_mercado.expira_em),
                ativo = TRUE
            "#,
            Value::Array(linhas),
        )
        .execute(&mut *tx)
        .await
        .map_err(erro)?
        .rows_affected();
        if let Some(escopo) = desativar {
            let (tipos, moedas): (Vec<String>, Vec<String>) = escopo.iter().cloned().unzip();
            sqlx::query!(
                r#"
                UPDATE public.fato_oferta_mercado o SET ativo = FALSE
                FROM UNNEST($2::text[], $3::text[]) AS e(tipo, moeda)
                WHERE o.ativo AND o.ts_visto < $1 AND o.tipo = e.tipo AND o.moeda = e.moeda
                "#,
                agora,
                &tipos,
                &moedas,
            )
            .execute(&mut *tx)
            .await
            .map_err(erro)?;
        }
        tx.commit().await.map_err(erro)?;
        Ok(gravadas)
    }

    async fn gravar_resumo(&self, linhas: &[LinhaResumo], ts: i64) -> Result<u64, ErroRepo> {
        if linhas.is_empty() {
            return Ok(0);
        }
        let itens: Vec<i32> = linhas.iter().map(|l| l.item_id).collect();
        let moedas: Vec<String> = linhas.iter().map(|l| l.moeda.to_owned()).collect();
        let anuncios: Vec<i32> = linhas.iter().map(|l| l.anuncios).collect();
        let precos: Vec<i64> = linhas.iter().map(|l| l.preco_min).collect();
        let n = sqlx::query!(
            r#"
            INSERT INTO public.fato_resumo_item (ts, item_id, moeda, anuncios, preco_min)
            SELECT $1, * FROM UNNEST($2::int[], $3::text[], $4::int[], $5::bigint[])
            ON CONFLICT DO NOTHING
            "#,
            ts,
            &itens,
            &moedas,
            &anuncios,
            &precos,
        )
        .execute(&self.db)
        .await
        .map_err(erro)?
        .rows_affected();
        Ok(n)
    }

    async fn limpar_ofertas(&self, agora: i64) -> Result<u64, ErroRepo> {
        let n = sqlx::query!(
            r#"
            UPDATE public.fato_oferta_mercado SET ativo = FALSE
            WHERE ativo AND ((expira_em IS NOT NULL AND expira_em < $1) OR ts_visto < $2)
            "#,
            agora,
            agora - LIMPA_VISTO_MS,
        )
        .execute(&self.db)
        .await
        .map_err(erro)?
        .rows_affected();
        Ok(n)
    }

    async fn meus_itens(&self) -> Result<MeusItens, ErroRepo> {
        let itens =
            sqlx::query!("SELECT item_id, quantidade FROM nexus.meus_itens ORDER BY item_id")
                .fetch_all(&self.db)
                .await
                .map_err(erro)?
                .into_iter()
                .map(|l| (l.item_id, l.quantidade))
                .collect();
        let ts = sqlx::query_scalar!(
            "SELECT valor FROM nexus.parametro WHERE chave = $1",
            CHAVE_TS_MEUS_ITENS
        )
        .fetch_optional(&self.db)
        .await
        .map_err(erro)?
        .and_then(|v| v.as_i64());
        Ok(MeusItens { ts, itens })
    }

    async fn gravar_meus_itens(&self, m: &MeusItens) -> Result<(), ErroRepo> {
        let (ids, qtds): (Vec<i32>, Vec<i64>) = m.itens.iter().map(|(k, v)| (*k, *v)).unzip();
        let mut tx = self.db.begin().await.map_err(erro)?;
        sqlx::query!("DELETE FROM nexus.meus_itens")
            .execute(&mut *tx)
            .await
            .map_err(erro)?;
        sqlx::query!(
            "INSERT INTO nexus.meus_itens (item_id, quantidade) SELECT * FROM UNNEST($1::int[], $2::bigint[])",
            &ids,
            &qtds,
        )
        .execute(&mut *tx)
        .await
        .map_err(erro)?;
        sqlx::query!(
            "INSERT INTO nexus.parametro (chave, valor) VALUES ($1, $2)
             ON CONFLICT (chave) DO UPDATE SET valor = EXCLUDED.valor, atualizado_em = now()",
            CHAVE_TS_MEUS_ITENS,
            json!(m.ts),
        )
        .execute(&mut *tx)
        .await
        .map_err(erro)?;
        tx.commit().await.map_err(erro)
    }
}
