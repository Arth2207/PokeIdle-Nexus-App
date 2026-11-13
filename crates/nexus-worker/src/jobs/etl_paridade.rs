//! Comparador de paridade Node × Nexus, em janelas **fechadas**.
//!
//! - `fato_kill`: `public.fato_kill` × `nexus.fato_kill_shadow`, por conta.
//! - `fato_hunt_eficiencia`: `public.fato_hunt_eficiencia` × `nexus.fato_hunt_eficiencia_shadow`.
//!
//! A janela termina numa hora cheia já passada (não em `NOW()`): Node e Nexus rodam em
//! cadências diferentes, então a borda recente sempre divergiria. O atraso é tolerância de
//! **tempo**, não de lógica — dentro da janela os totais têm de ser idênticos.
//! Grava em `nexus.etl_paridade_janela` com upsert: recalcular a mesma janela dá o mesmo resultado.

use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use sqlx::PgPool;
use tracing::warn;

const CONTAS: &[&str] = &["mvxorqh6", "mvxoui28", "mvxoxith", "mw002fcf"];
const JANELAS: &[(&str, i64)] = &[("1h", 1), ("6h", 6), ("24h", 24)];
/// Eficiência sem a janela de 24 h: os dois lados usam `ts_at >= NOW() - 24h`, então a hora
/// mais antiga é regravada parcial conforme o minuto em que cada um roda (Node `:05`,
/// Nexus `:mm` do boot). É efeito de agenda, igual nos dois códigos; horas com menos de
/// ~20 h de idade estão inteiras nos dois lados e têm de bater exato.
/// Correção (hora cheia) proposta em NEXUS_MELHORIAS.
const JANELAS_EFIC: &[(&str, i64)] = &[("1h", 1), ("6h", 6)];

/// fato_kill: o Node grava a cada 5 min e o Nexus lê o `mapa.db` sem o WAL (`immutable=1`),
/// que pode atrasar alguns minutos até o checkpoint. 30 min cobre os dois.
const ATRASO_KILL_MIN: i64 = 30;
/// Eficiência: o Node roda a `:05` e o Nexus 1×/h a partir do boot; 2 h evita comparar
/// uma hora que um dos lados ainda não recalculou.
const ATRASO_EFIC_MIN: i64 = 120;

/// Nome de cada posição do vetor de totais (rótulo `metrica` no Prometheus).
/// Na eficiência as posições 4–5 são `kills_total`/`duracao_s` e 6–7 ficam 0.
const METRICAS: [&str; 8] = [
    "linhas", "xp_tr", "xp_pk", "ouro", "extra_1", "extra_2", "extra_3", "nulos",
];

// Mesmo SELECT nos dois lados; só muda a tabela. `$1` conta, `[$2, $3)` janela.
macro_rules! sql_kill {
    ($tabela:literal) => {
        concat!(
            "SELECT ARRAY[COUNT(*),
                    COALESCE(SUM(xp_treinador), 0),
                    COALESCE(SUM(xp_pokemon), 0),
                    COALESCE(SUM(ouro), 0),
                    COALESCE(SUM(ouro_loot_bonus), 0),
                    COUNT(*) FILTER (WHERE loot_boost),
                    COUNT(*) FILTER (WHERE shiny),
                    COUNT(*) FILTER (WHERE xp_treinador IS NULL OR xp_pokemon IS NULL
                                        OR ouro IS NULL OR hunt_slug IS NULL)]::bigint[]
               FROM ",
            $tabela,
            " WHERE conta_id = $1 AND ts_at >= $2 AND ts_at < $3"
        )
    };
}

