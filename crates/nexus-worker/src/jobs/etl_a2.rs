//! ETL do app (A2): abates de `nexus.evento` → `fato_kill` (`source = 'pc'`: o app roda no PC).
//!
//! É o "SQLite → PG" da Fase 7: as contas que rodam no app não passam mais pelo `mapa.db`;
//! os abates delas chegam pelo WebSocket e ficam em `nexus.evento`. Mesmo esquema do ETL do
//! SQLite: cursor por conta em `nexus.etl_cursor` (fonte `nexus.evento`), atualizado na mesma
//! transação do INSERT; a chave única `(ts_at, ts_ms, conta_id)` evita duplicar.
//!
//! Uma conta roda em um app só por vez (o jogo derruba a outra sessão), então as duas fontes
//! não se sobrepõem. A hunt vem do próprio evento ou, se faltar, da última hunt conhecida da
//! conta (`nexus.config_hunt`, que o app atualiza pelo `/api/hunt-sync`).

use anyhow::Result;
use serde_json::Value;
use sqlx::PgPool;
use tracing::{info, warn};

use super::etl_continuo::{
    garantir_dim_hunt, garantir_sessao, inserir_e_avancar_de, Conta, Evento, Fonte, CONTAS,
};

const BATCH: i64 = 5_000;

pub(crate) const FONTE_A2: Fonte = Fonte {
    cursor: "nexus.evento",
    source: "pc",
};

/// Nick (minúsculo, como o app grava em `nexus.evento.conta`) de cada conta do ETL.
/// shortcut: cópia de `persistencia::cacada::CONTAS_A2` (o worker não depende de persistencia); conta nova muda as duas.
const NICKS: &[(&str, &str)] = &[
    ("mvxorqh6", "alllphha"),
    ("mvxoui28", "sikien"),
    ("mvxoxith", "alllphha2"),
    ("mw002fcf", "alllphha3"),
];

fn nick_de(conta_id: &str) -> Option<&'static str> {
    NICKS
        .iter()
        .find(|(id, _)| *id == conta_id)
        .map(|(_, n)| *n)
}

/// Número positivo do JSON (0 vira `None`, como no ETL do SQLite).
fn num(d: &Value, k: &str) -> Option<i64> {
    d.get(k)
        .and_then(|v| v.as_f64())
        .map(|x| x.round() as i64)
        .filter(|x| *x != 0)
}

fn verdade(d: &Value, k: &str) -> bool {
    match d.get(k) {
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|x| x != 0.0),
        _ => false,
    }
}

/// Converte o `dados` de um evento `kill` do app.
pub(crate) fn evento_de(ts_ms: i64, d: &Value, hunt_padrao: Option<&str>) -> Evento {
    let hunt = d
        .get("hunt_slug")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .or(hunt_padrao)
        .map(str::to_owned);
    Evento {
        ts_ms,
        hunt_slug: hunt,
        xp_treinador: num(d, "xp_treinador"),
        xp_pokemon: num(d, "xp_pokemon"),
        ouro: num(d, "ouro"),
        ouro_loot_bonus: num(d, "ouro_loot_bonus"),
        loot_boost: verdade(d, "loot_boost") || num(d, "ouro_loot_bonus").is_some(),
        shiny: verdade(d, "shiny"),
    }
}

/// Abates lidos do HUD (com `seq`) valem só quando o WebSocket não trouxe o mesmo abate: o
/// app manda os dois enquanto o WS ainda não deu sinal. Tira os do HUD a menos de 5 s de um
/// abate do WS.
fn sem_repetidos(linhas: &[(i64, Value)]) -> impl Iterator<Item = &(i64, Value)> {
    let ws: Vec<i64> = linhas
        .iter()
        .filter(|(_, d)| d.get("seq").is_none())
        .map(|(t, _)| *t)
        .collect();
    linhas
        .iter()
        .filter(move |(t, d)| d.get("seq").is_none() || !ws.iter().any(|w| (w - t).abs() < 5_000))
}

