//! Rotas de escrita do dossiê, em **sombra**: o mesmo SQL do legado roda nas tabelas
//! `nexus.*_shadow`, e os uploads vão para `dados_dir`. Nada toca o `public` nem os
//! arquivos do legado. No cutover, troca-se a tabela/diretório de destino.
//!
//! Diferenças conhecidas, só com entrada patológica (revisão de 2026-11-13):
//! - foto em JSON com `b64`/`nome` que não são string: o JS estoura (500), aqui converte.
//! - corpo com UTF-8 inválido, número fora do f64 ou >128 níveis: o JS faz parse, aqui `{}`.
//! - corte de `tipo`/`descricao` no meio de um emoji: o JS manda meia letra e o Postgres
//!   recusa (500); aqui a letra sai inteira e grava.

use std::sync::LazyLock;

use axum::response::Response;
use regex::Regex;
use serde_json::{json, Map, Value};

use super::acesso::chave_ok;
use super::estaticos::basename;
use axum::body::Body;

use super::{enviar, js, ler_corpo, ler_json, Dossie, Erro, Pedido};

const MAX_FOTO: usize = 30_000_000;
const MAX_FOTOS: usize = 500;
/// O legado aceita 200 MB no `/arquivo`. Na sombra o teto é 24 MB: o corpo fica inteiro na
/// memória (o multipart precisa dele) e o worker tem 256 MB com o ETL no mesmo processo.
/// No cutover: subir o limite do container ou gravar em streaming antes de voltar a 200 MB.
const MAX_ARQUIVO: usize = 24 * 1024 * 1024;

/// Um upload por vez: dois corpos grandes juntos estourariam a memória do worker.
static UPLOAD: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(1);

/// Itens que o precificador cota; a venda de item só traz a descrição "N× Nome".
fn item_por_nome(nome: &str) -> Option<i64> {
    match nome {
        "fragmento de z-move" | "z-move fragment" => Some(59194),
        "aoe tm disk piece" => Some(40530),
        "mega stone fragment" => Some(70014),
        "mega shiny stone fragment" => Some(70015),
        _ => None,
    }
}

// `/^(\d+)\s*[×x]\s*(.+)$/`: `\d` ASCII e `.` sem quebras de linha, como no JS.
static QTD_NOME: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"^([0-9]+)\s*[×x]\s*([^\n\r\u{2028}\u{2029}]+)$").ok());

fn chave_ou_401(d: &Dossie, p: &Pedido) -> Option<Response> {
    (!chave_ok(d, p)).then(|| enviar(&json!({ "erro": "chave inválida" }), 401))
}

fn muitas(d: &Dossie, p: &Pedido, lim: u32, msg: &str) -> Option<Response> {
    (!d.limites.ok(&p.ip, lim)).then(|| enviar(&json!({ "erro": msg }), 429))
}

const MSG_LIMITE: &str = "muitas requisições, tenta de novo em 1 min";

