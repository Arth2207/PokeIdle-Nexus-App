//! Repositório de leitura da bolsa em Postgres. As tabelas do schema `public` pertencem ao
//! legado e aqui só são lidas; as consultas reproduzem as do `bolsa.js`.

use nucleo::bolsa::catalogo::ItemCatalogo;
use nucleo::bolsa::repo::{
    Agregado, AgregadoItem, AgregadoPokemon, Agregados, Ask, ErroRepo, Estaticos, FiltroOfertas,
    OfertaBruta, RepoMercado,
};
use nucleo::bolsa::venda::VendaBruta;
use nucleo::precificador::cotacao::PrecoItemDia;
use nucleo::precificador::especie::{Especie, Golpe};
use nucleo::precificador::motor::LinhaMotor;
use serde_json::Value;
use sqlx::PgPool;

#[derive(Clone)]
pub struct PgRepoMercado {
    pub(crate) db: PgPool,
}

impl PgRepoMercado {
    pub fn novo(db: PgPool) -> Self {
        Self { db }
    }
}

pub(crate) fn erro(e: sqlx::Error) -> ErroRepo {
    ErroRepo(Box::new(e))
}

/// `ficha` JSON `null` conta como ausente, como o `r.ficha &&` do legado.
fn ficha(v: Option<Value>) -> Option<Value> {
    v.filter(|f| !f.is_null())
}

/// Monta a venda a partir de uma linha. Uma macro em vez de função porque cada `query!`
/// gera um tipo de linha diferente (anônimo), com os mesmos nomes de campo.
macro_rules! venda {
    ($l:expr) => {
        VendaBruta {
            id: $l.id,
            em: $l.em,
            lido_em: $l.lido_em,
            ficha: ficha($l.ficha),
            tipo: $l.tipo.unwrap_or_default(),
            moeda: $l.moeda.unwrap_or_default(),
            preco: $l.preco,
            quantidade: $l.quantidade,
            descricao: $l.descricao,
            item_id: $l.item_id,
            especie: $l.especie,
            shiny: $l.shiny,
        }
    };
}

macro_rules! oferta {
    ($l:expr) => {
        OfertaBruta {
            id: $l.id,
            ts_visto: $l.ts_visto,
            tipo: $l.tipo,
            moeda: $l.moeda,
            preco: $l.preco,
            especie: $l.species_id,
            item_id: $l.item_id,
            quantidade: $l.quantidade,
            nome: $l.nome,
            ficha: ficha($l.ficha),
        }
    };
}

fn agregado(
    n: Option<i32>,
    mn: Option<f64>,
    mx: Option<f64>,
    q: Option<Vec<f64>>,
    desde: Option<i64>,
) -> Agregado {
    Agregado {
        n: i64::from(n.unwrap_or(0)),
        min: mn.unwrap_or(f64::NAN),
        max: mx.unwrap_or(f64::NAN),
        q: q.unwrap_or_default(),
        desde,
    }
}

