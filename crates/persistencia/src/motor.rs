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
    #[error("JSON do estado inválido: {0}")]
    Json(serde_json::Error),
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

    // Cada pedaço vai como texto, na ordem do arquivo: `texto` (json) guarda a ordem das
    // chaves, `dados` (jsonb) serve para consultas.
    let texto = |v: &Value| serde_json::to_string(v).map_err(ErroMotor::Json);
    let (mut nicks, mut secoes, mut textos) = (Vec::new(), Vec::new(), Vec::new());
    for (n, s, d) in &linhas_config {
        nicks.push(n.clone());
        secoes.push(s.clone());
        textos.push(texto(d)?);
    }
    let textos_acoes = acoes.iter().map(texto).collect::<Result<Vec<_>, _>>()?;
    let (mut chaves, mut textos_estado) = (Vec::new(), Vec::new());
    for (k, v) in topo {
        if !matches!(k.as_str(), "config" | "acoes") && !CHAVES_FORA.contains(&k.as_str()) {
            chaves.push(k.clone());
            textos_estado.push(texto(v)?);
        }
    }

    let mut tx = db.begin().await?;
    for apagar in [
        "DELETE FROM nexus.motor_config",
        "DELETE FROM nexus.motor_acao",
        "DELETE FROM nexus.motor_estado",
    ] {
        sqlx::query(apagar).execute(&mut *tx).await?;
    }
    let n_config = sqlx::query(
        "INSERT INTO nexus.motor_config (nick, secao, dados, texto, ordem)
         SELECT n, s, t::jsonb, t::json, o::int
         FROM unnest($1::text[], $2::text[], $3::text[]) WITH ORDINALITY AS a(n, s, t, o)",
    )
    .bind(&nicks)
    .bind(&secoes)
    .bind(&textos)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    // O arquivo guarda da mais nova para a mais antiga; o id cresce no sentido do tempo.
    let n_acoes = sqlx::query(
        "INSERT INTO nexus.motor_acao (nick, t, tipo, ok, dados, texto)
         SELECT a->>'nick', (a->>'t')::bigint, a->>'tipo',
                CASE jsonb_typeof(a->'ok') WHEN 'boolean' THEN (a->>'ok')::boolean END,
                a, x.t::json
         FROM unnest($1::text[]) WITH ORDINALITY AS x(t, o)
         CROSS JOIN LATERAL (SELECT x.t::jsonb AS a) j
         ORDER BY o DESC",
    )
    .bind(&textos_acoes)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    let n_estado = sqlx::query(
        "INSERT INTO nexus.motor_estado (chave, dados, texto, ordem)
         SELECT k, t::jsonb, t::json, o::int
         FROM unnest($1::text[], $2::text[]) WITH ORDINALITY AS a(k, t, o)",
    )
    .bind(&chaves)
    .bind(&textos_estado)
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

/// Monta de volta o objeto do `holodex-dados.json` (sem as [`CHAVES_FORA`]), com as chaves
/// na ordem do arquivo. Linhas sem `texto` (importadas antes da ordem) saem na do jsonb.
pub async fn carregar_dados(db: &PgPool) -> Result<Value, ErroMotor> {
    let ler = |t: &str| serde_json::from_str::<Value>(t).map_err(ErroMotor::Json);
    // Três leituras do mesmo instante: uma importação no meio não mistura dois arquivos.
    let mut tx = db.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
        .execute(&mut *tx)
        .await?;
    let linhas: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT nick, secao, coalesce(texto::text, dados::text) FROM nexus.motor_config
         ORDER BY ordem NULLS LAST, nick, secao",
    )
    .fetch_all(&mut *tx)
    .await?;
    let mut config = Map::new();
    for (nick, secao, t) in linhas {
        let conta = config
            .entry(nick)
            .or_insert_with(|| Value::Object(Map::new()));
        if let (false, Value::Object(c)) = (secao.is_empty(), conta) {
            c.insert(secao, ler(&t)?);
        }
    }
    let acoes: Vec<String> = sqlx::query_scalar(
        "SELECT coalesce(texto::text, dados::text) FROM nexus.motor_acao ORDER BY id DESC",
    )
    .fetch_all(&mut *tx)
    .await?;
    let estado: Vec<(String, String)> = sqlx::query_as(
        "SELECT chave, coalesce(texto::text, dados::text) FROM nexus.motor_estado
         ORDER BY ordem NULLS LAST, chave",
    )
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    let mut d = Map::new();
    d.insert("config".into(), Value::Object(config));
    d.insert(
        "acoes".into(),
        Value::Array(acoes.iter().map(|t| ler(t)).collect::<Result<_, _>>()?),
    );
    for (k, t) in estado {
        d.insert(k, ler(&t)?);
    }
    Ok(Value::Object(d))
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

/// Primeiro objeto (abaixo do topo) cujas chaves estão em outra ordem. O legado itera na
/// ordem de inserção, então a ordem faz parte do estado.
pub fn primeira_diferenca_de_ordem(a: &Value, b: &Value, caminho: &str) -> Option<String> {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            if !caminho.is_empty() && !x.keys().eq(y.keys()) {
                return Some(format!("{caminho}: chaves em outra ordem"));
            }
            x.iter().find_map(|(k, p)| {
                y.get(k)
                    .and_then(|q| primeira_diferenca_de_ordem(p, q, &format!("{caminho}.{k}")))
            })
        }
        (Value::Array(x), Value::Array(y)) => {
            x.iter().zip(y).enumerate().find_map(|(i, (p, q))| {
                primeira_diferenca_de_ordem(p, q, &format!("{caminho}[{i}]"))
            })
        }
        _ => None,
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