// `$1` (conta) não filtra na eficiência: a tabela não tem conta. Mantido para a mesma assinatura.
macro_rules! sql_efic {
    ($tabela:literal) => {
        concat!(
            "SELECT ARRAY[COUNT(*),
                    COALESCE(SUM(xp_tr_total), 0),
                    COALESCE(SUM(xp_pk_total), 0),
                    COALESCE(SUM(ouro_total), 0),
                    COALESCE(SUM(kills_total), 0),
                    COALESCE(SUM(duracao_s), 0),
                    0, 0]::bigint[]
               FROM ",
            $tabela,
            " WHERE $1::text IS NOT NULL
                AND ts_hora >= (EXTRACT(epoch FROM $2::timestamptz) * 1000)::bigint
                AND ts_hora <  (EXTRACT(epoch FROM $3::timestamptz) * 1000)::bigint"
        )
    };
}

/// Hora cheia de `agora - atraso`. Em JS: `Math.floor((agora - atraso) / 3600000) * 3600000`.
fn fim_janela(agora: DateTime<Utc>, atraso_min: i64) -> DateTime<Utc> {
    let t = (agora - Duration::minutes(atraso_min)).timestamp();
    let hora = t - t.rem_euclid(3600);
    DateTime::from_timestamp(hora, 0).unwrap_or(agora)
}

struct Alvo {
    tabela: &'static str,
    conta: &'static str,
    sql_node: &'static str,
    sql_nexus: &'static str,
    fim: DateTime<Utc>,
    janelas: &'static [(&'static str, i64)],
}

pub async fn ciclo(db: &PgPool) -> Result<usize> {
    let agora = Utc::now();
    let fim_kill = fim_janela(agora, ATRASO_KILL_MIN);

    let mut alvos: Vec<Alvo> = CONTAS
        .iter()
        .map(|c| Alvo {
            tabela: "fato_kill",
            conta: c,
            sql_node: sql_kill!("public.fato_kill"),
            sql_nexus: sql_kill!("nexus.fato_kill_shadow"),
            fim: fim_kill,
            janelas: JANELAS,
        })
        .collect();
    alvos.push(Alvo {
        tabela: "fato_hunt_eficiencia",
        conta: "*",
        sql_node: sql_efic!("public.fato_hunt_eficiencia"),
        sql_nexus: sql_efic!("nexus.fato_hunt_eficiencia_shadow"),
        fim: fim_janela(agora, ATRASO_EFIC_MIN),
        janelas: JANELAS_EFIC,
    });

    let mut ok = 0usize;
    for a in &alvos {
        for (janela, horas) in a.janelas {
            let ini = a.fim - Duration::hours(*horas);
            match comparar(db, a, janela, ini).await {
                Ok(()) => ok += 1,
                Err(e) => warn!(
                    tabela = a.tabela,
                    conta = a.conta,
                    janela,
                    erro = %e,
                    "etl_paridade: erro ao comparar"
                ),
            }
        }
    }
    // Nenhuma comparação gravada (ex.: tabela ausente) é falha do ciclo, não "ok com 0".
    anyhow::ensure!(ok > 0, "nenhuma comparação concluída");
    Ok(ok)
}

async fn totais(db: &PgPool, sql: &'static str, a: &Alvo, ini: DateTime<Utc>) -> Result<Vec<i64>> {
    let (v,): (Vec<i64>,) = sqlx::query_as(sql)
        .bind(a.conta)
        .bind(ini)
        .bind(a.fim)
        .fetch_one(db)
        .await?;
    Ok(v)
}

