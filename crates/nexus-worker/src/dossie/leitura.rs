//! Rotas de leitura do dossiê. Mesmas consultas do legado, nas tabelas `public`.
//!
//! O node-pg devolve `bigint`/`numeric` como **string**, `date`/`timestamptz` como `Date`
//! (vira ISO no JSON) e `real` pelo texto do Postgres. Para o JSON sair igual, cada linha é
//! montada no próprio Postgres com `json_build_object` (mantém a ordem das colunas) e os
//! casts de `Tipo`. Em JS: é o que o `pg.types` faz por baixo do `rows`.

use std::path::Path;
use std::sync::LazyLock;

use axum::response::Response;
use serde_json::{json, Value};
use sqlx::AssertSqlSafe;

use super::acesso::chave_ok;
use super::{enviar, js, Dossie, Erro, Pedido};

#[derive(Clone, Copy)]
pub enum Tipo {
    /// text, int2/int4, bool, jsonb, float8: igual no JSON.
    Cru,
    /// int8/numeric: o node-pg devolve string.
    Texto,
    /// date: `Date` à meia-noite no fuso do processo (UTC).
    Data,
    /// timestamptz: `Date` com milissegundos.
    Instante,
    /// real (float4): o node-pg faz `parseFloat` do texto (0.3, não 0.30000001…).
    Real,
}

/// `json_build_object('col', expr, …)` na ordem dada.
pub fn objeto(colunas: &[(&str, Tipo)]) -> String {
    let partes: Vec<String> = colunas
        .iter()
        .map(|(c, t)| {
            let expr = match t {
                Tipo::Cru => (*c).to_owned(),
                Tipo::Texto => format!("{c}::text"),
                Tipo::Data => format!("to_char({c}, 'YYYY-MM-DD\"T\"00:00:00.000\"Z\"')"),
                Tipo::Instante => {
                    format!("to_char({c} AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')")
                }
                Tipo::Real => format!("{c}::text::float8"),
            };
            format!("'{c}', {expr}")
        })
        .collect();
    format!("json_build_object({})", partes.join(", "))
}

use Tipo::{Cru, Data, Instante, Real, Texto};

/// `SELECT * FROM pokemon_ficha`, na ordem das colunas do legado.
const FICHA: &[(&str, Tipo)] = &[
    ("id", Texto),
    ("nick", Cru),
    ("pokemon_id", Cru),
    ("nome", Cru),
    ("slug", Cru),
    ("dex_id", Cru),
    ("nivel", Cru),
    ("tipo_1", Cru),
    ("tipo_2", Cru),
    ("potencia", Cru),
    ("qualidade", Texto),
    ("iv_total", Cru),
    ("pve_power", Texto),
    ("hp_combate", Texto),
    ("pvp_power", Texto),
    ("nivel_efetivo", Cru),
    ("score", Texto),
    ("stat_hp", Cru),
    ("stat_atk", Cru),
    ("stat_def", Cru),
    ("stat_spatk", Cru),
    ("stat_spdef", Cru),
    ("stat_spd", Cru),
    ("iv_hp", Cru),
    ("iv_atk", Cru),
    ("iv_def", Cru),
    ("iv_spatk", Cru),
    ("iv_spdef", Cru),
    ("iv_spd", Cru),
    ("capturado_em", Data),
    ("capturado_por", Cru),
    ("raw_text", Cru),
    ("ts_salvo", Texto),
    ("oculto", Cru),
];

/// `SELECT * FROM dim_especie`.
const ESPECIE: &[(&str, Tipo)] = &[
    ("slug", Cru),
    ("nome_display", Cru),
    ("dex_id", Cru),
    ("tipo_1", Cru),
    ("tipo_2", Cru),
    ("hp_base", Cru),
    ("atk_base", Cru),
    ("def_base", Cru),
    ("spatk_base", Cru),
    ("spdef_base", Cru),
    ("spe_base", Cru),
    ("total_base", Cru),
    ("nivel_hunt", Cru),
    ("preco_npc", Cru),
    ("evolui_de", Cru),
    ("evolui_para", Cru),
    ("chance_ultra_ball_pct", Real),
    ("derrotas_ultra_ball", Cru),
    ("shiny_1_em_ultra_ball", Cru),
    ("fonte_csv", Cru),
    ("atualizado_em", Instante),
];

