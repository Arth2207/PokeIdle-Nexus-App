//! Regras de acesso do legado: chave, lista pública de arquivos e limite por IP.

use std::collections::{HashMap, HashSet};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use regex::Regex;

use super::{Dossie, Pedido};

/// `rateOk(req, lim)`: janela de 60 s por IP, contador compartilhado entre as rotas.
#[derive(Default)]
pub struct Limites(Mutex<HashMap<String, (u32, Instant)>>);

impl Limites {
    pub fn ok(&self, ip: &str, lim: u32) -> bool {
        let agora = Instant::now();
        // Mutex envenenado só se outra thread entrou em pânico segurando; o mapa continua válido.
        let mut hits = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if hits.len() > 20_000 {
            hits.clear();
        }
        match hits.get_mut(ip) {
            Some((n, ts)) if agora.duration_since(*ts) <= Duration::from_secs(60) => {
                *n += 1;
                *n <= lim
            }
            _ => {
                hits.insert(ip.to_owned(), (1, agora));
                true
            }
        }
    }
}

/// Chave em `x-chave` ou `?chave=`, comparada em tempo constante (igual ao legado).
pub fn chave_ok(d: &Dossie, p: &Pedido) -> bool {
    let Some(k) = d.chave.as_deref() else {
        return false;
    };
    let dada = p
        .header("x-chave")
        .filter(|v| !v.is_empty())
        .or_else(|| p.param("chave"))
        .unwrap_or("");
    if dada.len() != k.len() {
        return false;
    }
    dada.bytes()
        .zip(k.bytes())
        .fold(0u8, |acc, (a, b)| acc | (a ^ b))
        == 0
}

/// `privado()`: chave, ou a conexão veio pela porta privada (a do Tailscale no host).
pub fn privado(d: &Dossie, p: &Pedido) -> bool {
    p.rede_privada || chave_ok(d, p)
}

static PUBLICO_HTML: LazyLock<HashSet<&str>> = LazyLock::new(|| {
    HashSet::from([
        "dossie.html",
        "cacada.html",
        "capturas.html",
        "apoie.html",
        "galeria.html",
        "upload.html",
        "dossier.html",
        "precificador/index.html",
    ])
});

static PUBLICO_DEP: LazyLock<HashSet<&str>> = LazyLock::new(|| {
    HashSet::from([
        "sim.js",
        "ficha-parse.js",
        "veredito.js",
        "previsao.js",
        "pokedex.json",
        "evolucoes.json",
        "creatures.json",
        "spa.js",
        "tema.js",
        "tema.css",
        "dex-walls.json",
        "robots.txt",
        "favicon.ico",
        "precificador/app.css",
        "precificador/tema.css",
        "precificador/tema.js",
        "precificador/js/app.js",
        "precificador/js/ficha.js",
    ])
});

// O legado usa `^(?!lixeira\/|raw\/|logs-archive\/)…` com flag `i`. O `regex` do Rust não
// tem lookahead: o prefixo proibido é checado à parte, também sem diferenciar maiúsculas.
// `\w` do JS é ASCII; aqui explícito.
static PUBLICO_IMG: LazyLock<Option<Regex>> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^(?:[A-Za-z0-9_.-]+/)*[A-Za-z0-9_.-]+\.(webp|png|jpe?g|svg|gif|avif|ico|woff2?)$",
    )
    .ok()
});
static PUBLICO_FOTO: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"^fotos/[A-Za-z0-9_.-]+$").ok());

pub fn publico_ok(rel: &str) -> bool {
    if PUBLICO_HTML.contains(rel) || PUBLICO_DEP.contains(rel) {
        return true;
    }
    let minusculo = rel.to_ascii_lowercase();
    let proibido = ["lixeira/", "raw/", "logs-archive/"]
        .iter()
        .any(|p| minusculo.starts_with(p));
    let img = !proibido && PUBLICO_IMG.as_ref().is_some_and(|r| r.is_match(rel));
    img || PUBLICO_FOTO.as_ref().is_some_and(|r| r.is_match(rel))
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn lista_publica_igual_ao_legado() {
        assert!(publico_ok("dossie.html"));
        assert!(publico_ok("precificador/js/app.js"));
        assert!(publico_ok("img/fundo.WEBP"));
        assert!(publico_ok("fotos/foto-1-a.png"));
        assert!(publico_ok("fotos/qualquer"));
        assert!(!publico_ok("lixeira/a.png"));
        assert!(!publico_ok("RAW/a.png"));
        assert!(!publico_ok("ia.html"));
        assert!(!publico_ok("precos-nexus.json"));
        assert!(!publico_ok("a b.png"));
    }

    #[test]
    fn limite_por_ip() {
        let l = Limites::default();
        assert!(l.ok("1", 2));
        assert!(l.ok("1", 2));
        assert!(!l.ok("1", 2));
        assert!(l.ok("2", 2));
    }
}
