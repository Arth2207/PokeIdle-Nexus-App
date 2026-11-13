//! ETL contínuo: SQLite `evento_kill` → Postgres `fato_kill` (ou shadow).
//!
//! Diferença chave em relação ao legado (`etl-continuo.js`):
//! o cursor é atualizado **na mesma transação** do INSERT. Se o processo cair
//! no meio, o cursor não avança — o próximo ciclo reprocesa o mesmo intervalo
//! sem duplicar (a chave única `(ts_at, ts_ms, conta_id)` garante idempotência).
//!
//! Em JS: equivale a fazer `BEGIN; INSERT …; UPDATE cursor; COMMIT` em vez de
//! salvar um arquivo JSON depois do INSERT. Se a energia cair no meio do commit,
//! o banco desfaz tudo e o arquivo fica como estava.

use anyhow::{Context, Result};
use sqlx::PgPool;
use tokio::task::spawn_blocking;
use tracing::{info, warn};

/// Conta com seu cursor de ETL.
#[derive(Debug, Clone)]
struct Conta {
    conta_id: &'static str,
    sessao_etl: i32,
}

const CONTAS: &[Conta] = &[
    Conta {
        conta_id: "mvxorqh6",
        sessao_etl: 100,
    },
    Conta {
        conta_id: "mvxoui28",
        sessao_etl: 101,
    },
    Conta {
        conta_id: "mvxoxith",
        sessao_etl: 102,
    },
    Conta {
        conta_id: "mw002fcf",
        sessao_etl: 103,
    },
];

const BATCH: usize = 5_000;
/// Timestamp sentinela do legado — descartado.
const SENTINELA_MS: i64 = 1_234_567_890_000;

/// Linha lida do SQLite.
#[derive(Debug)]
struct Evento {
    ts_ms: i64,
    hunt_slug: Option<String>,
    xp_treinador: Option<i64>,
    xp_pokemon: Option<i64>,
    ouro: Option<i64>,
    ouro_loot_bonus: Option<i64>,
    loot_boost: bool,
    shiny: bool,
}

/// Roda um ciclo completo do ETL para todas as contas.
/// Retorna o total de linhas inseridas (novas).
pub async fn ciclo(db: &PgPool, sqlite_path: &str, escrita: bool) -> Result<u64> {
    let mut total_inseridos = 0u64;

    for conta in CONTAS {
        match processar_conta(db, sqlite_path, conta, escrita).await {
            Ok(n) => {
                total_inseridos += n;
                metrics::counter!(
                    "nexus_worker_etl_inseridos_total",
                    "conta" => conta.conta_id
                )
                .increment(n);
            }
            Err(e) => {
                warn!(conta = conta.conta_id, erro = %e, "etl_continuo: erro no ciclo da conta");
                metrics::counter!(
                    "nexus_worker_etl_erros_total",
                    "conta" => conta.conta_id
                )
                .increment(1);
            }
        }
    }

    Ok(total_inseridos)
}

async fn processar_conta(
    db: &PgPool,
    sqlite_path: &str,
    conta: &Conta,
    escrita: bool,
) -> Result<u64> {
    // 1. Lê cursor atual do Postgres (transacional, não do arquivo)
    let cursor_ms = ler_cursor(db, conta.conta_id).await?;
    // Atraso do cursor: se só cresce, o ETL está travado (alerta no Grafana).
    metrics::gauge!("nexus_worker_etl_cursor_atraso_s", "conta" => conta.conta_id)
        .set((chrono_now_ms() - cursor_ms) as f64 / 1000.0);

    // 2. Lê batch do SQLite em thread bloqueante
    //    spawn_blocking: como o rayon + oneshot que você já viu — move a operação
    //    síncrona para uma thread separada, libera o tokio para outras tarefas.
    let path = sqlite_path.to_owned();
    let conta_id_owned = conta.conta_id.to_owned();
    let eventos =
        spawn_blocking(move || ler_sqlite_batch(&path, &conta_id_owned, cursor_ms, BATCH))
            .await
            .context("thread SQLite falhou")??;

    if eventos.is_empty() {
        return Ok(0);
    }

    // 3. Garante sessão ETL e dim_hunt (fora da transação principal — idempotente)
    garantir_sessao(db, conta, escrita).await?;
    let slugs: Vec<&str> = eventos
        .iter()
        .filter_map(|e| e.hunt_slug.as_deref())
        .collect();
    garantir_dim_hunt(db, &slugs, escrita).await?;

    // 4. INSERT + atualiza cursor — tudo na mesma transação
    let novo_cursor = eventos.last().map(|e| e.ts_ms).unwrap_or(cursor_ms);
    let inseridos = inserir_e_avançar(db, conta, &eventos, novo_cursor, escrita).await?;

    info!(
        conta = conta.conta_id,
        lidos = eventos.len(),
        inseridos,
        cursor_ms = novo_cursor,
        "etl_continuo: ciclo ok"
    );

    Ok(inseridos)
}

