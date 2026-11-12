//! Estado do motor de automação (fase 4): import do `holodex-dados.json` e do `contas.json`
//! do legado, e a reconstrução do arquivo a partir das tabelas para comparar.

use serde_json::{Map, Value};
use sqlx::PgPool;

/// Chaves de topo que não são do motor e ficam fora do banco.
pub const CHAVES_FORA: &[&str] = &["acesso"];

/// Quantas linhas cada parte da importação gravou.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Contagem {
    pub config: u64,
    pub acoes: u64,
    pub estado: u64,
}

/// Erro de formato do arquivo (antes de tocar no banco).
#[derive(Debug, thiserror::Error)]
pub enum ErroMotor {
    #[error("formato inesperado: {0}")]
    Formato(&'static str),
    #[error(transparent)]
    Banco(#[from] sqlx::Error),
}

/// Substitui todo o estado do motor pelo do arquivo, numa transação.
pub async fn importar_dados(db: &PgPool, d: &Value, sha256: &str) -> Result<Contagem, ErroMotor> {
    let topo = d
        .as_object()
        .ok_or(ErroMotor::Formato("o topo não é objeto"))?;
    let config = match topo.get("config") {
        Some(Value::Object(c)) => c.clone(),
        None => Map::new(),
        Some(_) => return Err(ErroMotor::Formato("config não é objeto")),
    };
    let acoes = match topo.get("acoes") {
        Some(Value::Array(a)) => a.clone(),
        None => Vec::new(),
        Some(_) => return Err(ErroMotor::Formato("acoes não é lista")),
    };
    // Valida tudo antes de abrir a transação.
    let mut linhas_config: Vec<(String, String, Value)> = Vec::new();
    for (nick, secoes) in &config {
        let secoes = secoes
            .as_object()
            .ok_or(ErroMotor::Formato("config de conta não é objeto"))?;
        if secoes.is_empty() {
            // Conta sem nenhuma seção: a linha com seção vazia guarda só o nick.
            linhas_config.push((nick.clone(), String::new(), Value::Null));
        }
        for (secao, dados) in secoes {
            linhas_config.push((nick.clone(), secao.clone(), dados.clone()));
        }
    }
    if acoes
        .iter()
        .any(|a| a.get("t").and_then(Value::as_i64).is_none())
    {
        return Err(ErroMotor::Formato("ação sem `t` numérico"));
    }

    let mut tx = db.begin().await?;
    for apagar in [
        "DELETE FROM nexus.motor_config",
        "DELETE FROM nexus.motor_acao",
        "DELETE FROM nexus.motor_estado",
    ] {
        sqlx::query(apagar).execute(&mut *tx).await?;
    }
    let (nicks, secoes, dados): (Vec<String>, Vec<String>, Vec<Value>) =
        linhas_config.into_iter().fold(
            (Vec::new(), Vec::new(), Vec::new()),
            |(mut n, mut s, mut d), (a, b, c)| {
                n.push(a);
                s.push(b);
                d.push(c);
                (n, s, d)
            },
        );
    let n_config = sqlx::query(
        "INSERT INTO nexus.motor_config (nick, secao, dados)
         SELECT n, s, d FROM unnest($1::text[], $2::text[]) WITH ORDINALITY AS a(n, s, o)
         JOIN jsonb_array_elements($3) WITH ORDINALITY AS b(d, o2) ON o = o2",
    )
    .bind(&nicks)
    .bind(&secoes)
    .bind(Value::Array(dados))
    .execute(&mut *tx)
    .await?
    .rows_affected();
    // O arquivo guarda da mais nova para a mais antiga; o id cresce no sentido do tempo.
    let n_acoes = sqlx::query(
        "INSERT INTO nexus.motor_acao (nick, t, tipo, ok, dados)
         SELECT a->>'nick', (a->>'t')::bigint, a->>'tipo',
                CASE jsonb_typeof(a->'ok') WHEN 'boolean' THEN (a->>'ok')::boolean END, a
         FROM jsonb_array_elements($1) WITH ORDINALITY AS x(a, o)
         ORDER BY o DESC",
    )
    .bind(Value::Array(acoes))
    .execute(&mut *tx)
    .await?
    .rows_affected();
    let resto: Map<String, Value> = topo
        .iter()
        .filter(|(k, _)| {
            !matches!(k.as_str(), "config" | "acoes") && !CHAVES_FORA.contains(&k.as_str())
        })
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let n_estado = sqlx::query(
        "INSERT INTO nexus.motor_estado (chave, dados) SELECT key, value FROM jsonb_each($1)",
    )
    .bind(Value::Object(resto))
    .execute(&mut *tx)
    .await?
    .rows_affected();
    let total = i32::try_from(n_config + n_acoes + n_estado).unwrap_or(i32::MAX);
    registrar(&mut tx, "dados", sha256, total).await?;
    tx.commit().await?;
    Ok(Contagem {
        config: n_config,
        acoes: n_acoes,
        estado: n_estado,
    })
}

/// Monta de volta o objeto do `holodex-dados.json` (sem as [`CHAVES_FORA`]).
pub async fn carregar_dados(db: &PgPool) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT jsonb_build_object(
            'config', coalesce((SELECT jsonb_object_agg(nick, secoes) FROM (
                SELECT nick, coalesce(jsonb_object_agg(secao, dados) FILTER (WHERE secao <> ''), '{}')
                       AS secoes
                FROM nexus.motor_config GROUP BY nick) c), '{}'),
            'acoes', coalesce((SELECT jsonb_agg(dados ORDER BY id DESC) FROM nexus.motor_acao), '[]'))
         || coalesce((SELECT jsonb_object_agg(chave, dados) FROM nexus.motor_estado), '{}')",
    )
    .fetch_one(db)
    .await
}

