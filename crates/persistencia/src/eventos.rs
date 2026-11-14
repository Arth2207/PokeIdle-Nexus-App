//! Eventos brutos do jogo (`POST /ingest`) e configuração de hunt (`POST /api/hunt-sync`).
//!
//! Cada evento ganha uma chave (hash do conteúdo): reenviar o mesmo lote não duplica linha.
//! As queries usam `sqlx::query` (checadas pelos testes de integração), porque as tabelas
//! novas só existem depois da migration.

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};

/// Tipos de evento que o legado conhece (`ev.k`).
pub const TIPOS: &[&str] = &[
    "kill",
    "capturado",
    "drop",
    "bola",
    "welcome",
    "perfil",
    "mercado_aviso",
    "inventario",
    // Captura manual do Tracker pela A2 (1 clique do dono = 1 evento).
    "tracker.captura",
    // Modo Descoberta da A2: mensagem do WS do jogo como veio (segredos removidos no app).
    "ws.bruto",
    // PvP, Área de Treinamento, shiny do servidor e fichas de raid (A2, `competitivo.rs`).
    "pvp.partida",
    "pvp.estado",
    "treino.resultado",
    "shiny.servidor",
    "boss.fichas",
    // Resumo do dia de cada conta (o `D.dia` do motor), mandado pelo app a cada 5 min.
    "diario",
];

/// Campos que não entram na chave: o instante e o tipo já vão separados.
const FORA_DA_CHAVE: &[&str] = &["ts", "k", "recebido_em"];
/// Instante sentinela que o coletor antigo gravava.
const TS_SENTINELA: i64 = 1_234_567_890_000;
/// Limite de sanidade para o instante (ano 2100): `to_timestamp` falharia além disso.
const TS_MAXIMO: i64 = 4_102_444_800_000;

/// Um evento aceito, pronto para gravar.
#[derive(Debug, Clone, PartialEq)]
pub struct Evento {
    pub tipo: String,
    pub em_ms: i64,
    pub chave: Vec<u8>,
    pub dados: Value,
    /// O evento veio sem `ts` válido: gravado com o instante da chegada, então reenviar o
    /// mesmo lote não é idempotente para ele (a chave inclui o instante).
    pub sem_ts: bool,
}

/// O resultado de validar um lote: os aceitos e, por tipo, os erros.
#[derive(Debug, Default, PartialEq)]
pub struct Lote {
    pub eventos: Vec<Evento>,
    /// Tipo de cada evento rejeitado (`"?"` quando o evento nem tem tipo).
    pub erros: Vec<String>,
}

/// O nick como o Nexus guarda: minúsculas, sem espaços nas pontas.
pub fn normalizar(nick: &str) -> String {
    nick.trim().to_lowercase()
}

/// JSON canônico: chaves em ordem, sem espaços e sem os campos fora da chave (no 1º nível).
fn canonico(v: &Value, fora: &[&str], saida: &mut String) {
    match v {
        Value::Object(m) => {
            let mut chaves: Vec<&String> =
                m.keys().filter(|k| !fora.contains(&k.as_str())).collect();
            chaves.sort();
            saida.push('{');
            for (i, k) in chaves.iter().enumerate() {
                if i > 0 {
                    saida.push(',');
                }
                saida.push_str(&Value::String((*k).clone()).to_string());
                saida.push(':');
                if let Some(x) = m.get(k.as_str()) {
                    canonico(x, &[], saida);
                }
            }
            saida.push('}');
        }
        Value::Array(a) => {
            saida.push('[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    saida.push(',');
                }
                canonico(x, &[], saida);
            }
            saida.push(']');
        }
        outro => saida.push_str(&outro.to_string()),
    }
}

/// A chave de idempotência: sha256 de `conta|tipo|instante|json canônico do evento`.
pub fn chave(conta: &str, tipo: &str, em_ms: i64, evento: &Value) -> Vec<u8> {
    let mut texto = format!("{conta}|{tipo}|{em_ms}|");
    canonico(evento, FORA_DA_CHAVE, &mut texto);
    Sha256::digest(texto.as_bytes()).to_vec()
}

