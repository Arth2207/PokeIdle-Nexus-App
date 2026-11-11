//! Dados da equipe da bolsa: wiki de bosses, faltas de coleta, sugestões e preços típicos.
//!
//! Queries com `sqlx::query` (checadas nos testes de integração), pelo mesmo motivo do módulo
//! [`crate::equipes`].

use std::collections::HashMap;

use nucleo::bolsa_equipe::{BossWiki, Faltas};
use serde_json::{json, Map, Value};
use sqlx::{PgPool, Row};

/// Substitui a tabela de bosses da wiki.
pub async fn importar_wiki(db: &PgPool, lista: &[BossWiki]) -> Result<u64, sqlx::Error> {
    let mut tx = db.begin().await?;
    sqlx::query("DELETE FROM nexus.boss_wiki")
        .execute(&mut *tx)
        .await?;
    let mut n = 0;
    for (i, b) in lista.iter().enumerate() {
        sqlx::query("INSERT INTO nexus.boss_wiki (ordem, nome, dados) VALUES ($1, $2, $3)")
            .bind(i32::try_from(i).unwrap_or(i32::MAX))
            .bind(&b.nome)
            .bind(json!({ "nivel": b.nivel, "drops": b.drops, "entrada": b.entrada }))
            .execute(&mut *tx)
            .await?;
        n += 1;
    }
    tx.commit().await?;
    Ok(n)
}

pub async fn carregar_wiki(db: &PgPool) -> Result<Vec<BossWiki>, sqlx::Error> {
    let linhas = sqlx::query("SELECT nome, dados FROM nexus.boss_wiki ORDER BY ordem")
        .fetch_all(db)
        .await?;
    linhas
        .into_iter()
        .map(|l| {
            let d: Value = l.try_get("dados")?;
            Ok(BossWiki {
                nome: l.try_get("nome")?,
                nivel: d.get("nivel").and_then(Value::as_f64).unwrap_or(0.0),
                drops: d.get("drops").cloned().unwrap_or(Value::Null),
                entrada: d.get("entrada").cloned().unwrap_or(Value::Null),
            })
        })
        .collect()
}

pub async fn carregar_faltas(db: &PgPool) -> Result<Faltas, sqlx::Error> {
    let linhas = sqlx::query("SELECT chave, dados FROM nexus.falta_coleta")
        .fetch_all(db)
        .await?;
    let mut m = Map::new();
    for l in linhas {
        m.insert(l.try_get("chave")?, l.try_get("dados")?);
    }
    Ok(Faltas(m))
}