/// Monta a linha de uma venda como o legado; `None` se não passar no filtro.
pub fn linha_venda(r: &Value, lido: f64) -> Option<Value> {
    let r = Some(r);
    let id = js::numero(js::campo(r, "id"));
    let em = js::numero(js::campo(r, "em"));
    let preco = js::numero(js::campo(r, "bruto"));
    // `!(id > 0 && …)`: NaN reprova.
    if !(id > 0.0 && em > 0.0 && preco > 0.0) {
        return None;
    }
    // `typeof r.ficha === 'object'` inclui array; `{...array}` vira objeto de índices.
    let ficha_bruta = js::campo(r, "ficha").filter(|v| js::verdade(Some(v)));
    let ficha: Option<Map<String, Value>> = match ficha_bruta {
        Some(Value::Object(o)) => {
            let mut o = o.clone();
            o.shift_remove("apelido");
            Some(o)
        }
        Some(Value::Array(a)) => Some(
            a.iter()
                .enumerate()
                .map(|(i, v)| (i.to_string(), v.clone()))
                .collect(),
        ),
        _ => None,
    };
    let descricao_txt = if js::verdade(js::campo(r, "descricao")) {
        js::texto(js::campo(r, "descricao"))
    } else {
        String::new()
    };
    let mq = QTD_NOME
        .as_ref()
        .and_then(|re| re.captures(&descricao_txt))
        .map(|c| {
            (
                c.get(1).map_or("", |m| m.as_str()).to_owned(),
                c.get(2).map_or("", |m| m.as_str()).to_owned(),
            )
        });
    let species_id = ficha
        .as_ref()
        .and_then(|f| f.get("speciesId"))
        .and_then(Value::as_f64)
        .filter(|v| v.fract() == 0.0 && v.is_finite())
        .map_or(Value::Null, js::json_num);
    let item_bruto = js::numero(js::ou(
        js::campo(r, "itemId"),
        js::ou(
            js::campo(r, "item_id"),
            js::campo(js::campo(r, "item"), "id"),
        ),
    ));
    let item_id = if js::verdade_num(item_bruto) {
        js::json_num(item_bruto)
    } else {
        let nome = match &mq {
            Some((_, n)) => n.clone(),
            None => descricao_txt.clone(),
        };
        item_por_nome(&nome.trim().to_lowercase()).map_or(Value::Null, Value::from)
    };
    let qtd_bruta = js::numero(js::ou(js::campo(r, "quantidade"), js::campo(r, "qtd")));
    let quantidade = if js::verdade_num(qtd_bruta) {
        js::json_num(qtd_bruta)
    } else {
        match &mq {
            Some((n, _)) => js::json_num(js::numero_str(n)),
            None => 1.into(),
        }
    };
    let tipo = if js::verdade(js::campo(r, "tipo")) {
        js::texto(js::campo(r, "tipo"))
    } else {
        String::new()
    };
    let moeda = if js::verdade(js::campo(r, "moeda")) {
        js::texto(js::campo(r, "moeda"))
    } else {
        String::new()
    };
    Some(json!({
        "id": js::json_num(id),
        "em": js::json_num(em),
        "lido_em": js::json_num(lido),
        "tipo": js::fatia_utf16(&tipo, 20),
        "moeda": js::fatia_utf16(&moeda, 10),
        "preco": js::json_num(preco),
        "descricao": js::fatia_utf16(&descricao_txt, 120),
        "species_id": species_id,
        "item_id": item_id,
        "quantidade": quantidade,
        "ficha": ficha.map_or(Value::Null, Value::Object),
    }))
}

fn agora_ms() -> f64 {
    chrono::Utc::now().timestamp_millis() as f64
}

/// `POST /api/dossie/vendas`: vendas do Mercado da Comunidade, coletadas pelo relay.
pub async fn vendas(d: &Dossie, p: &Pedido, corpo: Body) -> Result<Response, Erro> {
    if let Some(r) = muitas(d, p, 60, MSG_LIMITE) {
        return Ok(r);
    }
    if let Some(r) = chave_ou_401(d, p) {
        return Ok(r);
    }
    let b = ler_json(corpo).await?;
    if b.is_null() {
        return Err(Erro::novo(
            500,
            "Cannot read properties of null (reading 'vendas')",
        ));
    }
    let lista: Vec<Value> = match b.get("vendas") {
        Some(Value::Array(a)) => a.iter().take(3000).cloned().collect(),
        _ => vec![],
    };
    let lido_bruto = js::numero(b.get("lidoEm"));
    let lido = if lido_bruto.is_finite() && lido_bruto > 0.0 {
        lido_bruto
    } else {
        agora_ms()
    };
    let linhas: Vec<Value> = lista.iter().filter_map(|r| linha_venda(r, lido)).collect();
    if linhas.is_empty() {
        return Ok(enviar(
            &json!({ "ok": true, "recebidas": lista.len(), "novas": 0 }),
            200,
        ));
    }
    let novas = sqlx::query(
        "INSERT INTO nexus.fato_venda_mercado_shadow
                (id, em, lido_em, tipo, moeda, preco, descricao, species_id, item_id, quantidade, ficha)
         SELECT id, em, lido_em, tipo, moeda, preco, descricao, species_id, item_id, quantidade, ficha
           FROM jsonb_to_recordset($1::jsonb) AS x(id bigint, em bigint, lido_em bigint, tipo text, moeda text, preco bigint,
                descricao text, species_id int, item_id int, quantidade int, ficha jsonb)
         ON CONFLICT (id) DO NOTHING",
    )
    .bind(Value::Array(linhas.clone()).to_string())
    .execute(&d.db)
    .await?
    .rows_affected();
    Ok(enviar(
        &json!({ "ok": true, "recebidas": lista.len(), "validas": linhas.len(), "novas": novas }),
        200,
    ))
}