/// Valida cada evento do lote. Um evento ruim vira erro e não derruba os outros.
/// `melhorias` liga o descarte do instante sentinela (divergência do legado).
pub fn validar(conta: &str, eventos: &[Value], agora: i64, melhorias: bool) -> Lote {
    let mut lote = Lote::default();
    for ev in eventos {
        let tipo = ev.get("k").and_then(Value::as_str).unwrap_or("?");
        if !ev.is_object() || !TIPOS.contains(&tipo) {
            lote.erros.push(tipo.to_owned());
            continue;
        }
        // `ev.ts || Date.now()`: instante ausente, 0 ou inválido vira "agora".
        let ts = nucleo_ts(ev.get("ts"));
        let sem_ts = ts == 0;
        let em_ms = if sem_ts { agora } else { ts };
        if !(0..=TS_MAXIMO).contains(&em_ms) || (melhorias && em_ms == TS_SENTINELA) {
            lote.erros.push(tipo.to_owned());
            continue;
        }
        lote.eventos.push(Evento {
            tipo: tipo.to_owned(),
            em_ms,
            chave: chave(conta, tipo, em_ms, ev),
            dados: ev.clone(),
            sem_ts,
        });
    }
    lote
}

/// O `ts` como inteiro de ms; ausente ou ilegível dá 0.
fn nucleo_ts(v: Option<&Value>) -> i64 {
    match v {
        Some(Value::Number(n)) => n
            .as_i64()
            .or_else(|| n.as_f64().filter(|f| f.is_finite()).map(|f| f as i64))
            .unwrap_or(0),
        Some(Value::String(s)) => s
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|f| f.is_finite())
            .map_or(0, |f| f as i64),
        _ => 0,
    }
}

/// Grava o lote de uma vez (`UNNEST`), com a `conta` (nick) e o `conta_id` do lote; eventos que já existem são ignorados. Devolve o tipo de
/// cada evento que entrou de fato.
pub async fn gravar(
    db: &PgPool,
    conta: &str,
    conta_id: Option<&str>,
    eventos: &[Evento],
) -> Result<Vec<String>, sqlx::Error> {
    if eventos.is_empty() {
        return Ok(Vec::new());
    }
    let tipos: Vec<&str> = eventos.iter().map(|e| e.tipo.as_str()).collect();
    let ems: Vec<i64> = eventos.iter().map(|e| e.em_ms).collect();
    let chaves: Vec<&[u8]> = eventos.iter().map(|e| e.chave.as_slice()).collect();
    let dados: Vec<&Value> = eventos.iter().map(|e| &e.dados).collect();
    sqlx::query_scalar(
        "INSERT INTO nexus.evento (conta, conta_id, tipo, em, chave, dados)
         SELECT $1, $6, t, to_timestamp(e / 1000.0), c, d
         FROM UNNEST($2::text[], $3::bigint[], $4::bytea[], $5::jsonb[]) AS x(t, e, c, d)
         ON CONFLICT DO NOTHING
         RETURNING tipo",
    )
    .bind(conta)
    .bind(&tipos)
    .bind(&ems)
    .bind(&chaves)
    .bind(&dados)
    .bind(conta_id)
    .fetch_all(db)
    .await
}

/// Grava a hunt atual de cada conta. Devolve quantas foram atualizadas.
pub async fn gravar_hunt(
    db: &PgPool,
    contas: &[(String, String)],
    agora_ms: i64,
) -> Result<u64, sqlx::Error> {
    let mut tx = db.begin().await?;
    let mut n = 0;
    for (conta, slug) in contas {
        let mut dados = Map::new();
        dados.insert("hunt_slug".into(), Value::from(slug.as_str()));
        dados.insert("atualizado".into(), Value::from(agora_ms));
        sqlx::query(
            "INSERT INTO nexus.config_hunt (conta, dados) VALUES ($1, $2)
             ON CONFLICT (conta) DO UPDATE SET dados = EXCLUDED.dados, atualizado_em = now()",
        )
        .bind(conta)
        .bind(Value::Object(dados))
        .execute(&mut *tx)
        .await?;
        n += 1;
    }
    tx.commit().await?;
    Ok(n)
}