/// Colunas do `/api/dossie/dataset`.
const DATASET: &[(&str, Tipo)] = &[
    ("nome", Cru),
    ("slug", Cru),
    ("dex_id", Cru),
    ("nivel", Cru),
    ("tipo_1", Cru),
    ("tipo_2", Cru),
    ("potencia", Cru),
    ("qualidade", Texto),
    ("iv_total", Cru),
    ("pve_power", Texto),
    ("hp_combate", Texto),
    ("pvp_power", Texto),
    ("nivel_efetivo", Cru),
    ("score", Texto),
    ("stat_hp", Cru),
    ("stat_atk", Cru),
    ("stat_def", Cru),
    ("stat_spatk", Cru),
    ("stat_spdef", Cru),
    ("stat_spd", Cru),
    ("iv_hp", Cru),
    ("iv_atk", Cru),
    ("iv_def", Cru),
    ("iv_spatk", Cru),
    ("iv_spdef", Cru),
    ("iv_spd", Cru),
    ("capturado_em", Data),
    ("ts_salvo", Texto),
];

static SQL_FICHAS_NICKS: LazyLock<String> = LazyLock::new(|| {
    format!(
        "SELECT {} FROM public.pokemon_ficha
          WHERE (nick ILIKE ANY($1) OR capturado_por ILIKE ANY($1)) AND oculto IS NOT TRUE
          ORDER BY ts_salvo DESC LIMIT $2::text::bigint",
        objeto(FICHA)
    )
});
static SQL_FICHAS_NICK: LazyLock<String> = LazyLock::new(|| {
    format!(
        "SELECT {} FROM public.pokemon_ficha
          WHERE (capturado_por ILIKE $1 OR nick ILIKE $1) AND oculto IS NOT TRUE
          ORDER BY ts_salvo DESC LIMIT $2::text::bigint",
        objeto(FICHA)
    )
});
static SQL_FICHAS: LazyLock<String> = LazyLock::new(|| {
    format!(
        "SELECT {} FROM public.pokemon_ficha WHERE oculto IS NOT TRUE
          ORDER BY ts_salvo DESC LIMIT $1::text::bigint",
        objeto(FICHA)
    )
});
static SQL_ESPECIE: LazyLock<String> = LazyLock::new(|| {
    format!(
        "SELECT {} FROM public.dim_especie WHERE slug=$1 OR lower(nome_display)=lower($1)",
        objeto(ESPECIE)
    )
});
static SQL_DATASET: LazyLock<String> = LazyLock::new(|| {
    format!(
        "SELECT {} FROM public.pokemon_ficha ORDER BY ts_salvo DESC LIMIT 10000",
        objeto(DATASET)
    )
});

/// `GET /api/dossie/fichas?nick=|nicks=&limit=`
pub async fn fichas(d: &Dossie, p: &Pedido) -> Result<Response, Erro> {
    let nick = p.param("nick");
    let nicks: Vec<String> = p
        .param("nicks")
        .unwrap_or("")
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect();
    // `Math.min(+(limit || 60), 200)`: NaN passa adiante e o Postgres recusa, como no legado.
    let limite = match p.param("limit").filter(|s| !s.is_empty()) {
        Some(s) => js::numero_str(s),
        None => 60.0,
    };
    let limite = if limite.is_nan() {
        f64::NAN
    } else {
        limite.min(200.0)
    };
    let lim = nucleo::js::numero(limite);
    let linhas: Vec<Value> = if !nicks.is_empty() {
        sqlx::query_scalar(AssertSqlSafe(SQL_FICHAS_NICKS.as_str()))
            .bind(&nicks)
            .bind(&lim)
            .fetch_all(&d.db)
            .await?
    } else if let Some(n) = nick.filter(|n| !n.is_empty()) {
        sqlx::query_scalar(AssertSqlSafe(SQL_FICHAS_NICK.as_str()))
            .bind(n)
            .bind(&lim)
            .fetch_all(&d.db)
            .await?
    } else {
        sqlx::query_scalar(AssertSqlSafe(SQL_FICHAS.as_str()))
            .bind(&lim)
            .fetch_all(&d.db)
            .await?
    };
    Ok(enviar(&json!({ "fichas": linhas }), 200))
}