/// Colunas do `pokemon_ficha` na ordem dos `$n` do legado, com o tipo do Postgres.
/// O node-pg manda texto sem tipo e o Postgres infere pela coluna; `$n::text::tipo` dá o
/// mesmo resultado (e a mesma mensagem de erro para valor inválido).
const COLUNAS_FICHA: &[(&str, &str)] = &[
    ("nick", "text"),
    ("pokemon_id", "text"),
    ("nome", "text"),
    ("slug", "text"),
    ("dex_id", "int4"),
    ("nivel", "int4"),
    ("tipo_1", "text"),
    ("tipo_2", "text"),
    ("potencia", "text"),
    ("qualidade", "numeric"),
    ("iv_total", "int4"),
    ("pve_power", "numeric"),
    ("hp_combate", "numeric"),
    ("pvp_power", "numeric"),
    ("nivel_efetivo", "int4"),
    ("score", "numeric"),
    ("stat_hp", "int4"),
    ("stat_atk", "int4"),
    ("stat_def", "int4"),
    ("stat_spatk", "int4"),
    ("stat_spdef", "int4"),
    ("stat_spd", "int4"),
    ("iv_hp", "int2"),
    ("iv_atk", "int2"),
    ("iv_def", "int2"),
    ("iv_spatk", "int2"),
    ("iv_spdef", "int2"),
    ("iv_spd", "int2"),
    ("capturado_em", "date"),
    ("capturado_por", "text"),
    ("raw_text", "text"),
];

static SQL_FICHA: LazyLock<String> = LazyLock::new(|| {
    let nomes: Vec<&str> = COLUNAS_FICHA.iter().map(|(c, _)| *c).collect();
    let valores: Vec<String> = COLUNAS_FICHA
        .iter()
        .enumerate()
        .map(|(i, (_, t))| format!("${}::text::{t}", i + 1))
        .collect();
    let n = COLUNAS_FICHA.len() + 1;
    let sets: Vec<String> = nomes
        .iter()
        .map(|c| format!("{c}=COALESCE(EXCLUDED.{c},pokemon_ficha_shadow.{c})"))
        .collect();
    format!(
        "INSERT INTO nexus.pokemon_ficha_shadow AS pokemon_ficha_shadow ({}, oculto)
         VALUES ({}, ${n}::bool)
         ON CONFLICT (pokemon_id) WHERE pokemon_id IS NOT NULL DO UPDATE SET
           {},
           oculto=(pokemon_ficha_shadow.oculto OR EXCLUDED.oculto),
           ts_salvo=extract(epoch from now())*1000
         RETURNING id::text",
        nomes.join(","),
        valores.join(","),
        // `pokemon_id` não entra no SET (é a chave do conflito), igual ao legado.
        sets.into_iter()
            .filter(|s| !s.starts_with("pokemon_id="))
            .collect::<Vec<_>>()
            .join(",\n           ")
    )
});

/// `POST /api/dossie/ficha`: salva ficha (sem chave, como no legado).
pub async fn ficha(d: &Dossie, p: &Pedido, corpo: Body) -> Result<Response, Erro> {
    if let Some(r) = muitas(d, p, 30, MSG_LIMITE) {
        return Ok(r);
    }
    let b = ler_json(corpo).await?;
    if b.is_null() {
        return Err(Erro::novo(
            500,
            "Cannot read properties of null (reading 'nome')",
        ));
    }
    if !js::verdade(b.get("nome")) {
        return Ok(enviar(&json!({ "erro": "ficha sem nome" }), 400));
    }
    let mut q = sqlx::query_scalar::<_, String>(sqlx::AssertSqlSafe(SQL_FICHA.as_str()));
    for (col, _) in COLUNAS_FICHA {
        // `b.nome` vai cru; o resto é `b.x || null`.
        let v = if *col == "nome" {
            js::param_pg(b.get(*col))
        } else {
            js::param_ou_null(b.get(*col))
        };
        q = q.bind(v);
    }
    let id = q
        .bind(js::verdade(b.get("oculto")).to_string())
        .fetch_one(&d.db)
        .await?;
    // `rows[0].id` é bigint → string no node-pg.
    Ok(enviar(&json!({ "ok": true, "id": id }), 200))
}