/// Filtros do histórico de eventos. Instantes em ms desde 1970 (`de` inclusivo, `ate` exclusivo).
#[derive(Debug, Default, Clone)]
pub struct FiltroHistorico<'a> {
    pub conta: Option<&'a str>,
    pub tipo: Option<&'a str>,
    pub de_ms: Option<i64>,
    pub ate_ms: Option<i64>,
}

/// Um evento gravado.
#[derive(Debug, Clone, PartialEq)]
pub struct EventoGravado {
    pub conta: String,
    pub tipo: String,
    pub em_ms: i64,
    pub dados: Value,
}

/// Eventos brutos, do mais recente para o mais antigo, até `limite` linhas.
pub async fn listar(
    db: &PgPool,
    f: &FiltroHistorico<'_>,
    limite: i64,
) -> Result<Vec<EventoGravado>, sqlx::Error> {
    let linhas = sqlx::query(
        "SELECT conta, tipo, (extract(epoch FROM em) * 1000)::bigint AS em_ms, dados
         FROM nexus.evento
         WHERE ($1::text IS NULL OR conta = $1) AND ($2::text IS NULL OR tipo = $2)
           AND ($3::bigint IS NULL OR em >= to_timestamp($3 / 1000.0))
           AND ($4::bigint IS NULL OR em < to_timestamp($4 / 1000.0))
         ORDER BY em DESC LIMIT $5",
    )
    .bind(f.conta)
    .bind(f.tipo)
    .bind(f.de_ms)
    .bind(f.ate_ms)
    .bind(limite)
    .fetch_all(db)
    .await?;
    linhas
        .iter()
        .map(|l| {
            Ok(EventoGravado {
                conta: l.try_get("conta")?,
                tipo: l.try_get("tipo")?,
                em_ms: l.try_get("em_ms")?,
                dados: l.try_get("dados")?,
            })
        })
        .collect()
}

/// Contagem por hora, conta e tipo. Lê o agregado contínuo `nexus.evento_hora` quando existe
/// (Timescale); sem ele, agrega `nexus.evento` na hora. Devolve `(hora_ms, conta, tipo, total)`.
pub async fn por_hora(
    db: &PgPool,
    f: &FiltroHistorico<'_>,
    limite: i64,
) -> Result<Vec<(i64, String, String, i64)>, sqlx::Error> {
    let existe: bool = sqlx::query_scalar("SELECT to_regclass('nexus.evento_hora') IS NOT NULL")
        .fetch_one(db)
        .await?;
    let sql = if existe {
        "SELECT (extract(epoch FROM hora) * 1000)::bigint, conta, tipo, total::bigint
         FROM nexus.evento_hora
         WHERE ($1::text IS NULL OR conta = $1) AND ($2::text IS NULL OR tipo = $2)
           AND ($3::bigint IS NULL OR hora >= to_timestamp($3 / 1000.0))
           AND ($4::bigint IS NULL OR hora < to_timestamp($4 / 1000.0))
         ORDER BY hora DESC, conta, tipo LIMIT $5"
    } else {
        "SELECT (extract(epoch FROM date_trunc('hour', em)) * 1000)::bigint AS h, conta, tipo,
                count(*)::bigint
         FROM nexus.evento
         WHERE ($1::text IS NULL OR conta = $1) AND ($2::text IS NULL OR tipo = $2)
           AND ($3::bigint IS NULL OR em >= to_timestamp($3 / 1000.0))
           AND ($4::bigint IS NULL OR em < to_timestamp($4 / 1000.0))
         GROUP BY h, conta, tipo ORDER BY h DESC, conta, tipo LIMIT $5"
    };
    sqlx::query_as(sql)
        .bind(f.conta)
        .bind(f.tipo)
        .bind(f.de_ms)
        .bind(f.ate_ms)
        .bind(limite)
        .fetch_all(db)
        .await
}

