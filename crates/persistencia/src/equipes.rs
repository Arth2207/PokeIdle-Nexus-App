//! Pokédex do motor e estado de equipes (`nexus.pokedex_motor`, `nexus.equipes_*`).
//!
//! As queries daqui usam `sqlx::query` (checado em execução, pelos testes de integração) em
//! vez de `query!`: a checagem em compilação exigiria aplicar a migration no banco de
//! desenvolvimento, que é o de produção, antes do deploy.

use nucleo::equipes::EstadoEquipes;
use serde_json::Value;
use sqlx::{PgPool, Row};

/// Substitui a pokédex do motor pelo array do `pokedex.json`, na mesma ordem.
pub async fn importar_pokedex(db: &PgPool, especies: &Value) -> Result<u64, sqlx::Error> {
    let mut tx = db.begin().await?;
    sqlx::query("DELETE FROM nexus.pokedex_motor")
        .execute(&mut *tx)
        .await?;
    let n = sqlx::query(
        "INSERT INTO nexus.pokedex_motor (id, ordem, dados)
         SELECT (e->>'d')::bigint, o::int, e FROM jsonb_array_elements($1) WITH ORDINALITY AS t(e, o)",
    )
    .bind(especies)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    tx.commit().await?;
    Ok(n)
}

/// A pokédex do motor como o array do arquivo.
pub async fn carregar_pokedex(db: &PgPool) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT coalesce(jsonb_agg(dados ORDER BY ordem), '[]') FROM nexus.pokedex_motor",
    )
    .fetch_one(db)
    .await
}

/// Substitui todo o estado (lista de bosses e contas).
pub async fn importar_estado(db: &PgPool, e: &EstadoEquipes) -> Result<u64, sqlx::Error> {
    let mut tx = db.begin().await?;
    gravar_bosses(&mut tx, e).await?;
    sqlx::query("DELETE FROM nexus.equipes_conta")
        .execute(&mut *tx)
        .await?;
    let (nicks, dados): (Vec<&str>, Vec<&Value>) =
        e.contas.iter().map(|(n, d)| (n.as_str(), d)).unzip();
    let dados = Value::Array(dados.into_iter().cloned().collect());
    let n = sqlx::query(
        "INSERT INTO nexus.equipes_conta (nick, ordem, dados)
         SELECT n, o::int, d FROM unnest($1::text[]) WITH ORDINALITY AS a(n, o)
         JOIN jsonb_array_elements($2) WITH ORDINALITY AS b(d, o2) ON o = o2",
    )
    .bind(&nicks)
    .bind(&dados)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    tx.commit().await?;
    Ok(n)
}

async fn gravar_bosses(tx: &mut sqlx::PgConnection, e: &EstadoEquipes) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO nexus.equipes_estado (id, bosses, bosses_em) VALUES (1, $1, $2)
         ON CONFLICT (id) DO UPDATE SET bosses = EXCLUDED.bosses, bosses_em = EXCLUDED.bosses_em",
    )
    .bind(Value::Array(e.bosses.clone()))
    .bind(e.bosses_em)
    .execute(tx)
    .await?;
    Ok(())
}

/// Grava o resultado de um ingest: a lista de bosses e a conta que mudou.
pub async fn gravar_ingest(db: &PgPool, e: &EstadoEquipes, nick: &str) -> Result<(), sqlx::Error> {
    let Some((_, dados)) = e.contas.iter().find(|(n, _)| n == nick) else {
        return Ok(());
    };
    let mut tx = db.begin().await?;
    gravar_bosses(&mut tx, e).await?;
    sqlx::query(
        "INSERT INTO nexus.equipes_conta (nick, ordem, dados)
         VALUES ($1, (SELECT coalesce(max(ordem), 0) + 1 FROM nexus.equipes_conta), $2)
         ON CONFLICT (nick) DO UPDATE SET dados = EXCLUDED.dados",
    )
    .bind(nick)
    .bind(dados)
    .execute(&mut *tx)
    .await?;
    tx.commit().await
}

/// O estado inteiro, com as contas na ordem de chegada.
pub async fn carregar_estado(db: &PgPool) -> Result<EstadoEquipes, sqlx::Error> {
    let linha = sqlx::query("SELECT bosses, bosses_em FROM nexus.equipes_estado WHERE id = 1")
        .fetch_optional(db)
        .await?;
    let (bosses, bosses_em) = match linha {
        Some(l) => (
            l.try_get::<Value, _>("bosses")?,
            l.try_get::<i64, _>("bosses_em")?,
        ),
        None => (Value::Array(Vec::new()), 0),
    };
    let contas = sqlx::query("SELECT nick, dados FROM nexus.equipes_conta ORDER BY ordem")
        .fetch_all(db)
        .await?
        .into_iter()
        .map(|l| {
            Ok((
                l.try_get::<String, _>("nick")?,
                l.try_get::<Value, _>("dados")?,
            ))
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()?;
    Ok(EstadoEquipes {
        bosses: bosses.as_array().cloned().unwrap_or_default(),
        bosses_em,
        contas,
    })
}