/// Substitui as contas do motor pelas do `contas.json`.
pub async fn importar_contas(db: &PgPool, contas: &Value, sha256: &str) -> Result<u64, ErroMotor> {
    let lista = contas
        .as_array()
        .ok_or(ErroMotor::Formato("contas.json não é lista"))?;
    if lista.iter().any(|c| {
        c.get("id").and_then(Value::as_str).is_none()
            || c.get("nome").and_then(Value::as_str).is_none()
    }) {
        return Err(ErroMotor::Formato("conta sem `id` ou `nome` texto"));
    }
    let mut tx = db.begin().await?;
    sqlx::query("DELETE FROM nexus.motor_conta")
        .execute(&mut *tx)
        .await?;
    let n = sqlx::query(
        "INSERT INTO nexus.motor_conta (id, nome, cor, ordem)
         SELECT c->>'id', c->>'nome', c->>'cor', o::int
         FROM jsonb_array_elements($1) WITH ORDINALITY AS x(c, o)",
    )
    .bind(contas)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    registrar(
        &mut tx,
        "contas",
        sha256,
        i32::try_from(n).unwrap_or(i32::MAX),
    )
    .await?;
    tx.commit().await?;
    Ok(n)
}

/// As contas como o array do `contas.json`.
pub async fn carregar_contas(db: &PgPool) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT coalesce(jsonb_agg(jsonb_strip_nulls(jsonb_build_object('id', id, 'nome', nome, 'cor', cor))
                ORDER BY ordem), '[]')
         FROM nexus.motor_conta",
    )
    .fetch_one(db)
    .await
}

async fn registrar(
    tx: &mut sqlx::PgConnection,
    tipo: &str,
    sha256: &str,
    linhas: i32,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO nexus.motor_importacao (tipo, sha256, linhas) VALUES ($1, $2, $3)")
        .bind(tipo)
        .bind(sha256)
        .bind(linhas)
        .execute(tx)
        .await?;
    Ok(())
}

/// Primeira diferença exata entre dois JSONs, com o caminho. `None` se forem iguais.
/// Sem tolerância: o banco tem de devolver o arquivo igual (números comparados pelo valor,
/// como no JS: `1` e `1.0` são o mesmo número).
pub fn primeira_diferenca(a: &Value, b: &Value, caminho: &str) -> Option<String> {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            let chaves: std::collections::BTreeSet<&String> = x.keys().chain(y.keys()).collect();
            chaves.into_iter().find_map(|k| {
                let c = format!("{caminho}.{k}");
                match (x.get(k), y.get(k)) {
                    (Some(p), Some(q)) => primeira_diferenca(p, q, &c),
                    (Some(_), None) => Some(format!("{c}: só no arquivo")),
                    (None, _) => Some(format!("{c}: só no banco")),
                }
            })
        }
        (Value::Array(x), Value::Array(y)) if x.len() == y.len() => x
            .iter()
            .zip(y)
            .enumerate()
            .find_map(|(i, (p, q))| primeira_diferenca(p, q, &format!("{caminho}[{i}]"))),
        (Value::Array(x), Value::Array(y)) => Some(format!(
            "{caminho}: {} itens no arquivo, {} no banco",
            x.len(),
            y.len()
        )),
        // Inteiros comparam exatos (acima de 2^53 o f64 perde precisão); com algum lado
        // fracionário, pelo valor, como no JS (`1` e `1.0` são o mesmo número).
        (Value::Number(x), Value::Number(y)) => {
            let iguais = match (x.as_i64(), y.as_i64(), x.as_u64(), y.as_u64()) {
                (Some(p), Some(q), _, _) => p == q,
                (_, _, Some(p), Some(q)) => p == q,
                _ if x.is_f64() || y.is_f64() => x.as_f64() == y.as_f64(),
                _ => false,
            };
            (!iguais).then(|| format!("{caminho}: {x} ≠ {y}"))
        }
        _ if a == b => None,
        _ => Some(format!("{caminho}: {} ≠ {}", resumo(a), resumo(b))),
    }
}

/// Escalar por extenso; objeto e lista só pelo tipo, para não despejar o arquivo no log.
fn resumo(v: &Value) -> String {
    match v {
        Value::Object(o) => format!("objeto com {} chaves", o.len()),
        Value::Array(a) => format!("lista com {} itens", a.len()),
        outro => outro.to_string(),
    }
}

#[cfg(test)]
mod testes {
    use super::*;
    use serde_json::json;

    #[test]
    fn diferenca_aponta_o_caminho() {
        let a = json!({"x": [1, {"y": 2}], "z": true});
        assert_eq!(primeira_diferenca(&a, &a, ""), None);
        let b = json!({"x": [1, {"y": 3}], "z": true});
        assert_eq!(
            primeira_diferenca(&a, &b, "").as_deref(),
            Some(".x[1].y: 2 ≠ 3")
        );
        let c = json!({"x": [1], "z": true});
        assert!(primeira_diferenca(&a, &c, "").is_some_and(|d| d.starts_with(".x:")));
        assert_eq!(primeira_diferenca(&json!(1), &json!(1.0), ""), None);
        let grande = json!(9_007_199_254_740_993_u64);
        assert!(primeira_diferenca(&grande, &json!(9_007_199_254_740_992_u64), "").is_some());
        let d = primeira_diferenca(&json!({"a": {"x": 1}}), &json!({"a": [1]}), "");
        assert_eq!(
            d.as_deref(),
            Some(".a: objeto com 1 chaves ≠ lista com 1 itens")
        );
    }
}