pub async fn ciclo(db: &PgPool, escrita: bool) -> Result<u64> {
    let mut total = 0;
    for conta in CONTAS {
        let Some(nick) = nick_de(conta.conta_id) else {
            continue;
        };
        match processar(db, conta, nick, escrita).await {
            Ok(n) => total += n,
            Err(e) => warn!(conta = conta.conta_id, erro = %e, "etl_a2: erro no ciclo da conta"),
        }
    }
    Ok(total)
}

async fn processar(db: &PgPool, conta: &Conta, nick: &str, escrita: bool) -> Result<u64> {
    let cursor: i64 =
        sqlx::query_scalar("SELECT ts_ms FROM nexus.etl_cursor WHERE conta_id = $1 AND fonte = $2")
            .bind(conta.conta_id)
            .bind(FONTE_A2.cursor)
            .fetch_optional(db)
            .await?
            .unwrap_or(0);
    // O app grava a hunt pelo nick do jogo (Alllphhha), não pelo rótulo da conta (alllphha).
    let hunt: Option<String> = sqlx::query_scalar(
        "SELECT dados->>'hunt_slug' FROM nexus.config_hunt
          WHERE conta = $1
             OR conta = (SELECT lower(janela->>'nick') FROM nexus.motor_janela WHERE conta = $1)
          ORDER BY (conta = $1) DESC, atualizado_em DESC LIMIT 1",
    )
    .bind(nick)
    .fetch_optional(db)
    .await?
    .flatten();
    let linhas: Vec<(i64, Value)> = sqlx::query_as(
        "SELECT (extract(epoch FROM em) * 1000)::bigint, dados
           FROM nexus.evento
          WHERE conta = $1 AND tipo = 'kill' AND em > to_timestamp($2::float8 / 1000)
          ORDER BY em
          LIMIT $3",
    )
    .bind(nick)
    .bind(cursor as f64)
    .bind(BATCH)
    .fetch_all(db)
    .await?;
    if linhas.is_empty() {
        return Ok(0);
    }
    let eventos: Vec<Evento> = sem_repetidos(&linhas)
        .map(|(ts, d)| evento_de(*ts, d, hunt.as_deref()))
        .collect();
    if eventos.is_empty() {
        return Ok(0);
    }
    garantir_sessao(db, conta, escrita).await?;
    let slugs: Vec<&str> = eventos
        .iter()
        .filter_map(|e| e.hunt_slug.as_deref())
        .collect();
    garantir_dim_hunt(db, &slugs, escrita).await?;
    let novo = eventos.last().map_or(cursor, |e| e.ts_ms);
    let n = inserir_e_avancar_de(db, conta, &eventos, novo, escrita, FONTE_A2).await?;
    info!(
        conta = conta.conta_id,
        lidos = eventos.len(),
        inseridos = n,
        "etl_a2: ciclo ok"
    );
    Ok(n)
}

#[cfg(test)]
mod testes {
    use super::*;
    use serde_json::json;

    #[test]
    fn converte_kill_do_app() {
        let d = json!({"k": "kill", "ouro": 850, "xp_pokemon": 32062.0, "xp_treinador": 32062,
                       "ouro_loot_bonus": 120, "shiny": 0, "hunt_slug": null});
        let e = evento_de(5, &d, Some("furious_scizor"));
        assert_eq!(e.hunt_slug.as_deref(), Some("furious_scizor"));
        assert_eq!(
            (e.ouro, e.xp_pokemon, e.ouro_loot_bonus),
            (Some(850), Some(32062), Some(120))
        );
        assert!(e.loot_boost && !e.shiny);
        let e = evento_de(
            5,
            &json!({"hunt_slug": "x", "ouro": 0, "shiny": true}),
            Some("y"),
        );
        assert_eq!(
            (e.hunt_slug.as_deref(), e.ouro, e.shiny),
            (Some("x"), None, true)
        );
        assert_eq!(nick_de("mvxoui28"), Some("sikien"));
        let l = vec![
            (1_000, json!({"seq": 1})),
            (2_000, json!({})),
            (60_000, json!({"seq": 2})),
        ];
        let ts: Vec<i64> = sem_repetidos(&l).map(|(t, _)| *t).collect();
        assert_eq!(ts, vec![2_000, 60_000]);
    }
}