#[cfg(test)]
mod testes {
    use serde_json::json;

    use super::*;

    #[test]
    fn chave_ignora_ordem_dos_campos_e_o_ts() {
        let a = json!({"k": "kill", "ts": 5, "nome": "x", "xp": 1});
        let b = json!({"xp": 1, "nome": "x", "ts": 99, "k": "kill"});
        assert_eq!(chave("c", "kill", 5, &a), chave("c", "kill", 5, &b));
        assert_ne!(chave("c", "kill", 5, &a), chave("c", "kill", 6, &a));
        assert_ne!(chave("c", "kill", 5, &a), chave("d", "kill", 5, &a));
        let com_seq = json!({"k": "kill", "ts": 5, "nome": "x", "xp": 1, "seq": 7});
        assert_ne!(chave("c", "kill", 5, &a), chave("c", "kill", 5, &com_seq));
    }

    #[test]
    fn validar_separa_aceitos_e_erros() {
        let evs = vec![
            json!({"k": "kill", "ts": 1_700_000_000_000_i64}),
            json!({"k": "inventado"}),
            json!({"semk": 1}),
            json!("texto"),
            json!({"k": "perfil"}),
        ];
        let l = validar("c", &evs, 1_800_000_000_000, false);
        assert_eq!(l.eventos.len(), 2);
        assert_eq!(l.erros, vec!["inventado", "?", "?"]);
        // Sem `ts`, vale o instante da chegada.
        assert_eq!(l.eventos[1].em_ms, 1_800_000_000_000);
    }

    #[test]
    fn sentinela_so_cai_com_melhorias() {
        let evs = vec![json!({"k": "kill", "ts": TS_SENTINELA})];
        assert_eq!(validar("c", &evs, 1, false).eventos.len(), 1);
        assert_eq!(validar("c", &evs, 1, true).erros, vec!["kill"]);
    }

    mod propriedades {
        use proptest::prelude::*;
        use serde_json::{Map, Value};

        use super::super::*;

        /// Campos de negócio (nunca os voláteis `ts`, `k`, `recebido_em`).
        fn campos() -> impl Strategy<Value = Vec<(String, i64)>> {
            proptest::collection::vec(("[a-j]{1,4}", -1000_i64..1000), 0..6)
        }

        fn monta(campos: &[(String, i64)], ts: i64, ordem_inversa: bool) -> Value {
            let mut pares: Vec<_> = campos.to_vec();
            // Chaves repetidas: vale a última, igual para as duas ordens.
            pares.sort_by(|a, b| a.0.cmp(&b.0));
            pares.dedup_by(|b, a| {
                let igual = a.0 == b.0;
                if igual {
                    a.1 = b.1;
                }
                igual
            });
            if ordem_inversa {
                pares.reverse();
            }
            let mut m = Map::new();
            m.insert("k".into(), Value::from("kill"));
            m.insert("ts".into(), Value::from(ts));
            for (k, v) in pares {
                m.insert(k, Value::from(v));
            }
            Value::Object(m)
        }

        proptest! {
            #[test]
            fn mesmo_evento_mesma_chave(c in campos(), em in 0_i64..4_000_000_000_000) {
                let a = monta(&c, em, false);
                prop_assert_eq!(chave("c", "kill", em, &a), chave("c", "kill", em, &a.clone()));
            }

            #[test]
            fn campo_volatil_e_ordem_nao_mudam_a_chave(
                c in campos(),
                em in 0_i64..4_000_000_000_000,
                outro_ts in any::<i64>(),
                recebido in any::<i64>(),
            ) {
                let a = monta(&c, em, false);
                let mut b = monta(&c, outro_ts, true);
                if let Some(o) = b.as_object_mut() {
                    o.insert("recebido_em".into(), Value::from(recebido));
                }
                prop_assert_eq!(chave("c", "kill", em, &a), chave("c", "kill", em, &b));
            }
        }
    }
}
