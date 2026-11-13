//! Comparador de rotas do dossiê: a mesma requisição no legado e no Nexus, resposta
//! comparada byte a byte (status, content-type e corpo). Grava em `nexus.dossie_paridade`.
//!
//! **Só requisições que não gravam nada no legado:** GETs e POST/DELETE sem chave (param no
//! 401 antes de tocar o banco). `POST /api/dossie/ficha` grava sem chave e fica de fora.
//! As requisições vão com `X-Forwarded-For: nexus-paridade` para usar um balde próprio no
//! limite por IP e não gastar o dos usuários.

use std::time::Duration;

use anyhow::{Context, Result};
use reqwest::Method;
use serde_json::Value;
use sqlx::PgPool;
use tracing::{info, warn};

pub struct Caso {
    pub metodo: Method,
    pub caminho: String,
    /// Consulta com `GROUP BY` sem `ORDER BY` no legado: a ordem das linhas não é garantida
    /// pelo Postgres. Compara com as listas ordenadas (tolerância de ordem, não de conteúdo).
    pub ordem_livre: bool,
}

/// Únicos casos que não são GET. O legado responde 405/401 antes de gravar qualquer coisa
/// (sem corpo, sem chave). **Nunca** pôr aqui `POST /api/dossie/ficha` (grava sem chave)
/// nem `DELETE` (apagaria dado real se a checagem de chave mudasse no legado).
const NAO_GET: &[(Method, &str)] = &[
    (Method::PUT, "/api/dossie/fichas"),
    (Method::POST, "/api/dossie/vendas"),
];

fn caso(metodo: Method, caminho: impl Into<String>) -> Caso {
    Caso {
        metodo,
        caminho: caminho.into(),
        ordem_livre: false,
    }
}

/// Casos fixos + os que dependem dos dados (um nick, uma espécie e uma foto reais).
pub async fn casos(db: &PgPool, http: &reqwest::Client, legado: &str) -> Vec<Caso> {
    let mut v: Vec<Caso> = [
        "/robots.txt",
        "/sitemap.xml",
        "/",
        "/dossie.html",
        "/cacada.html",
        "/precificador/",
        "/tema.css",
        "/sim.js",
        "/pokedex.json",
        "/ia.html",
        "/precos-nexus.json",
        "/nao-existe.png",
        "/lixeira/x.png",
        "/api/dossie/fotos",
        "/api/dossie/fichas",
        "/api/dossie/fichas?limit=5",
        "/api/dossie/fichas?limit=abc",
        "/api/dossie/especie?slug=nao-existe",
        "/api/dossie/dataset",
        "/api/dossie/cacada-medidas",
        "/api/ia/estado",
        "/precificador/precos.json",
    ]
    .into_iter()
    .map(|c| caso(Method::GET, c))
    .collect();
    v.extend(NAO_GET.iter().map(|(m, c)| caso(m.clone(), *c)));
    v.push(Caso {
        metodo: Method::GET,
        caminho: "/api/dossie/cacada-medidas?esp=x".into(),
        ordem_livre: true,
    });

    // Amostras reais: uma espécie das fichas, quem capturou e um nick de conta (este
    // exercita o caminho "medidas da conta" da caçada).
    let um = |sql: &'static str| async move {
        sqlx::query_scalar::<_, String>(sql)
            .fetch_optional(db)
            .await
            .unwrap_or(None)
    };
    if let Some(slug) = um("SELECT slug FROM public.pokemon_ficha WHERE slug IS NOT NULL ORDER BY ts_salvo DESC LIMIT 1").await {
        v.push(caso(
            Method::GET,
            format!("/api/dossie/especie?slug={}&score=50&iv=100&q=1.2", codificar(&slug)),
        ));
    }
    if let Some(quem) = um("SELECT capturado_por FROM public.pokemon_ficha WHERE capturado_por IS NOT NULL ORDER BY ts_salvo DESC LIMIT 1").await {
        let q = codificar(&quem);
        v.push(caso(Method::GET, format!("/api/dossie/fichas?nick={q}")));
        v.push(caso(Method::GET, format!("/api/dossie/fichas?nicks={q},x")));
    }
    if let Some(nick) =
        um("SELECT nick FROM public.dim_conta WHERE nick IS NOT NULL ORDER BY conta_id LIMIT 1")
            .await
    {
        v.push(Caso {
            metodo: Method::GET,
            caminho: format!("/api/dossie/cacada-medidas?esp=x&nick={}", codificar(&nick)),
            ordem_livre: true,
        });
    }
    if let Some(foto) = primeira_foto(http, legado).await {
        v.push(caso(Method::GET, format!("/fotos/{}", codificar(&foto))));
    }
    v
}