/// `null` se ausente, senão `String(+valor)` (como o node-pg manda o número).
fn numero_ou_null(p: &Pedido, nome: &str) -> Option<String> {
    p.param(nome).map(|s| nucleo::js::numero(js::numero_str(s)))
}

/// `GET /api/dossie/especie?slug=&score=&iv=&q=`
pub async fn especie(d: &Dossie, p: &Pedido) -> Result<Response, Erro> {
    let slug = p.param("slug").unwrap_or("");
    let esp: Option<Value> = sqlx::query_scalar(AssertSqlSafe(SQL_ESPECIE.as_str()))
        .bind(slug)
        .fetch_all(&d.db)
        .await?
        .into_iter()
        .next();
    let media: Option<Value> = sqlx::query_scalar(
        "SELECT json_build_object(
                'n', COUNT(*)::text,
                'q_media', AVG(qualidade)::numeric(10,3)::text,
                'iv_media', AVG(iv_total)::numeric(10,1)::text,
                'score_medio', AVG(score)::numeric(10,2)::text,
                'score_max', MAX(score)::text,
                'nivel_max', MAX(nivel),
                'pve_medio', AVG(pve_power)::numeric(12,0)::text,
                'n_abaixo_score', (COUNT(*) FILTER (WHERE score < $2::text::numeric))::text,
                'n_abaixo_iv', (COUNT(*) FILTER (WHERE iv_total < $3::text::int4))::text,
                'n_abaixo_q', (COUNT(*) FILTER (WHERE qualidade < $4::text::numeric))::text)
           FROM public.pokemon_ficha WHERE slug=$1",
    )
    .bind(slug)
    .bind(numero_ou_null(p, "score"))
    .bind(numero_ou_null(p, "iv"))
    .bind(numero_ou_null(p, "q"))
    .fetch_optional(&d.db)
    .await?;
    Ok(enviar(&json!({ "especie": esp, "media": media }), 200))
}

/// `GET /api/dossie/dataset`
pub async fn dataset(d: &Dossie) -> Result<Response, Erro> {
    let linhas: Vec<Value> = sqlx::query_scalar(AssertSqlSafe(SQL_DATASET.as_str()))
        .fetch_all(&d.db)
        .await?;
    Ok(enviar(&json!({ "dataset": linhas }), 200))
}

const SQL_MEDIDAS_GLOBAIS: &str = "SELECT especie_alvo, avg(xp_pokemon)::float xp, count(*)::int n
           FROM public.fato_kill WHERE especie_alvo IS NOT NULL AND xp_pokemon>0
          GROUP BY especie_alvo HAVING count(*) >= 3";

type Medida = (String, f64, i32);

fn medidas_json(linhas: impl IntoIterator<Item = Medida>) -> Vec<Value> {
    linhas
        .into_iter()
        .map(|(alvo, xp, n)| {
            json!({ "especie_alvo": alvo, "xp": js::json_num(nucleo::js::round(xp)), "n": n })
        })
        .collect()
}

/// `GET /api/dossie/cacada-medidas?esp=&nick=`: XP por abate medido, por espécie-alvo.
pub async fn cacada_medidas(d: &Dossie, p: &Pedido) -> Result<Response, Erro> {
    if !d.limites.ok(&p.ip, 30) {
        return Ok(enviar(&json!({ "erro": "muitas requisições" }), 429));
    }
    let esp: String = p
        .param("esp")
        .unwrap_or("")
        .to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '_')
        .collect();
    let nick = p.param("nick").unwrap_or("").trim().to_owned();
    if esp.is_empty() {
        return Ok(enviar(&json!({ "medidas": [] }), 200));
    }
    // O legado engole qualquer erro e responde lista vazia.
    match medidas(d, &nick).await {
        Ok(v) => Ok(enviar(&v, 200)),
        Err(_) => Ok(enviar(&json!({ "medidas": [] }), 200)),
    }
}

