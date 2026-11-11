//! Registro das rodadas do comparador de paridade (`nexus.paridade`).

use serde_json::Value;
use sqlx::PgPool;

/// Resultado de uma rota numa rodada.
#[derive(Debug, Clone, PartialEq)]
pub struct Linha {
    pub rota: String,
    pub comparados: i32,
    pub iguais: i32,
    pub so_legado: i32,
    pub so_nexus: i32,
    pub exemplos: Vec<String>,
}

/// Última rodada de cada rota, com a idade em segundos.
#[derive(Debug, Clone, PartialEq)]
pub struct Ultima {
    pub linha: Linha,
    pub idade_s: f64,
}

/// Grava uma rodada e apaga o que tiver mais de 30 dias.
pub async fn gravar(db: &PgPool, linhas: &[Linha]) -> Result<(), sqlx::Error> {
    let mut tx = db.begin().await?;
    for l in linhas {
        sqlx::query!(
            "INSERT INTO nexus.paridade (rota, comparados, iguais, so_legado, so_nexus, exemplos)
             VALUES ($1, $2, $3, $4, $5, $6)",
            l.rota,
            l.comparados,
            l.iguais,
            l.so_legado,
            l.so_nexus,
            Value::from(l.exemplos.clone()),
        )
        .execute(&mut *tx)
        .await?;
    }
    sqlx::query!("DELETE FROM nexus.paridade WHERE em < now() - interval '30 days'")
        .execute(&mut *tx)
        .await?;
    tx.commit().await
}

pub async fn ultimas(db: &PgPool) -> Result<Vec<Ultima>, sqlx::Error> {
    let linhas = sqlx::query!(
        r#"
        SELECT DISTINCT ON (rota) rota, comparados, iguais, so_legado, so_nexus, exemplos,
               extract(epoch FROM now() - em)::float8 AS "idade_s!"
        FROM nexus.paridade
        ORDER BY rota, em DESC
        "#
    )
    .fetch_all(db)
    .await?;
    Ok(linhas
        .into_iter()
        .map(|l| Ultima {
            linha: Linha {
                rota: l.rota,
                comparados: l.comparados,
                iguais: l.iguais,
                so_legado: l.so_legado,
                so_nexus: l.so_nexus,
                exemplos: serde_json::from_value(l.exemplos).unwrap_or_default(),
            },
            idade_s: l.idade_s,
        })
        .collect())
}