/// `DELETE /api/dossie/ficha?id=`
pub async fn apagar_ficha(d: &Dossie, p: &Pedido) -> Result<Response, Erro> {
    if let Some(r) = muitas(d, p, 15, "muitas requisições") {
        return Ok(r);
    }
    if let Some(r) = chave_ou_401(d, p) {
        return Ok(r);
    }
    let apagadas = sqlx::query("DELETE FROM nexus.pokemon_ficha_shadow WHERE id=$1::text::bigint")
        .bind(p.param("id"))
        .execute(&d.db)
        .await?
        .rows_affected();
    Ok(enviar(&json!({ "ok": apagadas > 0 }), 200))
}

/// Uma parte do multipart: (`name`, `filename`, dados).
type Parte = (String, Option<String>, Vec<u8>);

static NAME: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r#"name="([^"]*)""#).ok());
static FILENAME: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r#"filename="([^"]*)""#).ok());
static BOUNDARY: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r#"boundary=(?:"([^"]+)"|([^;]+))"#).ok());

/// Latin-1 → texto (cada byte vira um char), como o `toString('latin1')` do Node.
fn latin1(b: &[u8]) -> String {
    b.iter().map(|&c| char::from(c)).collect()
}

fn achar(h: &[u8], agulha: &[u8], desde: usize) -> Option<usize> {
    if agulha.is_empty() || h.len() < agulha.len() {
        return None;
    }
    (desde..=h.len() - agulha.len()).find(|&i| &h[i..i + agulha.len()] == agulha)
}

/// `parseMultipart` do legado, sobre bytes.
pub fn multipart(buf: &[u8], boundary: &str) -> Vec<Parte> {
    let delim = format!("--{boundary}").into_bytes();
    let mut segmentos = Vec::new();
    let mut ini = 0;
    while let Some(i) = achar(buf, &delim, ini) {
        segmentos.push(&buf[ini..i]);
        ini = i + delim.len();
    }
    segmentos.push(&buf[ini..]);
    let mut out = Vec::new();
    for seg in segmentos {
        let Some(fim_h) = achar(seg, b"\r\n\r\n", 0) else {
            continue;
        };
        let hdr = latin1(&seg[..fim_h]);
        let mut dados = &seg[fim_h + 4..];
        if dados.ends_with(b"\r\n") {
            dados = &dados[..dados.len() - 2];
        }
        let cap = |re: &Option<Regex>| {
            re.as_ref()
                .and_then(|r| r.captures(&hdr))
                .and_then(|c| c.get(1))
                .map(|m| m.as_str().to_owned())
        };
        if let Some(name) = cap(&NAME) {
            out.push((name, cap(&FILENAME), dados.to_vec()));
        }
    }
    out
}

fn boundary(ct: &str) -> String {
    BOUNDARY
        .as_ref()
        .and_then(|r| r.captures(ct))
        .and_then(|c| c.get(1).or_else(|| c.get(2)))
        .map_or_else(|| "undefined".to_owned(), |m| m.as_str().to_owned())
}

/// `Buffer.from(texto, 'base64')`: tolerante (ignora lixo, aceita url-safe e sem `=`).
pub fn base64_node(s: &str) -> Vec<u8> {
    let mut bits = 0u32;
    let mut n = 0;
    let mut out = Vec::new();
    for c in s.chars() {
        let v = match c {
            'A'..='Z' => c as u32 - 'A' as u32,
            'a'..='z' => c as u32 - 'a' as u32 + 26,
            '0'..='9' => c as u32 - '0' as u32 + 52,
            '+' | '-' => 62,
            '/' | '_' => 63,
            '=' => break,
            _ => continue,
        };
        bits = (bits << 6) | v;
        n += 6;
        if n >= 8 {
            n -= 8;
            out.push(((bits >> n) & 0xff) as u8);
        }
    }
    out
}