/// Substitui todas as faltas (é um mapa pequeno, gravado numa transação).
pub async fn gravar_faltas(db: &PgPool, f: &Faltas) -> Result<(), sqlx::Error> {
    let mut tx = db.begin().await?;
    sqlx::query("DELETE FROM nexus.falta_coleta")
        .execute(&mut *tx)
        .await?;
    for (k, v) in &f.0 {
        sqlx::query("INSERT INTO nexus.falta_coleta (chave, dados) VALUES ($1, $2)")
            .bind(k)
            .bind(v)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await
}

pub async fn registrar_sugestao(
    db: &PgPool,
    em: i64,
    boss: &str,
    dados: &Value,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO nexus.equipe_sugestao (em, boss, dados) VALUES ($1, $2, $3)")
        .bind(em)
        .bind(boss)
        .bind(dados)
        .execute(db)
        .await?;
    Ok(())
}

/// Preço típico por espécie (`precosTipicos`): mediana das vendas na faixa de nível (com 3
/// ou mais vendas), senão o menor anúncio fresco. Mesmas queries do legado.
pub async fn precos_tipicos(
    db: &PgPool,
    moeda: &str,
    lo: f64,
    hi: f64,
    corte_ofertas: i64,
) -> Result<HashMap<i64, f64>, sqlx::Error> {
    precos_tipicos_modo(db, moeda, lo, hi, corte_ofertas, None).await
}

/// Com `desde_vendas` (modo de melhorias): o nível é lido como número (o `::int` do legado
/// derruba a consulta inteira quando aparece um nível decimal) e só entram as vendas a
/// partir desse instante (o legado varre o histórico todo a cada 30 minutos).
pub async fn precos_tipicos_modo(
    db: &PgPool,
    moeda: &str,
    lo: f64,
    hi: f64,
    corte_ofertas: i64,
    desde_vendas: Option<i64>,
) -> Result<HashMap<i64, f64>, sqlx::Error> {
    if let Some(desde) = desde_vendas {
        let vendas = sqlx::query(
            "SELECT species_id::bigint id, count(*)::int n, percentile_cont(.5) WITHIN GROUP (ORDER BY preco) p
             FROM public.fato_venda_mercado WHERE tipo='pokemon' AND moeda=$1 AND species_id IS NOT NULL AND preco>0
             AND em >= $4 AND (ficha->>'level') ~ '^[0-9]+(\\.[0-9]+)?$'
             AND (ficha->>'level')::numeric BETWEEN $2 AND $3 GROUP BY 1",
        )
        .bind(moeda)
        .bind(lo)
        .bind(hi)
        .bind(desde)
        .fetch_all(db)
        .await?;
        let ofertas = sqlx::query(
            "SELECT species_id::bigint id, min(preco)::float8 p FROM public.fato_oferta_mercado
             WHERE ativo AND tipo='pokemon' AND moeda=$1 AND species_id IS NOT NULL AND ts_visto >= $2
             AND (ficha->>'level') ~ '^[0-9]+(\\.[0-9]+)?$'
             AND (ficha->>'level')::numeric BETWEEN $3 AND $4 GROUP BY 1",
        )
        .bind(moeda)
        .bind(corte_ofertas)
        .bind(lo)
        .bind(hi)
        .fetch_all(db)
        .await?;
        return mesclar(&vendas, &ofertas);
    }
    let vendas = sqlx::query(
        "SELECT species_id::bigint id, count(*)::int n, percentile_cont(.5) WITHIN GROUP (ORDER BY preco) p
         FROM public.fato_venda_mercado WHERE tipo='pokemon' AND moeda=$1 AND species_id IS NOT NULL AND preco>0
         AND NULLIF(ficha->>'level','')::int BETWEEN $2 AND $3 GROUP BY 1",
    )
    .bind(moeda)
    .bind(lo)
    .bind(hi)
    .fetch_all(db)
    .await?;
    let ofertas = sqlx::query(
        "SELECT species_id::bigint id, min(preco)::float8 p FROM public.fato_oferta_mercado
         WHERE ativo AND tipo='pokemon' AND moeda=$1 AND species_id IS NOT NULL AND ts_visto >= $2
         AND NULLIF(ficha->>'level','')::int BETWEEN $3 AND $4 GROUP BY 1",
    )
    .bind(moeda)
    .bind(corte_ofertas)
    .bind(lo)
    .bind(hi)
    .fetch_all(db)
    .await?;
    mesclar(&vendas, &ofertas)
}

fn mesclar(
    vendas: &[sqlx::postgres::PgRow],
    ofertas: &[sqlx::postgres::PgRow],
) -> Result<HashMap<i64, f64>, sqlx::Error> {
    let of: HashMap<i64, f64> = ofertas
        .iter()
        .map(|r| Ok((r.try_get::<i64, _>("id")?, r.try_get::<f64, _>("p")?)))
        .collect::<Result<_, sqlx::Error>>()?;
    let mut m = HashMap::new();
    for r in vendas {
        let (id, n, p): (i64, i32, f64) = (r.try_get("id")?, r.try_get("n")?, r.try_get("p")?);
        if n >= 3 || !of.contains_key(&id) {
            m.insert(id, p);
        }
    }
    for (id, p) in of {
        m.entry(id).or_insert(p);
    }
    Ok(m)
}
