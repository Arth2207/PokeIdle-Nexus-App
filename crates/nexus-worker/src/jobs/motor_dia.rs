//! Diário do motor a partir do app: os eventos `diario` (o `D.dia` de cada conta, montado no
//! app com as mesmas regras do motor do legado) atualizam `nexus.motor_estado` (`dia` e
//! `saldo`). Assim Finanças, Caçada e o histórico seguem vivos para as contas que rodam no
//! app, sem o motor do PC.
//!
//! Só os dias que o app mandou mudam; `compras` e `capturas` do legado são mantidas.

use anyhow::Result;
use serde_json::{json, Map, Value};
use sqlx::PgPool;
use tracing::info;

/// Campos do dia que vêm do app.
const CAMPOS_DIA: [&str; 10] = [
    "kills", "bolas", "capt", "shiny", "pot", "desm", "ouro", "xp", "xpPk", "drops",
];

fn objeto<'a>(v: &'a mut Value, k: &str) -> Option<&'a mut Map<String, Value>> {
    let m = v.as_object_mut()?;
    m.entry(k).or_insert_with(|| json!({})).as_object_mut()
}

/// Aplica os eventos `(nick, dia, dados)` nos objetos `dia` e `saldo` do motor. Devolve
/// quantos dias mudaram.
pub(crate) fn aplicar(
    dia: &mut Value,
    saldo: &mut Value,
    eventos: &[(String, String, Value)],
) -> usize {
    if !dia.is_object() {
        *dia = json!({});
    }
    if !saldo.is_object() {
        *saldo = json!({});
    }
    let mut n = 0;
    for (nick, d, ev) in eventos {
        let Some(por) = objeto(dia, nick) else {
            continue;
        };
        let reg = por.entry(d.clone()).or_insert_with(|| json!({}));
        let Some(reg) = reg.as_object_mut() else {
            continue;
        };
        let antes = Value::Object(reg.clone());
        for k in CAMPOS_DIA {
            if let Some(x) = ev.get(k) {
                reg.insert(k.into(), x.clone());
            }
        }
        reg.entry("compras").or_insert(json!(0));
        reg.entry("capturas").or_insert(json!([]));
        if Value::Object(reg.clone()) != antes {
            n += 1;
        }
        if let (Some(s), Some(por)) = (ev.get("saldo"), objeto(saldo, nick)) {
            let r = por.entry(d.clone()).or_insert_with(|| json!({}));
            if let Some(r) = r.as_object_mut() {
                for k in ["ini", "fim", "min", "max", "tIni", "tFim"] {
                    if let Some(x) = s.get(k) {
                        // O início do dia não muda depois de gravado.
                        if (k == "ini" || k == "tIni") && r.contains_key(k) {
                            continue;
                        }
                        r.insert(k.into(), x.clone());
                    }
                }
                r.entry("lootIni").or_insert(json!(0));
                r.entry("comprasIni").or_insert(json!(0));
            }
        }
    }
    n
}

async fn ler(tx: &mut sqlx::PgConnection, chave: &str) -> Result<Option<Value>> {
    Ok(
        sqlx::query_scalar("SELECT dados FROM nexus.motor_estado WHERE chave = $1 FOR UPDATE")
            .bind(chave)
            .fetch_optional(&mut *tx)
            .await?,
    )
}

async fn gravar(tx: &mut sqlx::PgConnection, chave: &str, v: &Value) -> Result<()> {
    sqlx::query(
        "INSERT INTO nexus.motor_estado (chave, dados, texto, ordem)
         VALUES ($1, $2, $2::json, (SELECT coalesce(max(ordem), 0) + 1 FROM nexus.motor_estado))
         ON CONFLICT (chave) DO UPDATE SET dados = EXCLUDED.dados, texto = EXCLUDED.texto",
    )
    .bind(chave)
    .bind(v)
    .execute(&mut *tx)
    .await?;
    Ok(())
}

pub async fn ciclo(db: &PgPool) -> Result<usize> {
    let eventos: Vec<(String, String, Value)> = sqlx::query_as(
        // Diário com o rótulo da conta no lugar do nick do jogo (o app, logo depois de um
        // reinício, contava o HUD para o rótulo) não entra quando o jogo já deu outro nick.
        "SELECT DISTINCT ON (e.dados->>'jogo_nick', e.dados->>'dia')
                e.dados->>'jogo_nick', e.dados->>'dia', e.dados
           FROM nexus.evento e
          WHERE e.tipo = 'diario' AND e.em > now() - interval '3 days'
            AND e.dados ? 'jogo_nick' AND e.dados ? 'dia'
            AND NOT EXISTS (
                SELECT 1 FROM nexus.motor_janela j
                 WHERE j.conta = lower(e.conta)
                   AND lower(e.dados->>'jogo_nick') = j.conta
                   AND j.janela->>'nick' IS NOT NULL
                   AND j.janela->>'nick' <> e.dados->>'jogo_nick')
          ORDER BY e.dados->>'jogo_nick', e.dados->>'dia', e.em DESC",
    )
    .fetch_all(db)
    .await?;
    if eventos.is_empty() {
        return Ok(0);
    }
    let mut tx = db.begin().await?;
    let mut dia = ler(&mut tx, "dia").await?.unwrap_or_else(|| json!({}));
    let mut saldo = ler(&mut tx, "saldo").await?.unwrap_or_else(|| json!({}));
    let n = aplicar(&mut dia, &mut saldo, &eventos);
    if n > 0 {
        gravar(&mut tx, "dia", &dia).await?;
        gravar(&mut tx, "saldo", &saldo).await?;
        tx.commit().await?;
        info!(dias = n, "motor_dia: diário do app aplicado");
    }
    Ok(n)
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn atualiza_so_os_campos_do_app() {
        let mut dia = json!({"Sikien": {"2026-11-14": {"kills": 1, "compras": 50, "capturas": [1]},
                                        "2026-11-10": {"kills": 9}}});
        let mut saldo = json!({"Sikien": {"2026-11-14": {"ini": 10, "tIni": 1, "lootIni": 3}}});
        let ev = json!({"kills": 455, "ouro": 849.0, "xpPk": 5.0, "drops": {"X": 2},
                        "saldo": {"ini": 99, "fim": 500, "min": 10, "max": 600, "tIni": 7, "tFim": 8}});
        let n = aplicar(
            &mut dia,
            &mut saldo,
            &[("Sikien".into(), "2026-11-14".into(), ev.clone())],
        );
        assert_eq!(n, 1);
        let d = &dia["Sikien"]["2026-11-14"];
        assert_eq!(
            (
                d["kills"].clone(),
                d["compras"].clone(),
                d["capturas"].clone()
            ),
            (json!(455), json!(50), json!([1]))
        );
        assert_eq!(dia["Sikien"]["2026-11-10"]["kills"], 9);
        let s = &saldo["Sikien"]["2026-11-14"];
        assert_eq!(
            (s["ini"].clone(), s["fim"].clone(), s["lootIni"].clone()),
            (json!(10), json!(500), json!(3))
        );
        // De novo, igual: nada muda.
        assert_eq!(
            aplicar(
                &mut dia,
                &mut saldo,
                &[("Sikien".into(), "2026-11-14".into(), ev)]
            ),
            0
        );
        // Conta nova cria a chave.
        let mut vazio = Value::Null;
        let mut s2 = Value::Null;
        assert_eq!(
            aplicar(
                &mut vazio,
                &mut s2,
                &[("Novo".into(), "2026-11-14".into(), json!({"kills": 1}))]
            ),
            1
        );
        assert_eq!(vazio["Novo"]["2026-11-14"]["compras"], 0);
    }
}