async fn medidas(d: &Dossie, nick: &str) -> Result<Value, sqlx::Error> {
    if !nick.is_empty() {
        let ids: Vec<String> =
            sqlx::query_scalar("SELECT conta_id FROM public.dim_conta WHERE nick ILIKE $1")
                .bind(nick)
                .fetch_all(&d.db)
                .await?;
        if !ids.is_empty() {
            let pessoais: Vec<Medida> = sqlx::query_as(
                "SELECT especie_alvo, avg(xp_pokemon)::float xp, count(*)::int n
                   FROM public.fato_kill WHERE especie_alvo IS NOT NULL AND xp_pokemon>0 AND conta_id=ANY($1)
                  GROUP BY especie_alvo HAVING count(*) >= 2",
            )
            .bind(&ids)
            .fetch_all(&d.db)
            .await?;
            if !pessoais.is_empty() {
                let globais: Vec<Medida> =
                    sqlx::query_as(SQL_MEDIDAS_GLOBAIS).fetch_all(&d.db).await?;
                let tem: std::collections::HashSet<String> =
                    pessoais.iter().map(|m| m.0.clone()).collect();
                let extra = globais.into_iter().filter(|m| !tem.contains(&m.0));
                let todas = medidas_json(pessoais.into_iter().chain(extra));
                return Ok(json!({ "medidas": todas, "escopo": "conta" }));
            }
        }
    }
    let globais: Vec<Medida> = sqlx::query_as(SQL_MEDIDAS_GLOBAIS).fetch_all(&d.db).await?;
    Ok(json!({ "medidas": medidas_json(globais) }))
}

/// `GET /api/dossie/fotos`: fotos do diretório do legado, mais nova primeiro.
pub async fn fotos(d: &Dossie) -> Result<Response, Erro> {
    let dir = d.fotos_dir();
    let lista = tokio::task::spawn_blocking(move || listar_fotos(&dir))
        .await
        .map_err(|e| Erro::novo(500, e.to_string()))??;
    Ok(enviar(&json!({ "fotos": lista }), 200))
}

fn listar_fotos(dir: &Path) -> Result<Vec<Value>, Erro> {
    let mut nomes: Vec<String> = std::fs::read_dir(dir)
        .map_err(|_| {
            Erro::novo(
                500,
                format!(
                    "ENOENT: no such file or directory, scandir '{}'",
                    dir.display()
                ),
            )
        })?
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.starts_with("foto-"))
        .collect();
    // `sort()` do JS compara unidades UTF-16; para nomes ASCII é a ordem de bytes.
    nomes.sort();
    nomes.reverse();
    Ok(nomes
        .into_iter()
        .map(|nome| {
            let arq = dir.join(&nome);
            // `d.w || null`: zero ou ausente vira null, cada um por si.
            let (w, h) = dim_imagem(&arq).unwrap_or((0, 0));
            let nulo = |v: u64| if v == 0 { Value::Null } else { v.into() };
            let (w, h) = (nulo(w), nulo(h));
            let bytes = std::fs::metadata(&arq).map(|m| m.len()).unwrap_or(0);
            json!({ "nome": nome, "url": format!("/fotos/{nome}"), "bytes": bytes, "w": w, "h": h })
        })
        .collect())
}