/// `extImagem`: pela assinatura, senão pela extensão do nome.
pub fn ext_imagem(buf: &[u8], nome: &str) -> Option<String> {
    let ini = |n: usize| buf.get(..n).unwrap_or(&[]);
    if buf.first() == Some(&0x89) && buf.get(1) == Some(&0x50) {
        return Some("png".into());
    }
    if buf.first() == Some(&0xFF) && buf.get(1) == Some(&0xD8) {
        return Some("jpg".into());
    }
    if ini(4) == b"RIFF" && buf.get(8..12) == Some(b"WEBP") {
        return Some("webp".into());
    }
    if ini(6) == b"GIF87a" || ini(6) == b"GIF89a" {
        return Some("gif".into());
    }
    static EXT: LazyLock<Option<Regex>> =
        LazyLock::new(|| Regex::new(r"(?i)\.(png|jpe?g|webp|gif|avif|bmp|heic)$").ok());
    EXT.as_ref()
        .and_then(|r| r.captures(nome))
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_lowercase().replace("jpeg", "jpg"))
}

fn base_foto(nome: &str) -> String {
    static EXT: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"\.[A-Za-z0-9]+$").ok());
    static RUIM: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"[^A-Za-z0-9_-]+").ok());
    let s = EXT
        .as_ref()
        .map_or(nome.to_owned(), |r| r.replace(nome, "").into_owned());
    let s = RUIM
        .as_ref()
        .map_or(s.clone(), |r| r.replace_all(&s, "-").into_owned());
    let s = s.trim_matches('-');
    let s = if s.is_empty() { "foto" } else { s };
    js::fatia_utf16(s, 60)
}

fn base_arquivo(nome: &str) -> String {
    static BARRAS: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"[/\\]+").ok());
    static RUIM: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"[^A-Za-z0-9_.-]+").ok());
    let s = if nome.is_empty() { "arquivo" } else { nome };
    let s = BARRAS
        .as_ref()
        .map_or(s.to_owned(), |r| r.replace_all(s, "-").into_owned());
    let s = RUIM
        .as_ref()
        .map_or(s.clone(), |r| r.replace_all(&s, "-").into_owned());
    let s = js::fatia_utf16(s.trim_matches('-'), 120);
    if s.is_empty() {
        "arquivo".into()
    } else {
        s
    }
}

/// Corpo do upload: multipart (primeira parte com arquivo), JSON `{b64, nome}` ou cru.
/// Devolve (dados, nome do arquivo) ou a resposta de erro do legado.
fn extrair_upload(
    buf: Vec<u8>,
    ct: &str,
    mut nome: String,
    aceita_json: bool,
) -> Result<(Vec<u8>, String), Box<Response>> {
    if ct.contains("multipart/form-data") {
        let parte = multipart(&buf, &boundary(ct))
            .into_iter()
            .find(|(_, f, _)| f.as_deref().is_some_and(|f| !f.is_empty()));
        match parte {
            Some((_, f, dados)) if !dados.is_empty() => {
                if nome.is_empty() {
                    nome = f.unwrap_or_default();
                }
                Ok((dados, nome))
            }
            _ => Err(Box::new(enviar(
                &json!({ "erro": "nenhum arquivo no form" }),
                400,
            ))),
        }
    } else if aceita_json && ct.contains("application/json") {
        let j: Value = serde_json::from_slice(&buf)
            .map_err(|_| Box::new(enviar(&json!({ "erro": "json inválido" }), 400)))?;
        if j.is_null() {
            // `j.b64` com `j = null` estoura TypeError no legado → catch geral → 500.
            return Err(Box::new(enviar(
                &json!({ "erro": "Cannot read properties of null (reading 'b64')" }),
                500,
            )));
        }
        let b64 = if js::verdade(j.get("b64")) {
            js::texto(j.get("b64"))
        } else {
            String::new()
        };
        if nome.is_empty() && js::verdade(j.get("nome")) {
            nome = js::texto(j.get("nome"));
        }
        Ok((base64_node(&b64), nome))
    } else {
        Ok((buf, nome))
    }
}

async fn gravar(dir: std::path::PathBuf, nome: &str, dados: &[u8]) -> Result<(), Erro> {
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| Erro::novo(500, e.to_string()))?;
    tokio::fs::write(dir.join(nome), dados)
        .await
        .map_err(|e| Erro::novo(500, e.to_string()))
}