impl RepoMercado for PgRepoMercado {
    async fn vendas(&self, desde_ms: i64, ate_ms: i64) -> Result<Vec<VendaBruta>, ErroRepo> {
        // A espécie vem da coluna ou, se nula, da ficha (`species_id ?? ficha.speciesId`).
        // `shiny` exige o booleano `true` no JSON (`ficha.shiny === true`).
        let linhas = sqlx::query!(
            r#"
            SELECT id, em, lido_em, tipo, moeda, preco, quantidade, descricao, item_id, ficha,
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
        Ok(linhas.into_iter().map(|l| venda!(l)).collect())
    }

    async fn vendas_recentes(&self, em_desde: i64) -> Result<Vec<VendaBruta>, ErroRepo> {
        let linhas = sqlx::query!(
            r#"
            SELECT id, em, lido_em, tipo, moeda, preco, quantidade, descricao, item_id, ficha,
                   COALESCE(species_id,
                            CASE WHEN jsonb_typeof(ficha->'speciesId') = 'number'
                                 THEN (ficha->>'speciesId')::numeric::int END) AS especie,
                   COALESCE(ficha->'shiny' = 'true'::jsonb, false) AS "shiny!"
            FROM public.fato_venda_mercado
            WHERE em >= $1
            ORDER BY em, id
            "#,
            em_desde,
        )
        .fetch_all(&self.db)
        .await
        .map_err(erro)?;
        Ok(linhas.into_iter().map(|l| venda!(l)).collect())
    }

    async fn estaticos(&self) -> Result<Estaticos, ErroRepo> {
        let especies = sqlx::query!(
            "SELECT id, nome, tipos, base, golpes, evolucoes, mega_base, sprite_id, desempate
             FROM nexus.especie ORDER BY ordem"
        )
        .fetch_all(&self.db)
        .await
        .map_err(erro)?
        .into_iter()
        .filter_map(|l| {
            let base: [f64; 6] = l
                .base
                .iter()
                .map(|b| f64::from(*b))
                .collect::<Vec<_>>()
                .try_into()
                .ok()?;
            Some(Especie {
                id: l.id,
                nome: l.nome,
                tipos: l.tipos,
                base,
                golpes: serde_json::from_value::<Vec<Golpe>>(l.golpes).unwrap_or_default(),
                evolucoes: l.evolucoes,
                mega_base: l.mega_base,
                sprite_id: l.sprite_id,
                desempate: l.desempate,
            })
        })
        .collect();

        let base = sqlx::query!(
            "SELECT em, preco, base, adicionais, pve, familia, familia_nome, shiny, nivel, especie,
                    nome, incerta, nota, mega, refinada
             FROM nexus.venda_base ORDER BY em"
        )
        .fetch_all(&self.db)
        .await
        .map_err(erro)?
        .into_iter()
        .map(|l| LinhaMotor {
            em: l.em,
            preco: l.preco as f64,
            base: l.base,
            log_base: l.base.ln(),
            adicionais: l.adicionais,
            mega: l.mega,
            pve: l.pve,
            familia: l.familia,
            familia_nome: l.familia_nome,
            shiny: l.shiny,
            nivel: f64::from(l.nivel),
            especie: l.especie,
            nome: Some(l.nome),
            nota: l.nota,
            incerta: l.incerta,
            refinada: l.refinada,
        })
        .collect();

        let itens_base = sqlx::query!(
            r#"SELECT (dia - date '1970-01-01') AS "dia!", item_id, preco FROM nexus.item_preco_base ORDER BY dia, item_id"#
        )
        .fetch_all(&self.db)
        .await
        .map_err(erro)?
        .into_iter()
        .map(|l| PrecoItemDia {
            dia: i64::from(l.dia),
            item_id: l.item_id,
            preco: l.preco,
        })
        .collect();

        let catalogo = sqlx::query!(
            "SELECT id, nome, categoria, raro, preco_npc, icone FROM nexus.item_catalogo ORDER BY ordem"
        )
        .fetch_all(&self.db)
        .await
        .map_err(erro)?
        .into_iter()
        .map(|l| ItemCatalogo {
            id: l.id,
            nome: l.nome,
            categoria: l.categoria,
            raro: l.raro,
            preco_npc: l.preco_npc,
            icone: l.icone,
        })
        .collect();

        let nomes_dim_item = sqlx::query!("SELECT item_id, nome FROM public.dim_item")
            .fetch_all(&self.db)
            .await
            .map_err(erro)?
            .into_iter()
            .map(|l| (l.item_id, l.nome))
            .collect();

        let parametros: Vec<(String, Value)> = sqlx::query!(
            "SELECT chave, valor FROM nexus.parametro WHERE chave LIKE 'precificador.%'"
        )
        .fetch_all(&self.db)
        .await
        .map_err(erro)?
        .into_iter()
        .map(|l| (l.chave, l.valor))
        .collect();
        let parametro = |k: &str| parametros.iter().find(|(c, _)| c == k).map(|(_, v)| v);

        Ok(Estaticos {
            especies,
            base,
            itens_base,
            catalogo,
            nomes_dim_item,
            pesos_confianca: parametro("precificador.pesos_confianca")
                .and_then(|v| serde_json::from_value::<[f64; 5]>(v.clone()).ok()),
            vies: parametro("precificador.vies").and_then(Value::as_f64),
        })
    }

    async fn agregados(
        &self,
        corte_item: i64,
        corte_pokemon: i64,
        corte_resumo: i64,
        com_historico: bool,
    ) -> Result<Agregados, ErroRepo> {
        let asks = sqlx::query!(
            "SELECT DISTINCT ON (item_id, moeda) item_id, moeda, anuncios, preco_min, ts
             FROM public.fato_resumo_item WHERE ts >= $1 ORDER BY item_id, moeda, ts DESC",
            corte_resumo
        )
        .fetch_all(&self.db)
        .await
        .map_err(erro)?
        .into_iter()
        .map(|l| Ask {
            item_id: l.item_id,
            moeda: l.moeda,
            anuncios: l.anuncios.map(i64::from),
            preco_min: l.preco_min.map_or(f64::NAN, |p| p as f64),
        })
        .collect();

        let ofertas_itens = sqlx::query!(
            r#"
            SELECT item_id AS "id!", moeda, count(*)::int AS n, min(p) AS mn, max(p) AS mx,
                   percentile_cont(ARRAY[.25,.5,.75]) WITHIN GROUP (ORDER BY p) AS q
            FROM (SELECT item_id, moeda, preco::float / GREATEST(quantidade, 1) AS p
                  FROM public.fato_oferta_mercado
                  WHERE ativo AND ts_visto >= $1 AND tipo = 'item' AND item_id IS NOT NULL) x
            GROUP BY 1, 2
            "#,
            corte_item
        )
        .fetch_all(&self.db)
        .await
        .map_err(erro)?
        .into_iter()
        .map(|l| AgregadoItem {
            chave: l.id.to_string(),
            moeda: l.moeda.unwrap_or_else(|| "null".into()),
            agregado: agregado(l.n, l.mn, l.mx, l.q, None),
        })
        .collect();

        let ofertas_pokemon = sqlx::query!(
            r#"
            SELECT species_id AS "id!", COALESCE((ficha->>'shiny') = 'true', false) AS "sh!",
                   moeda, count(*)::int AS n, min(p) AS mn, max(p) AS mx,
                   percentile_cont(ARRAY[.25,.5,.75]) WITHIN GROUP (ORDER BY p) AS q
            FROM (SELECT species_id, ficha, moeda, preco::float AS p
                  FROM public.fato_oferta_mercado
                  WHERE ativo AND ts_visto >= $1 AND tipo = 'pokemon' AND species_id IS NOT NULL) x
            GROUP BY 1, 2, 3
            "#,
            corte_pokemon
        )
        .fetch_all(&self.db)
        .await
        .map_err(erro)?
        .into_iter()
        .map(|l| AgregadoPokemon {
            especie: l.id,
            shiny: l.sh,
            moeda: l.moeda.unwrap_or_else(|| "null".into()),
            agregado: agregado(l.n, l.mn, l.mx, l.q, None),
        })
        .collect();

        if !com_historico {
            return Ok(Agregados {
                asks,
                ofertas_itens,
                ofertas_pokemon,
                ..Default::default()
            });
        }
        let hist_itens = sqlx::query!(
            r#"
            SELECT COALESCE(item_id::text,
                            lower(trim(regexp_replace(descricao, '^\d+\s*[×x]\s*', '')))) AS id,
                   moeda, count(*)::int AS n, min(p) AS mn, max(p) AS mx,
                   percentile_cont(ARRAY[.05,.95]) WITHIN GROUP (ORDER BY p) AS q,
                   min(em) AS desde
            FROM (SELECT item_id, descricao, moeda, em,
                         preco::float / GREATEST(quantidade, 1) AS p
                  FROM public.fato_venda_mercado WHERE tipo <> 'pokemon' AND preco > 0) x
            GROUP BY 1, 2
            "#
        )
        .fetch_all(&self.db)
        .await
        .map_err(erro)?
        .into_iter()
        .map(|l| AgregadoItem {
            chave: l.id.unwrap_or_else(|| "null".into()),
            moeda: l.moeda.unwrap_or_else(|| "null".into()),
            agregado: agregado(l.n, l.mn, l.mx, l.q, l.desde),
        })
        .collect();

        let hist_pokemon = sqlx::query!(
            r#"
            SELECT species_id AS "id!", COALESCE((ficha->>'shiny') = 'true', false) AS "sh!",
                   moeda, count(*)::int AS n, min(p) AS mn, max(p) AS mx,
                   percentile_cont(ARRAY[.05,.95]) WITHIN GROUP (ORDER BY p) AS q,
                   min(em) AS desde
            FROM (SELECT species_id, ficha, moeda, em, preco::float AS p
                  FROM public.fato_venda_mercado
                  WHERE tipo = 'pokemon' AND species_id IS NOT NULL AND preco > 0) x
            GROUP BY 1, 2, 3
            "#
        )
        .fetch_all(&self.db)
        .await
        .map_err(erro)?
        .into_iter()
        .map(|l| AgregadoPokemon {
            especie: l.id,
            shiny: l.sh,
            moeda: l.moeda.unwrap_or_else(|| "null".into()),
            agregado: agregado(l.n, l.mn, l.mx, l.q, l.desde),
        })
        .collect();

        Ok(Agregados {
            asks,
            ofertas_itens,
            ofertas_pokemon,
            hist_itens,
            hist_pokemon,
        })
    }

    async fn ofertas_ativas(&self, f: &FiltroOfertas) -> Result<Vec<OfertaBruta>, ErroRepo> {
        let linhas = sqlx::query!(
            r#"
            SELECT id, ts_visto, tipo, moeda, preco, species_id, item_id, quantidade, nome, ficha
            FROM public.fato_oferta_mercado
            WHERE ativo
              AND ts_visto >= (CASE WHEN tipo = 'pokemon' THEN $1::bigint ELSE $4::bigint END)
              AND ($2::text IS NULL OR tipo = $2)
              AND ($3::text IS NULL OR moeda = $3)
            ORDER BY ts_visto DESC, id DESC
            LIMIT $5
            "#,
            f.corte_pokemon,
            f.tipo,
            f.moeda,
            f.corte_item,
            f.limite,
        )
        .fetch_all(&self.db)
        .await
        .map_err(erro)?;
        Ok(linhas.into_iter().map(|l| oferta!(l)).collect())
    }

    async fn ofertas_do_ticker(
        &self,
        pokemon: bool,
        id: i32,
        moeda: &str,
        corte: i64,
    ) -> Result<Vec<OfertaBruta>, ErroRepo> {
        let linhas = sqlx::query!(
            r#"
            SELECT id, ts_visto, tipo, moeda, preco, species_id, item_id, quantidade, nome, ficha
            FROM public.fato_oferta_mercado
            WHERE ativo AND ts_visto >= $1 AND moeda = $2
              AND (CASE WHEN $4 THEN species_id ELSE item_id END) = $3
            ORDER BY preco ASC, id
            LIMIT 200
            "#,
            corte,
            moeda,
            id,
            pokemon,
        )
        .fetch_all(&self.db)
        .await
        .map_err(erro)?;
        Ok(linhas.into_iter().map(|l| oferta!(l)).collect())
    }
}