/// Lê o cursor salvo no Postgres. Retorna 0 se ainda não existe.
async fn ler_cursor(db: &PgPool, conta_id: &str) -> Result<i64> {
    let row: Option<(i64,)> = sqlx::query_as(
        "SELECT ts_ms FROM nexus.etl_cursor WHERE conta_id = $1 AND fonte = 'mapa.db'",
    )
    .bind(conta_id)
    .fetch_optional(db)
    .await?;

    Ok(row.map(|(ts,)| ts).unwrap_or(0))
}

/// Lê até `batch` linhas do SQLite para a conta a partir do cursor.
fn ler_sqlite_batch(
    path: &str,
    conta_id: &str,
    cursor_ms: i64,
    batch: usize,
) -> Result<Vec<Evento>> {
    // mode=ro: lê o arquivo principal **e o WAL** (o legado grava em modo WAL). Com o diretório
    // inteiro montado só leitura (`-wal` e `-shm` presentes), o SQLite ≥ 3.22 abre o WAL
    // read-only mantendo o índice do WAL na memória do processo. Antes usávamos
    // `immutable=1`, que só via o arquivo principal: o dado chegava atrasado até o checkpoint.
    let uri = format!("file:{}?mode=ro", path);
    let conn = rusqlite::Connection::open_with_flags(
        uri,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY
            | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX
            | rusqlite::OpenFlags::SQLITE_OPEN_URI,
    )
    .context("não consegui abrir o SQLite")?;

    let mut stmt = conn.prepare(
        "SELECT ts, hunt_slug, xp_treinador, xp_pokemon, ouro, ouro_loot_bonus, loot_boost, shiny
           FROM evento_kill
          WHERE conta = ?1 AND ts > ?2
          ORDER BY ts ASC
          LIMIT ?3",
    )?;

    // `loot_boost`/`shiny` podem ser NULL (≈99% das linhas recentes). O legado faz
    // `r.loot_boost ? true : false` → NULL vira false; aqui `Option` + `unwrap_or(0)`.
    // Erro de leitura derruba o ciclo (sem avançar o cursor) em vez de pular a linha:
    // antes, `filter_map(|r| r.ok())` descartava as linhas com NULL e o ETL travava.
    let eventos = stmt
        .query_map(
            rusqlite::params![conta_id, cursor_ms, batch as i64],
            |row| {
                // O legado grava `valor || null`: 0 e '' viram NULL. `filter` faz o mesmo
                // (em JS: `Some(0).filter(v => v !== 0)` → None).
                let num = |i: usize| -> rusqlite::Result<Option<i64>> {
                    Ok(row.get::<_, Option<i64>>(i)?.filter(|v| *v != 0))
                };
                Ok(Evento {
                    ts_ms: row.get(0)?,
                    hunt_slug: row.get::<_, Option<String>>(1)?.filter(|s| !s.is_empty()),
                    xp_treinador: num(2)?,
                    xp_pokemon: num(3)?,
                    ouro: num(4)?,
                    ouro_loot_bonus: num(5)?,
                    loot_boost: row.get::<_, Option<i64>>(6)?.unwrap_or(0) != 0,
                    shiny: row.get::<_, Option<i64>>(7)?.unwrap_or(0) != 0,
                })
            },
        )?
        .collect::<rusqlite::Result<Vec<Evento>>>()
        .context("linha inválida no evento_kill")?
        .into_iter()
        .filter(|e| e.ts_ms != SENTINELA_MS)
        .collect();

    Ok(eventos)
}