/// `dimImagem`: largura/altura lendo só o cabeçalho (PNG, GIF, WEBP, JPEG).
pub fn dim_imagem(arq: &Path) -> Option<(u64, u64)> {
    use std::io::Read;
    let mut f = std::fs::File::open(arq).ok()?;
    let mut b = vec![0u8; 262_144];
    let mut n = 0;
    while n < b.len() {
        match f.read(&mut b[n..]) {
            Ok(0) => break,
            Ok(k) => n += k,
            Err(_) => return None,
        }
    }
    // O legado lê 64 bytes zerados para os formatos fixos; o resto do buffer já é zero.
    let h = &b[..];
    let u16be = |i: usize| u64::from(u16::from_be_bytes([h[i], h[i + 1]]));
    let u16le = |i: usize| u64::from(u16::from_le_bytes([h[i], h[i + 1]]));
    let u32be = |i: usize| u64::from(u32::from_be_bytes([h[i], h[i + 1], h[i + 2], h[i + 3]]));
    let u24le = |i: usize| u64::from(h[i]) | u64::from(h[i + 1]) << 8 | u64::from(h[i + 2]) << 16;
    let nz = Some;
    if h[0] == 0x89 && h[1] == 0x50 {
        return nz((u32be(16), u32be(20)));
    }
    if &h[0..3] == b"GIF" {
        return nz((u16le(6), u16le(8)));
    }
    if &h[0..4] == b"RIFF" && &h[8..12] == b"WEBP" {
        match &h[12..16] {
            b"VP8X" => return nz((1 + u24le(24), 1 + u24le(27))),
            b"VP8 " => return nz((u16le(26) & 0x3fff, u16le(28) & 0x3fff)),
            b"VP8L" => {
                let v = u64::from(u32::from_le_bytes([h[21], h[22], h[23], h[24]]));
                return nz(((v & 0x3fff) + 1, ((v >> 14) & 0x3fff) + 1));
            }
            _ => {}
        }
    }
    if h[0] == 0xFF && h[1] == 0xD8 {
        let mut i = 2;
        while i + 9 < n {
            if h[i] == 0xFF
                && (0xC0..=0xCF).contains(&h[i + 1])
                && ![0xC4, 0xC8, 0xCC].contains(&h[i + 1])
            {
                return nz((u16be(i + 7), u16be(i + 5)));
            }
            i += 1;
        }
    }
    None
}

/// `/api/ia/*`: chave obrigatória. O conteúdo depende de GRANT em tabelas de IA
/// (`ia_memoria`, `ia_playbook`, `v_ia_performance`, `config_ia`, `ia_estado`), ainda
/// pendente: com chave válida responde 501.
pub fn ia(d: &Dossie, p: &Pedido) -> Result<Response, Erro> {
    if p.caminho == "/api/ia/memoria" && !d.limites.ok(&p.ip, 30) {
        return Ok(enviar(&json!({ "erro": "muitas requisições" }), 429));
    }
    if !chave_ok(d, p) {
        return Ok(enviar(&json!({ "erro": "chave inválida" }), 401));
    }
    Ok(enviar(
        &json!({ "erro": "pendente no Nexus (GRANT nas tabelas de IA)" }),
        501,
    ))
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn objeto_monta_casts() {
        let s = objeto(&[("a", Cru), ("b", Texto), ("c", Real)]);
        assert_eq!(
            s,
            "json_build_object('a', a, 'b', b::text, 'c', c::text::float8)"
        );
    }

    #[test]
    fn dim_de_png_e_gif() {
        let dir = std::env::temp_dir();
        let png = dir.join(format!("dim-{}.png", std::process::id()));
        let mut b = vec![0u8; 64];
        b[0] = 0x89;
        b[1] = 0x50;
        b[16..20].copy_from_slice(&300u32.to_be_bytes());
        b[20..24].copy_from_slice(&200u32.to_be_bytes());
        std::fs::write(&png, &b).unwrap();
        assert_eq!(dim_imagem(&png), Some((300, 200)));
        let gif = dir.join(format!("dim-{}.gif", std::process::id()));
        let mut g = b"GIF89a".to_vec();
        g.extend_from_slice(&[10, 0, 20, 0]);
        std::fs::write(&gif, &g).unwrap();
        assert_eq!(dim_imagem(&gif), Some((10, 20)));
        let _ = std::fs::remove_file(png);
        let _ = std::fs::remove_file(gif);
    }
}