fn codificar(s: &str) -> String {
    form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

async fn primeira_foto(http: &reqwest::Client, legado: &str) -> Option<String> {
    let r = http
        .get(format!("{legado}/api/dossie/fotos"))
        .header("x-forwarded-for", "nexus-paridade")
        .send()
        .await
        .ok()?;
    let v: Value = serde_json::from_slice(&r.bytes().await.ok()?).ok()?;
    v["fotos"][0]["nome"].as_str().map(str::to_owned)
}

/// Resposta resumida: status, content-type e corpo.
#[derive(PartialEq, Debug)]
struct Resposta {
    status: u16,
    tipo: Option<String>,
    corpo: Vec<u8>,
}

async fn buscar(http: &reqwest::Client, base: &str, c: &Caso) -> Result<Resposta> {
    let r = http
        .request(c.metodo.clone(), format!("{base}{}", c.caminho))
        .header("x-forwarded-for", "nexus-paridade")
        // Mesmo Host nos dois: o robots.txt ecoa o Host (a porta mudaria o corpo).
        .header("host", "dossie")
        .send()
        .await
        .with_context(|| format!("{} {}{}", c.metodo, base, c.caminho))?;
    let status = r.status().as_u16();
    let tipo = r
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let corpo = r.bytes().await?.to_vec();
    Ok(Resposta {
        status,
        tipo,
        corpo,
    })
}

/// Ordena recursivamente as listas (para os casos de `ordem_livre`).
fn normalizar(v: &mut Value) {
    match v {
        Value::Array(a) => {
            a.iter_mut().for_each(normalizar);
            a.sort_by_key(Value::to_string);
        }
        Value::Object(o) => o.values_mut().for_each(normalizar),
        _ => {}
    }
}

/// Primeiro caminho JSON onde os dois diferem.
fn diferenca(a: &Value, b: &Value, caminho: &str) -> Option<String> {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            let kx: Vec<&String> = x.keys().collect();
            let ky: Vec<&String> = y.keys().collect();
            if kx != ky {
                return Some(format!("{caminho}: chaves {kx:?} ≠ {ky:?}"));
            }
            x.iter()
                .find_map(|(k, v)| diferenca(v, &y[k], &format!("{caminho}.{k}")))
        }
        (Value::Array(x), Value::Array(y)) => {
            if x.len() != y.len() {
                return Some(format!("{caminho}: {} ≠ {} itens", x.len(), y.len()));
            }
            x.iter()
                .zip(y)
                .enumerate()
                .find_map(|(i, (p, q))| diferenca(p, q, &format!("{caminho}[{i}]")))
        }
        _ if a != b => Some(format!("{caminho}: {a} ≠ {b}")),
        _ => None,
    }
}

/// Compara as duas respostas. `None` = iguais; `Some(motivo)` = divergem.
fn comparar(c: &Caso, node: &Resposta, nexus: &Resposta) -> Option<String> {
    if node.status != nexus.status {
        return Some(format!("status {} ≠ {}", node.status, nexus.status));
    }
    if node.tipo != nexus.tipo {
        return Some(format!("content-type {:?} ≠ {:?}", node.tipo, nexus.tipo));
    }
    if node.corpo == nexus.corpo {
        return None;
    }
    let (Ok(mut a), Ok(mut b)) = (
        serde_json::from_slice::<Value>(&node.corpo),
        serde_json::from_slice::<Value>(&nexus.corpo),
    ) else {
        return Some(format!(
            "corpo difere ({} ≠ {} bytes)",
            node.corpo.len(),
            nexus.corpo.len()
        ));
    };
    if c.ordem_livre {
        normalizar(&mut a);
        normalizar(&mut b);
        if a == b {
            return None;
        }
    }
    Some(diferenca(&a, &b, "$").unwrap_or_else(|| "formatação do JSON difere".into()))
}