async fn comparar(db: &PgPool, a: &Alvo, janela: &str, ini: DateTime<Utc>) -> Result<()> {
    let node = totais(db, a.sql_node, a, ini).await?;
    let nexus = totais(db, a.sql_nexus, a, ini).await?;

    sqlx::query(
        "INSERT INTO nexus.etl_paridade_janela (tabela, conta_id, janela, ini, fim, node, nexus)
         VALUES ($1, $2, $3, $4, $5, $6, $7)
         ON CONFLICT (tabela, conta_id, janela, fim) DO UPDATE
            SET em = NOW(), node = EXCLUDED.node, nexus = EXCLUDED.nexus",
    )
    .bind(a.tabela)
    .bind(a.conta)
    .bind(janela)
    .bind(ini)
    .bind(a.fim)
    .bind(&node)
    .bind(&nexus)
    .execute(db)
    .await?;

    // `nexus_etl_paridade_igual` (1/0) e `nexus_etl_paridade_diff{metrica}` = Node − Nexus.
    let rotulo = |k: &'static str, v: &str| (k, v.to_owned());
    let base = [
        rotulo("tabela", a.tabela),
        rotulo("conta", a.conta),
        rotulo("janela", janela),
    ];
    metrics::gauge!("nexus_etl_paridade_igual", &base).set(if node == nexus { 1.0 } else { 0.0 });
    for ((nome, n), x) in METRICAS.iter().zip(&node).zip(&nexus) {
        let mut r = base.to_vec();
        r.push(rotulo("metrica", nome));
        metrics::gauge!("nexus_etl_paridade_diff", &r).set((n - x) as f64);
    }
    Ok(())
}

/// Última janela de cada (tabela, conta, janela) calculada nas últimas `horas` horas,
/// mais quantas janelas de 1 h divergiram no período (para ver se a falta é sistemática).
pub async fn relatorio(db: &PgPool, horas: u32) -> Result<(Vec<LinhaRelatorio>, i64, i64)> {
    let linhas = sqlx::query_as::<_, LinhaRelatorio>(
        "SELECT DISTINCT ON (tabela, conta_id, janela)
                tabela, conta_id, janela, fim, node, nexus, igual
           FROM nexus.etl_paridade_janela
          WHERE em >= NOW() - ($1::int * interval '1 hour')
          ORDER BY tabela, conta_id, janela, fim DESC",
    )
    .bind(horas as i32)
    .fetch_all(db)
    .await?;
    let (total, divergentes): (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*)::bigint, COUNT(*) FILTER (WHERE NOT igual)::bigint
           FROM nexus.etl_paridade_janela
          WHERE janela = '1h' AND fim >= NOW() - ($1::int * interval '1 hour')",
    )
    .bind(horas as i32)
    .fetch_one(db)
    .await?;
    Ok((linhas, total, divergentes))
}

#[derive(sqlx::FromRow, Debug)]
pub struct LinhaRelatorio {
    pub tabela: String,
    pub conta_id: String,
    pub janela: String,
    pub fim: DateTime<Utc>,
    pub node: Vec<i64>,
    pub nexus: Vec<i64>,
    pub igual: bool,
}

impl LinhaRelatorio {
    /// Só as posições que diferem, ex.: `xp_pk 120≠118`. Vazio quando igual.
    pub fn diferencas(&self) -> String {
        METRICAS
            .iter()
            .zip(self.node.iter().zip(&self.nexus))
            .filter(|(_, (n, x))| n != x)
            .map(|(nome, (n, x))| format!("{nome} {n}≠{x}"))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn fim_janela_e_hora_cheia_com_atraso() {
        let t = |s: &str| DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc);
        assert_eq!(
            fim_janela(t("2026-11-13T10:29:59Z"), 30),
            t("2026-11-13T09:00:00Z")
        );
        assert_eq!(
            fim_janela(t("2026-11-13T10:30:00Z"), 30),
            t("2026-11-13T10:00:00Z")
        );
        assert_eq!(
            fim_janela(t("2026-11-13T10:05:00Z"), 120),
            t("2026-11-13T08:00:00Z")
        );
    }

    #[test]
    fn diferencas_lista_so_o_que_diverge() {
        let l = LinhaRelatorio {
            tabela: "fato_kill".into(),
            conta_id: "c".into(),
            janela: "1h".into(),
            fim: Utc::now(),
            node: vec![10, 5, 6, 7, 0, 0, 0, 0],
            nexus: vec![10, 5, 4, 7, 0, 0, 0, 1],
            igual: false,
        };
        assert_eq!(l.diferencas(), "xp_pk 6≠4, nulos 0≠1");
    }
}
