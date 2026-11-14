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
        // As 300 mais recentes, como o `D.acoes` do legado (que cortava em 300).
        "SELECT coalesce(texto::text, dados::text) FROM nexus.motor_acao ORDER BY id DESC LIMIT 300",
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

/// Uma decisão do laço em sombra, para `nexus.motor_decisao`.
#[derive(Debug, Clone)]
pub struct LinhaDecisao {
    pub ciclo: i64,
    pub nick: String,
    pub regra: String,
    pub decisao: String,
    pub dados: Value,
}

/// Grava as decisões de um ciclo de uma vez.
pub async fn gravar_decisoes(db: &PgPool, linhas: &[LinhaDecisao]) -> Result<u64, sqlx::Error> {
    if linhas.is_empty() {
        return Ok(0);
    }
    let ciclos: Vec<i64> = linhas.iter().map(|l| l.ciclo).collect();
    let nicks: Vec<&str> = linhas.iter().map(|l| l.nick.as_str()).collect();
    let regras: Vec<&str> = linhas.iter().map(|l| l.regra.as_str()).collect();
    let decisoes: Vec<&str> = linhas.iter().map(|l| l.decisao.as_str()).collect();
    let dados = Value::Array(linhas.iter().map(|l| l.dados.clone()).collect());
    Ok(sqlx::query(
        "INSERT INTO nexus.motor_decisao (ciclo, nick, regra, decisao, dados)
         SELECT c, n, r, k, d
         FROM unnest($1::bigint[], $2::text[], $3::text[], $4::text[]) WITH ORDINALITY AS a(c, n, r, k, o)
         JOIN jsonb_array_elements($5) WITH ORDINALITY AS b(d, o2) ON o = o2",
    )
    .bind(&ciclos)
    .bind(&nicks)
    .bind(&regras)
    .bind(&decisoes)
    .bind(dados)
    .execute(db)
    .await?
    .rows_affected())
}

/// Decisões gravadas desde `desde_ms` (instante do ciclo), da mais antiga para a mais nova:
/// `(ciclo, nick, regra, dados)`.
pub async fn decisoes_desde(
    db: &PgPool,
    desde_ms: i64,
) -> Result<Vec<(i64, String, String, Value)>, sqlx::Error> {
    sqlx::query_as(
        "SELECT ciclo, nick, regra, dados FROM nexus.motor_decisao WHERE ciclo >= $1
         ORDER BY ciclo, id",
    )
    .bind(desde_ms)
    .fetch_all(db)
    .await
}

/// Com `NEXUS_ESCRITA_MOTOR`: põe na fila da fase 3 os comandos do ciclo `ciclo` (ms),
/// `(conta, tipo, params)`, com origem `motor`. `expira_ms` é um instante absoluto (ms desde
/// 1970): um comando de ciclo velho não vale mais. A chave de idempotência (ciclo, conta,
/// posição) evita duplicar se o mesmo ciclo for enfileirado de novo depois de uma falha no
/// meio. Devolve os ids.
pub async fn enfileirar(
    db: &PgPool,
    ciclo: i64,
    comandos: &[(String, String, Value)],
    expira_ms: i64,
) -> Result<Vec<i64>, sqlx::Error> {
    let mut ids = Vec::with_capacity(comandos.len());
    for (i, (conta, tipo, params)) in comandos.iter().enumerate() {
        let novo = crate::comandos::NovoComando {
            conta,
            tipo,
            params,
            prioridade: 5,
            origem: "motor",
            expira_ms,
        };
        let chave = format!("motor:{ciclo}:{i}");
        ids.push(
            crate::comandos::inserir_ext(db, &novo, Some(&chave), None)
                .await?
                .0,
        );
    }
    Ok(ids)
}

/// Retenção: apaga decisões com mais de `dias_decisao` e ações do legado com mais de
/// `dias_acao` (pelo instante do ciclo / da ação). Devolve (decisões, ações) apagadas.
pub async fn podar_sombra(
    db: &PgPool,
    agora_ms: i64,
    dias_decisao: i64,
    dias_acao: i64,
) -> Result<(u64, u64), sqlx::Error> {
    let dia = 86_400_000;
    let d = sqlx::query("DELETE FROM nexus.motor_decisao WHERE ciclo < $1")
        .bind(agora_ms - dias_decisao * dia)
        .execute(db)
        .await?
        .rows_affected();
    let a = sqlx::query("DELETE FROM nexus.motor_acao_legado WHERE t < $1")
        .bind(agora_ms - dias_acao * dia)
        .execute(db)
        .await?
        .rows_affected();
    Ok((d, a))
}

/// Guarda as ações do legado ainda não vistas (`acoes` do `D` de um ciclo). Devolve quantas
/// eram novas.
pub async fn gravar_acoes_legado(db: &PgPool, acoes: &[Value]) -> Result<u64, sqlx::Error> {
    let validas: Vec<Value> = acoes
        .iter()
        .filter(|a| a.get("t").and_then(Value::as_i64).is_some())
        .cloned()
        .collect();
    if validas.is_empty() {
        return Ok(0);
    }
    Ok(sqlx::query(
        "INSERT INTO nexus.motor_acao_legado (t, nick, tipo, texto, ok, dados)
         SELECT (a->>'t')::bigint, coalesce(a->>'nick', ''), coalesce(a->>'tipo', ''),
                coalesce(a->>'texto', ''),
                CASE jsonb_typeof(a->'ok') WHEN 'boolean' THEN (a->>'ok')::boolean END, a
         FROM jsonb_array_elements($1) AS x(a)
         ON CONFLICT DO NOTHING",
    )
    .bind(Value::Array(validas))
    .execute(db)
    .await?
    .rows_affected())
}