/// Uma rodada: compara todos os casos e grava. Divergência em dado ao vivo (ficha salva
/// entre as duas requisições) pode acontecer: repete o caso uma vez antes de gravar.
pub async fn ciclo(
    db: &PgPool,
    http: &reqwest::Client,
    legado: &str,
    nexus: &str,
) -> Result<usize> {
    let casos = casos(db, http, legado).await;
    let mut divergentes = 0;
    for c in &casos {
        let mut resultado = None;
        for _ in 0..2 {
            let node = buscar(http, legado, c).await;
            let nx = buscar(http, nexus, c).await;
            resultado = Some(match (node, nx) {
                (Ok(a), Ok(b)) => (a.status, b.status, comparar(c, &a, &b)),
                (Err(e), _) => (0, 0, Some(format!("legado: {e:#}"))),
                (_, Err(e)) => (0, 0, Some(format!("nexus: {e:#}"))),
            });
            if matches!(resultado, Some((_, _, None))) {
                break;
            }
        }
        let Some((st_node, st_nexus, motivo)) = resultado else {
            continue;
        };
        if motivo.is_some() {
            divergentes += 1;
        }
        sqlx::query(
            "INSERT INTO nexus.dossie_paridade (rota, status_node, status_nexus, igual, detalhe)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(format!("{} {}", c.metodo, c.caminho))
        .bind(i32::from(st_node))
        .bind(i32::from(st_nexus))
        .bind(motivo.is_none())
        .bind(
            motivo
                .as_deref()
                .map(|m| m.chars().take(500).collect::<String>()),
        )
        .execute(db)
        .await?;
    }
    info!(
        casos = casos.len(),
        divergentes, "dossiê paridade: ciclo ok"
    );
    Ok(divergentes)
}

/// Laço do comparador (serviço `nexus-dossie-paridade`, rede do host).
pub async fn rodar(db: PgPool, legado: String, nexus: String, intervalo: Duration) -> Result<()> {
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .context("cliente HTTP")?;
    loop {
        if let Err(e) = ciclo(&db, &http, &legado, &nexus).await {
            warn!(erro = %e, "dossiê paridade: ciclo falhou");
        }
        tokio::select! {
            _ = tokio::time::sleep(intervalo) => {}
            _ = tokio::signal::ctrl_c() => return Ok(()),
        }
    }
}

/// (rota, status legado, status Nexus, igual, detalhe, total, divergentes)
type LinhaRelatorio = (String, i32, i32, bool, Option<String>, i64, i64);

/// Última comparação de cada rota nas últimas `horas` horas.
pub async fn relatorio(db: &PgPool, horas: u32) -> Result<()> {
    let linhas: Vec<LinhaRelatorio> = sqlx::query_as(
        "SELECT DISTINCT ON (rota) rota, status_node, status_nexus, igual, detalhe,
                COUNT(*) OVER (PARTITION BY rota),
                COUNT(*) FILTER (WHERE NOT igual) OVER (PARTITION BY rota)
           FROM nexus.dossie_paridade
          WHERE em >= NOW() - ($1::int * interval '1 hour')
          ORDER BY rota, em DESC",
    )
    .bind(horas as i32)
    .fetch_all(db)
    .await?;
    if linhas.is_empty() {
        println!("sem dados nas últimas {horas} horas");
        return Ok(());
    }
    for (rota, sn, sx, igual, detalhe, total, ruins) in linhas {
        let marca = if igual { "✓" } else { "✗" };
        println!(
            "{marca} {rota:<60} {sn}/{sx}  divergiu {ruins}/{total}  {}",
            detalhe.unwrap_or_default()
        );
    }
    Ok(())
}

#[cfg(test)]
mod testes {
    use super::*;
    use serde_json::json;

    fn r(corpo: &str) -> Resposta {
        Resposta {
            status: 200,
            tipo: Some("application/json; charset=utf-8".into()),
            corpo: corpo.as_bytes().to_vec(),
        }
    }

    /// Trava de segurança: o comparador nunca pode causar escrita no legado.
    #[test]
    fn comparador_nao_grava_no_legado() {
        for (m, c) in NAO_GET {
            assert_ne!(*m, Method::DELETE);
            assert_ne!(*m, Method::PATCH);
            assert!(!c.starts_with("/api/dossie/ficha?") && *c != "/api/dossie/ficha");
            assert!(!c.contains("foto") && !c.contains("arquivo"));
        }
    }

    #[test]
    fn compara_bytes_e_aponta_o_campo() {
        let c = caso(Method::GET, "/x");
        assert_eq!(comparar(&c, &r("{\"a\":1}"), &r("{\"a\":1}")), None);
        assert_eq!(
            comparar(&c, &r("{\"a\":[1,2]}"), &r("{\"a\":[1,3]}")),
            Some("$.a[1]: 2 ≠ 3".into())
        );
        assert_eq!(
            comparar(&c, &r("{\"a\":1}"), &r("{ \"a\": 1 }")),
            Some("formatação do JSON difere".into())
        );
    }

    #[test]
    fn ordem_livre_so_ignora_ordem() {
        let c = Caso {
            metodo: Method::GET,
            caminho: "/x".into(),
            ordem_livre: true,
        };
        assert_eq!(comparar(&c, &r("{\"m\":[1,2]}"), &r("{\"m\":[2,1]}")), None);
        assert!(comparar(&c, &r("{\"m\":[1,2]}"), &r("{\"m\":[2,3]}")).is_some());
        let mut v = json!([{"b": 2}, {"a": 1}]);
        normalizar(&mut v);
        assert_eq!(v, json!([{"a": 1}, {"b": 2}]));
    }
}
