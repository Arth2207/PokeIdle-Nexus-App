//! Arquivos estáticos, `robots.txt` e `sitemap.xml`, como o `handler` do legado.

use std::path::{Path, PathBuf};

use axum::body::Body;
use axum::http::header;
use axum::response::{IntoResponse, Response};

use super::acesso::{chave_ok, privado, publico_ok};
use super::{cru, Dossie, Erro, Pedido};

/// `MIME[ext]` do legado; o resto vai como `application/octet-stream`.
fn mime(arquivo: &Path) -> &'static str {
    let ext = arquivo
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    match ext.as_str() {
        "html" => "text/html; charset=utf-8",
        "js" => "application/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "avif" => "image/avif",
        "bmp" => "image/bmp",
        "heic" => "image/heic",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        _ => "application/octet-stream",
    }
}

/// `path.basename(p)` do Node (ignora a barra final).
pub fn basename(p: &str) -> &str {
    p.trim_end_matches('/').rsplit('/').next().unwrap_or("")
}

/// `path.join(base, p)` do Node: junta e normaliza `.`/barras duplas. `..` já foi removido.
fn juntar(base: &Path, p: &str) -> PathBuf {
    let mut out = base.to_path_buf();
    for seg in p.split('/') {
        if !seg.is_empty() && seg != "." {
            out.push(seg);
        }
    }
    out
}

pub fn robots(p: &Pedido) -> Response {
    let host = p.header("host").unwrap_or("dossie");
    (
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        format!("User-agent: *\nAllow: /\nSitemap: https://{host}/sitemap.xml\n"),
    )
        .into_response()
}

pub async fn sitemap(d: &Dossie) -> Result<Response, Erro> {
    let arq = d.static_dir.join("sitemap.xml");
    let corpo = tokio::fs::read(&arq).await.map_err(|e| erro_fs(&e, &arq))?;
    Ok((
        [(header::CONTENT_TYPE, "application/xml; charset=utf-8")],
        corpo,
    )
        .into_response())
}

/// Mensagem do `fs.readFileSync` do Node para os erros comuns.
fn erro_fs(e: &std::io::Error, arq: &Path) -> Erro {
    let msg = match e.kind() {
        std::io::ErrorKind::NotFound => format!(
            "ENOENT: no such file or directory, open '{}'",
            arq.display()
        ),
        std::io::ErrorKind::PermissionDenied => {
            format!("EACCES: permission denied, open '{}'", arq.display())
        }
        _ => e.to_string(),
    };
    Erro::novo(500, msg)
}

pub async fn servir(d: &Dossie, p: &Pedido) -> Result<Response, Erro> {
    let mut caminho = p.caminho.replace("..", "");
    if caminho == "/" || caminho.is_empty() {
        caminho = "/dossie.html".into();
    }
    if caminho == "/ia.html" && !chave_ok(d, p) {
        return Ok(cru(401, "401 — precisa de ?chave="));
    }
    let mut rel = caminho.trim_start_matches('/').to_owned();
    if rel.is_empty() || rel.ends_with('/') {
        rel.push_str("index.html");
    }
    if rel == "index.html" {
        rel = "dossie.html".into();
    }
    let nao_achou = || cru(404, format!("404 {}", basename(&caminho)));
    if !privado(d, p) && !publico_ok(&rel) {
        return Ok(nao_achou());
    }
    let mut arq = if caminho.starts_with("/fotos/") {
        d.fotos_dir().join(basename(&caminho))
    } else {
        juntar(&d.static_dir, &caminho)
    };
    if tokio::fs::metadata(&arq).await.is_ok_and(|m| m.is_dir()) {
        arq = arq.join("index.html");
    }
    let conteudo = match tokio::fs::read(&arq).await {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(nao_achou()),
        // `existsSync` deu true e o `readFileSync` falhou (permissão, diretório...): o legado
        // cai no catch geral? Não: está fora do try das rotas e derrubaria a requisição.
        // Aqui vira 500 com a mensagem do Node.
        Err(e) => return Err(erro_fs(&e, &arq)),
    };
    Ok((
        [
            (header::CONTENT_TYPE, mime(&arq)),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        Body::from(conteudo),
    )
        .into_response())
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn basename_como_o_node() {
        assert_eq!(basename("/a/b.png"), "b.png");
        assert_eq!(basename("/precificador/"), "precificador");
        assert_eq!(basename("/"), "");
    }

    #[test]
    fn juntar_normaliza() {
        let b = Path::new("/s");
        assert_eq!(juntar(b, "/a//./b.png"), PathBuf::from("/s/a/b.png"));
        assert_eq!(juntar(b, "/"), PathBuf::from("/s"));
    }
}