/// Garante que a sessão ETL da conta existe.
async fn garantir_sessao(db: &PgPool, conta: &Conta, escrita: bool) -> Result<()> {
    let agora_ms = chrono_now_ms();
    let tabela = if escrita {
        "fato_sessao_contexto"
    } else {
        // Em shadow não há tabela de sessão no nexus; pula
        return Ok(());
    };
    let sql = format!(
        "INSERT INTO public.{tabela} (sessao_id, conta_id, ts_inicio, source)
         VALUES ($1, $2, $3, 'vps')
         ON CONFLICT (sessao_id, conta_id, source) DO NOTHING"
    );
    // SAFETY: tabela é literal estático ('fato_sessao_contexto'), sem entrada do usuário.
    sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(conta.sessao_etl)
        .bind(conta.conta_id)
        .bind(agora_ms)
        .execute(db)
        .await?;

    Ok(())
}

/// Insere hunt_slugs novos em dim_hunt (somente em modo escrita).
async fn garantir_dim_hunt(db: &PgPool, slugs: &[&str], escrita: bool) -> Result<()> {
    if slugs.is_empty() || !escrita {
        return Ok(());
    }
    let unicos: Vec<&str> = {
        let mut v = slugs.to_vec();
        v.sort_unstable();
        v.dedup();
        v
    };
    sqlx::query(
        "INSERT INTO public.dim_hunt (hunt_slug, nome_display)
         SELECT s, initcap(replace(s, '_', ' '))
           FROM unnest($1::text[]) AS s
         ON CONFLICT (hunt_slug) DO NOTHING",
    )
    .bind(&unicos)
    .execute(db)
    .await?;

    Ok(())
}