/// `POST /api/dossie/foto`: imagem até 30 MB. Na sombra grava em `dados_dir/fotos`.
pub async fn foto(d: &Dossie, p: &Pedido, corpo: Body) -> Result<Response, Erro> {
    if let Some(r) = muitas(d, p, 20, MSG_LIMITE) {
        return Ok(r);
    }
    if let Some(r) = chave_ou_401(d, p) {
        return Ok(r);
    }
    // O teto conta as fotos do legado (é a pasta que o público vê).
    let qtd = std::fs::read_dir(d.fotos_dir()).map_or(0, Iterator::count);
    if qtd >= MAX_FOTOS {
        return Ok(enviar(&json!({ "erro": "limite de fotos atingido" }), 429));
    }
    let ct = p.header("content-type").unwrap_or("").to_owned();
    let nome = p.param("nome").unwrap_or("").to_owned();
    let _vez = UPLOAD
        .acquire()
        .await
        .map_err(|e| Erro::novo(500, e.to_string()))?;
    let buf = ler_corpo(corpo, MAX_FOTO, "arquivo muito grande (máx 30MB)").await?;
    let (dados, nome) = match extrair_upload(buf, &ct, nome, true) {
        Ok(x) => x,
        Err(r) => return Ok(*r),
    };
    if dados.is_empty() {
        return Ok(enviar(&json!({ "erro": "corpo vazio" }), 400));
    }
    let Some(ext) = ext_imagem(&dados, &nome) else {
        return Ok(enviar(
            &json!({ "erro": "não é imagem (png/jpg/webp/gif/avif/bmp/heic)" }),
            415,
        ));
    };
    let arq = format!(
        "foto-{}-{}.{ext}",
        chrono::Utc::now().timestamp_millis(),
        base_foto(&nome)
    );
    gravar(d.dados_dir.join("fotos"), &arq, &dados).await?;
    Ok(enviar(
        &json!({ "ok": true, "nome": arq, "url": format!("/fotos/{arq}"), "bytes": dados.len() }),
        200,
    ))
}

/// `POST /api/dossie/arquivo`: upload genérico. Na sombra grava em `dados_dir/uploads`.
pub async fn arquivo(d: &Dossie, p: &Pedido, corpo: Body) -> Result<Response, Erro> {
    if let Some(r) = muitas(d, p, 10, MSG_LIMITE) {
        return Ok(r);
    }
    if let Some(r) = chave_ou_401(d, p) {
        return Ok(r);
    }
    let ct = p.header("content-type").unwrap_or("").to_owned();
    let nome = p.param("nome").unwrap_or("").to_owned();
    let _vez = UPLOAD
        .acquire()
        .await
        .map_err(|e| Erro::novo(500, e.to_string()))?;
    // O legado usa a mesma mensagem da foto mesmo com teto de 200 MB.
    let buf = ler_corpo(corpo, MAX_ARQUIVO, "arquivo muito grande (máx 30MB)").await?;
    let (dados, nome) = match extrair_upload(buf, &ct, nome, false) {
        Ok(x) => x,
        Err(r) => return Ok(*r),
    };
    if dados.is_empty() {
        return Ok(enviar(&json!({ "erro": "corpo vazio" }), 400));
    }
    let arq = format!(
        "{}-{}",
        chrono::Utc::now().timestamp_millis(),
        base_arquivo(&nome)
    );
    gravar(d.dados_dir.join("uploads"), &arq, &dados).await?;
    Ok(enviar(
        &json!({ "ok": true, "nome": arq, "bytes": dados.len() }),
        200,
    ))
}