/// Ações do legado guardadas desde `desde_ms`: `(t, nick, tipo, texto)`, por tempo.
pub async fn acoes_legado_desde(
    db: &PgPool,
    desde_ms: i64,
) -> Result<Vec<(i64, String, String, String)>, sqlx::Error> {
    sqlx::query_as(
        "SELECT t, nick, tipo, texto FROM nexus.motor_acao_legado WHERE t >= $1 ORDER BY t",
    )
    .bind(desde_ms)
    .fetch_all(db)
    .await
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

/// Guarda a janela de uma conta (Fase 4). Se ela trouxer o login (`welcome`), ele vai para a
/// coluna própria; se não, o login guardado continua valendo.
pub async fn gravar_janela(db: &PgPool, conta: &str, janela: &Value) -> Result<(), sqlx::Error> {
    let mut j = janela.clone();
    let login = j.as_object_mut().and_then(|m| m.remove("welcome"));
    sqlx::query(
        "INSERT INTO nexus.motor_janela (conta, em, janela, login, login_em)
         VALUES ($1, now(), $2, $3, CASE WHEN $3::jsonb IS NULL THEN NULL ELSE now() END)
         ON CONFLICT (conta) DO UPDATE SET
           em = now(), janela = EXCLUDED.janela,
           login = coalesce(EXCLUDED.login, nexus.motor_janela.login),
           login_em = coalesce(EXCLUDED.login_em, nexus.motor_janela.login_em)",
    )
    .bind(conta)
    .bind(&j)
    .bind(login)
    .execute(db)
    .await?;
    Ok(())
}

/// Janelas recebidas nos últimos `max_s` segundos, com o login de volta em `welcome`.
pub async fn janelas_frescas(db: &PgPool, max_s: i64) -> Result<Vec<Value>, sqlx::Error> {
    let linhas: Vec<(Value, Option<Value>)> = sqlx::query_as(
        "SELECT janela, login FROM nexus.motor_janela
          WHERE em > now() - make_interval(secs => $1::float8)
          ORDER BY conta",
    )
    .bind(max_s as f64)
    .fetch_all(db)
    .await?;
    Ok(linhas
        .into_iter()
        .map(|(mut j, login)| {
            if let (Some(m), Some(l)) = (j.as_object_mut(), login) {
                m.insert("welcome".into(), l);
            }
            j
        })
        .collect())
}

/// Estado do laço do motor guardado (marcas, alertas, última hunt), se houver.
pub async fn carregar_laco(db: &PgPool) -> Result<Option<Value>, sqlx::Error> {
    sqlx::query_scalar("SELECT dados FROM nexus.motor_laco WHERE id = 1")
        .fetch_optional(db)
        .await
}

pub async fn gravar_laco(db: &PgPool, dados: &Value) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO nexus.motor_laco (id, em, dados) VALUES (1, now(), $1)
         ON CONFLICT (id) DO UPDATE SET em = now(), dados = EXCLUDED.dados",
    )
    .bind(dados)
    .execute(db)
    .await?;
    Ok(())
}

/// Ações do motor do Nexus (resultado dos comandos) no `D.acoes`: `{t, nick, tipo, texto, ok,
/// custo?}`, como o legado gravava.
pub async fn gravar_acoes(db: &PgPool, acoes: &[Value]) -> Result<u64, sqlx::Error> {
    let mut n = 0;
    for a in acoes {
        n += sqlx::query(
            "INSERT INTO nexus.motor_acao (nick, t, tipo, ok, dados)
             VALUES ($1::jsonb->>'nick', ($1::jsonb->>'t')::bigint, $1::jsonb->>'tipo',
                     ($1::jsonb->>'ok')::boolean, $1::jsonb)",
        )
        .bind(a)
        .execute(db)
        .await?
        .rows_affected();
    }
    Ok(n)
}

/// Resultado dos comandos já executados entre `ids`: `(id, resultado)`.
pub async fn resultados_comandos(
    db: &PgPool,
    ids: &[i64],
) -> Result<Vec<(i64, String)>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, coalesce(resultado, '') FROM nexus.comando
          WHERE id = ANY($1) AND executado_em IS NOT NULL",
    )
    .bind(ids)
    .fetch_all(db)
    .await
}

/// Contas do app: `{conta, nick, em}` (última janela recebida), da mais recente.
pub async fn contas_do_app(db: &PgPool) -> Result<Vec<Value>, sqlx::Error> {
    let linhas: Vec<(String, Option<String>, i64)> = sqlx::query_as(
        "SELECT conta, janela->>'nick', (extract(epoch FROM em) * 1000)::bigint
           FROM nexus.motor_janela ORDER BY conta",
    )
    .fetch_all(db)
    .await?;
    Ok(linhas
        .into_iter()
        .map(|(conta, nick, em)| serde_json::json!({"conta": conta, "nick": nick, "em": em}))
        .collect())
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