/// Insere o batch e atualiza o cursor na mesma transação.
async fn inserir_e_avançar(
    db: &PgPool,
    conta: &Conta,
    eventos: &[Evento],
    novo_cursor: i64,
    escrita: bool,
) -> Result<u64> {
    let tabela = if escrita {
        "public.fato_kill"
    } else {
        "nexus.fato_kill_shadow"
    };

    let mut tx = db.begin().await?;

    // INSERT em lote com UNNEST — uma única query, sem montar VALUES dinamicamente
    // UNNEST($1::timestamptz[], $2::bigint[], …): desempacota arrays em linhas.
    // É o equivalente ao VALUES (…),(…) do legado mas tipado e sem injeção de SQL.
    let len = eventos.len();
    let mut ts_at_vec: Vec<f64> = Vec::with_capacity(len);
    let mut ts_ms_vec: Vec<i64> = Vec::with_capacity(len);
    let mut sessao_vec: Vec<i32> = Vec::with_capacity(len);
    let mut hunt_vec: Vec<Option<String>> = Vec::with_capacity(len);
    let mut xp_tr_vec: Vec<Option<i64>> = Vec::with_capacity(len);
    let mut xp_pk_vec: Vec<Option<i64>> = Vec::with_capacity(len);
    let mut ouro_vec: Vec<Option<i64>> = Vec::with_capacity(len);
    let mut ouro_lb_vec: Vec<Option<i64>> = Vec::with_capacity(len);
    let mut loot_vec: Vec<bool> = Vec::with_capacity(len);
    let mut shiny_vec: Vec<bool> = Vec::with_capacity(len);

    for e in eventos {
        ts_at_vec.push(e.ts_ms as f64 / 1000.0);
        ts_ms_vec.push(e.ts_ms);
        sessao_vec.push(conta.sessao_etl);
        hunt_vec.push(e.hunt_slug.clone());
        xp_tr_vec.push(e.xp_treinador);
        xp_pk_vec.push(e.xp_pokemon);
        ouro_vec.push(e.ouro);
        ouro_lb_vec.push(e.ouro_loot_bonus);
        loot_vec.push(e.loot_boost);
        shiny_vec.push(e.shiny);
    }

    let sql = format!(
        "INSERT INTO {tabela}
           (ts_at, ts_ms, conta_id, sessao_id, hunt_slug,
            xp_treinador, xp_pokemon, ouro, ouro_loot_bonus, loot_boost, shiny, source)
         SELECT
           to_timestamp(t), ms, $1, s, h, xt, xp, o, olb, lb, sh, 'vps'
         FROM UNNEST(
           $2::float8[], $3::bigint[], $4::int[], $5::text[],
           $6::bigint[], $7::bigint[], $8::bigint[], $9::bigint[],
           $10::bool[], $11::bool[]
         ) AS u(t, ms, s, h, xt, xp, o, olb, lb, sh)
         ON CONFLICT (ts_at, ts_ms, conta_id) DO NOTHING"
    );
    // SAFETY: tabela é 'public.fato_kill' ou 'nexus.fato_kill_shadow' — sem entrada do usuário.
    let result = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(conta.conta_id)
        .bind(&ts_at_vec)
        .bind(&ts_ms_vec)
        .bind(&sessao_vec)
        .bind(&hunt_vec)
        .bind(&xp_tr_vec)
        .bind(&xp_pk_vec)
        .bind(&ouro_vec)
        .bind(&ouro_lb_vec)
        .bind(&loot_vec)
        .bind(&shiny_vec)
        .execute(&mut *tx)
        .await
        .context("INSERT fato_kill falhou")?;

    let inseridos = result.rows_affected();

    // Atualiza o cursor na mesma transação — se o commit falhar, nada avança
    sqlx::query(
        "INSERT INTO nexus.etl_cursor (conta_id, fonte, ts_ms, atualizado_em)
         VALUES ($1, 'mapa.db', $2, NOW())
         ON CONFLICT (conta_id, fonte) DO UPDATE
           SET ts_ms = EXCLUDED.ts_ms, atualizado_em = NOW()",
    )
    .bind(conta.conta_id)
    .bind(novo_cursor)
    .execute(&mut *tx)
    .await
    .context("UPDATE etl_cursor falhou")?;

    tx.commit()
        .await
        .context("commit da transação ETL falhou")?;

    Ok(inseridos)
}

fn chrono_now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod testes {
    use super::*;

    /// Regressão: `loot_boost`/`shiny` NULL não podem sumir do lote (travava o cursor).
    #[test]
    fn le_linhas_com_loot_boost_nulo() {
        let path = std::env::temp_dir().join(format!("etl-teste-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE evento_kill (conta TEXT, ts INTEGER, hunt_slug TEXT,
                xp_treinador INTEGER, xp_pokemon INTEGER, ouro INTEGER,
                ouro_loot_bonus INTEGER, loot_boost INTEGER, shiny INTEGER);
             INSERT INTO evento_kill VALUES
                ('c', 1234567890000, NULL, 1, 1, 1, NULL, NULL, 0),
                ('c', 10, '', 5, 0, 7, NULL, NULL, 0),
                ('c', 20, 'h', 5, NULL, 7, 2, 1, NULL),
                ('x', 30, 'h', 5, 6, 7, NULL, NULL, 0);",
        )
        .unwrap();
        drop(conn);

        let ev = ler_sqlite_batch(path.to_str().unwrap(), "c", 0, 10).unwrap();
        let _ = std::fs::remove_file(&path);

        assert_eq!(ev.len(), 2);
        assert_eq!(ev[0].ts_ms, 10);
        assert!(!ev[0].loot_boost);
        assert!(ev[1].loot_boost);
        assert!(!ev[1].shiny);
        assert_eq!(ev[1].xp_pokemon, None);
        // `valor || null` do legado: '' e 0 viram NULL
        assert_eq!(ev[0].hunt_slug, None);
        assert_eq!(ev[0].xp_pokemon, None);
        assert_eq!(ev[0].xp_treinador, Some(5));
    }
}