/// `DELETE /api/dossie/foto?nome=`: na sombra só apaga o que a sombra gravou; foto que
/// existe só no legado responde `ok` sem apagar (o Nexus não escreve na pasta do legado).
pub async fn apagar_foto(d: &Dossie, p: &Pedido) -> Result<Response, Erro> {
    if let Some(r) = muitas(d, p, 15, "muitas requisições") {
        return Ok(r);
    }
    if let Some(r) = chave_ou_401(d, p) {
        return Ok(r);
    }
    let nome = basename(p.param("nome").unwrap_or("")).to_owned();
    let sombra = d.dados_dir.join("fotos").join(&nome);
    let legado = d.fotos_dir().join(&nome);
    let existe = |c: &std::path::Path| c.is_file() || c.is_dir();
    if !nome.starts_with("foto-") || !(existe(&sombra) || existe(&legado)) {
        return Ok(enviar(&json!({ "erro": "não achou" }), 404));
    }
    if existe(&sombra) {
        tokio::fs::remove_file(&sombra)
            .await
            .map_err(|e| Erro::novo(500, e.to_string()))?;
    }
    Ok(enviar(&json!({ "ok": true }), 200))
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn venda_de_item_pela_descricao() {
        let r = json!({"id": "10", "em": 5, "bruto": 300, "descricao": "3× Mega Stone Fragment", "tipo": "item", "moeda": "orb"});
        let l = linha_venda(&r, 99.0).unwrap();
        assert_eq!(l["id"], json!(10));
        assert_eq!(l["item_id"], json!(70014));
        assert_eq!(l["quantidade"], json!(3));
        assert_eq!(l["lido_em"], json!(99));
        assert_eq!(l["ficha"], Value::Null);
        assert_eq!(l["species_id"], Value::Null);
    }

    #[test]
    fn venda_de_pokemon_tira_apelido() {
        let r = json!({"id": 1, "em": 2, "bruto": 3, "tipo": "pokemon", "itemId": 0,
            "ficha": {"nome": "X", "apelido": "segredo", "speciesId": 25}});
        let l = linha_venda(&r, 1.0).unwrap();
        assert_eq!(l["ficha"], json!({"nome": "X", "speciesId": 25}));
        assert_eq!(l["species_id"], json!(25));
        assert_eq!(l["item_id"], Value::Null);
        assert_eq!(l["quantidade"], json!(1));
        // Ordem das chaves igual ao legado (preserve_order).
        let chaves: Vec<&String> = l.as_object().unwrap().keys().collect();
        assert_eq!(chaves[0], "id");
        assert_eq!(chaves[10], "ficha");
    }

    #[test]
    fn venda_invalida_reprova() {
        assert!(linha_venda(&json!({"id": 0, "em": 1, "bruto": 1}), 1.0).is_none());
        assert!(linha_venda(&json!({"id": "x", "em": 1, "bruto": 1}), 1.0).is_none());
        assert!(linha_venda(&json!(5), 1.0).is_none());
    }

    #[test]
    fn multipart_acha_arquivo() {
        let corpo = b"--XX\r\nContent-Disposition: form-data; name=\"a\"\r\n\r\nv\r\n--XX\r\nContent-Disposition: form-data; name=\"f\"; filename=\"x.png\"\r\n\r\n\x89PNG\r\n--XX--\r\n";
        let partes = multipart(corpo, "XX");
        assert_eq!(partes.len(), 2);
        assert_eq!(partes[1].1.as_deref(), Some("x.png"));
        assert_eq!(partes[1].2, b"\x89PNG");
        assert_eq!(boundary("multipart/form-data; boundary=\"XX\""), "XX");
        assert_eq!(boundary("multipart/form-data; boundary=YY"), "YY");
    }

    #[test]
    fn base64_tolerante() {
        assert_eq!(base64_node("aGk="), b"hi");
        assert_eq!(base64_node("aGk"), b"hi");
        assert_eq!(base64_node("a G\nk"), b"hi");
    }

    #[test]
    fn nomes_saneados() {
        assert_eq!(base_foto("Minha Foto!.PNG"), "Minha-Foto");
        assert_eq!(base_foto("..."), "foto");
        assert_eq!(base_arquivo("a/b c.zip"), "a-b-c.zip");
        assert_eq!(ext_imagem(b"\xFF\xD8x", ""), Some("jpg".into()));
        assert_eq!(ext_imagem(b"zz", "a.JPEG"), Some("jpg".into()));
        assert_eq!(ext_imagem(b"zz", "a.txt"), None);
    }

    #[test]
    fn sql_da_ficha_bate_com_o_legado() {
        let s = SQL_FICHA.as_str();
        assert!(s.contains("$32::bool"));
        assert!(s.contains("$29::text::date"));
        assert!(!s.contains("pokemon_id=COALESCE"));
        assert!(s.contains("nick=COALESCE(EXCLUDED.nick,pokemon_ficha_shadow.nick)"));
    }
}
