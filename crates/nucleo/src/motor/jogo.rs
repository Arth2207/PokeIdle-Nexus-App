//! Leitura comum do estado do jogo que as regras usam (`num`, `janela`, `huntAtual`,
//! `slugDe`, `norm` do `motor.js`).

use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

use super::regras::ContaViva;
use crate::js;

/// `LINE`: linha de abate no HUD.
pub static LINHA_ABATE: LazyLock<Option<Regex>> = LazyLock::new(|| {
    Regex::new(r"^(.+?) derrotado! \+([\d.]+) xp treinador, \+([\d.]+) xp pokemon, \+([\d.]+) ouro")
        .ok()
});
static NUMERO: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"-?\d+(\.\d+)?").ok());
static CAPTURA_OU_FUGA: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"(?i)escapou e sumiu|captur\w*:").ok());
static POCAO: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"(?i)po[cç][aã]o usada").ok());
static DESMAIO: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"(?i)desmai").ok());
static HUNT: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"(?i)^(.+?)\s*·\s*nível\s*([\d.]+)").ok());

/// `re.test(texto)` com a regex compilada (sempre compila; `None` só por defesa).
pub fn casa(re: &LazyLock<Option<Regex>>, texto: &str) -> bool {
    re.as_ref().is_some_and(|r| r.is_match(texto))
}

/// `num(s)`: número escrito à brasileira (`1.234,5`, `−3`). `null`/`undefined` e texto sem
/// número dão `None`.
pub fn num(v: Option<&Value>) -> Option<f64> {
    let s = match v {
        None | Some(Value::Null) => return None,
        Some(x) => js::texto(Some(x)),
    };
    num_texto(&s)
}

/// [`num`] de um texto.
pub fn num_texto(s: &str) -> Option<f64> {
    // `.replace(/−/g, '-').replace(/\./g, '').replace(',', '.')`: só a primeira vírgula.
    let limpo = s.replace('−', "-").replace('.', "").replacen(',', ".", 1);
    let m = NUMERO.as_ref()?.find(&limpo)?;
    m.as_str().parse().ok()
}

/// XP base publicado pelo jogo, antes dos bônus e do arredondamento final.
pub fn xp_base(n: f64) -> f64 {
    if n <= 150.0 {
        (0.6 * n * n).floor() + 8.0
    } else {
        (13500.0 * (n / 150.0).powf(1.25)).round()
    }
}

/// `janela(conta, ms, ate)`: ritmo da conta nas linhas do HUD entre `ate - ms` e `ate`.
#[derive(Debug, Clone, PartialEq)]
pub struct Janela {
    pub kills: usize,
    pub capturas: usize,
    /// Abates por hora (`null` com menos de 3 abates ou menos de 1 min coberto).
    pub kph: Option<f64>,
    pub span: f64,
    pub xp_kill: Option<f64>,
    pub xp_pk_kill: Option<f64>,
    pub gold_kill: Option<f64>,
    pub pot_kill: Option<f64>,
    pub bolas_kill: Option<f64>,
    pub desm: u64,
    pub xp_h: Option<f64>,
    pub xp_pk_h: Option<f64>,
}

pub fn janela(conta: &ContaViva, ms: f64, ate: f64, agora: f64) -> Janela {
    let desde = ate - ms;
    let mut capturas = 0;
    let (mut kills, mut xp, mut xp_pk, mut gold, mut pot, mut bolas, mut desm) =
        (0usize, 0.0, 0.0, 0.0, 0u64, 0u64, 0u64);
    for (_, _, x) in conta
        .linhas
        .iter()
        .filter(|l| (l.1 as f64) >= desde && (l.1 as f64) <= ate)
    {
        if let Some(m) = LINHA_ABATE.as_ref().and_then(|r| r.captures(x)) {
            kills += 1;
            // `o.xp += num(m[2])`: `null` soma 0.
            xp += m.get(2).and_then(|g| num_texto(g.as_str())).unwrap_or(0.0);
            xp_pk += m.get(3).and_then(|g| num_texto(g.as_str())).unwrap_or(0.0);
            gold += m.get(4).and_then(|g| num_texto(g.as_str())).unwrap_or(0.0);
            continue;
        }
        if casa(&CAPTURA_OU_FUGA, x) {
            bolas += 1;
            if !x.to_lowercase().contains("escapou e sumiu") {
                capturas += 1;
            }
        } else if casa(&POCAO, x) {
            pot += 1;
        } else if casa(&DESMAIO, x) {
            desm += 1;
        }
    }
    let primeira = conta.linhas.first().map_or(desde, |l| l.1 as f64);
    let ini = desde.max(primeira);
    let span = ate.min(agora) - ini;
    let kph = (kills >= 3 && span >= 60_000.0).then(|| kills as f64 * 3.6e6 / span);
    let por = |v: f64| (kills > 0).then(|| v / kills as f64);
    Janela {
        kills,
        capturas,
        kph,
        span,
        xp_kill: por(xp),
        xp_pk_kill: por(xp_pk),
        gold_kill: por(gold),
        pot_kill: por(pot as f64),
        bolas_kill: por(bolas as f64),
        desm,
        xp_pk_h: kph.map(|k| xp_pk / kills as f64 * k),
        xp_h: kph
            .filter(|k| *k != 0.0 && kills > 0)
            .map(|k| xp / kills as f64 * k),
    }
}

/// `huntAtual(conta)`: nome e nível da hunt pelo texto `ultimo.hunt` ("Nome · nível 500").
pub fn hunt_atual(conta: &ContaViva) -> Option<(String, Option<f64>)> {
    if let Some(slug) = conta
        .ultimo
        .get("huntSlug")
        .or_else(|| conta.ultimo.pointer("/io/player/huntSlug"))
        .and_then(Value::as_str)
    {
        if let Some(h) = conta
            .welcome
            .get("hunts")
            .and_then(Value::as_array)
            .and_then(|hs| hs.iter().find(|h| h["slug"].as_str() == Some(slug)))
        {
            if let Some(nome) = h["nome"].as_str() {
                return Some((nome.into(), num(h.get("nivel"))));
            }
        }
    }
    let texto = conta.ultimo.get("hunt").map(|v| js::texto(Some(v)))?;
    let c = HUNT.as_ref()?.captures(&texto)?;
    Some((
        c.get(1)?
            .as_str()
            .split(" · Outland")
            .next()?
            .trim()
            .to_owned(),
        num_texto(c.get(2)?.as_str()),
    ))
}

/// `norm(s)`: sem acento, minúsculo, só letras e dígitos ASCII.
pub fn norm(v: Option<&Value>) -> String {
    let s = match v {
        None | Some(Value::Null) => String::new(),
        Some(x) if !js::verdadeiro(Some(x)) => String::new(),
        Some(x) => js::texto(Some(x)),
    };
    norm_texto(&s)
}

pub fn norm_texto(s: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    s.nfd()
        .filter(|c| !('\u{300}'..='\u{36f}').contains(c))
        .collect::<String>()
        .to_lowercase()
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect()
}

/// `slugDe(conta, nome, lvl)`: o slug da hunt pelo nome (e nível, se a hunt tiver), ou pelo
/// próprio slug.
pub fn slug_de(conta: &ContaViva, nome: &str, lvl: Option<f64>) -> Option<String> {
    let hunts = conta.welcome.get("hunts").and_then(Value::as_array)?;
    let alvo = norm_texto(nome);
    let nivel_ok = |h: &Value| match h.get("nivel") {
        None | Some(Value::Null) => true,
        n => lvl.is_some_and(|l| js::mais(n) == l),
    };
    hunts
        .iter()
        .find(|h| norm(h.get("nome")) == alvo && nivel_ok(h))
        .or_else(|| hunts.iter().find(|h| norm(h.get("slug")) == alvo))
        .and_then(|h| h.get("slug"))
        .map(|s| js::texto(Some(s)))
}

#[cfg(test)]
mod testes {
    use super::*;
    use serde_json::json;

    fn conta(linhas: &[(i64, &str)]) -> ContaViva {
        ContaViva {
            nick: "Ana".into(),
            linhas: linhas
                .iter()
                .enumerate()
                .map(|(i, (t, x))| (i as i64, *t, (*x).to_owned()))
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn num_como_o_legado() {
        assert_eq!(num_texto("1.234,5 ouro"), Some(1234.5));
        assert_eq!(num_texto("−3"), Some(-3.0));
        assert_eq!(num_texto("abc"), None);
        assert_eq!(num(Some(&json!(null))), None);
        assert_eq!(num(Some(&json!(12))), Some(12.0));
    }

    #[test]
    fn janela_conta_abates_e_ritmo() {
        let abate = "Wurmple derrotado! +1.000 xp treinador, +50 xp pokemon, +2.000 ouro";
        let c = conta(&[
            (0, "início"),
            (60_000, abate),
            (120_000, abate),
            (180_000, "Poção usada"),
            (240_000, abate),
            (250_000, "Wurmple escapou e sumiu"),
        ]);
        let j = janela(&c, 600_000.0, 300_000.0, 300_000.0);
        assert_eq!(j.kills, 3);
        assert_eq!(j.span, 300_000.0);
        assert_eq!(j.kph, Some(36.0));
        assert_eq!(j.xp_kill, Some(1000.0));
        assert_eq!(j.gold_kill, Some(2000.0));
        assert_eq!(j.xp_h, Some(36_000.0));
        assert_eq!(j.xp_pk_h, Some(1800.0));
        assert_eq!(j.xp_pk_kill, Some(50.0));
        assert_eq!(j.bolas_kill, Some(1.0 / 3.0));
    }

    #[test]
    fn hunt_e_slug() {
        let c = ContaViva {
            ultimo: json!({"hunt": "Silcoon · nível 650"}),
            welcome: json!({"hunts": [
                {"slug": "silcoon-500", "nome": "Silcoon", "nivel": 500},
                {"slug": "silcoon-650", "nome": "Silcoon", "nivel": 650},
                {"slug": "pokémon-x", "nome": "Outro"}
            ]}),
            ..Default::default()
        };
        assert_eq!(hunt_atual(&c), Some(("Silcoon".into(), Some(650.0))));
        assert_eq!(
            slug_de(&c, "Silcoon", Some(650.0)).as_deref(),
            Some("silcoon-650")
        );
        assert_eq!(slug_de(&c, "Pokemon X", None).as_deref(), Some("pokémon-x"));
        assert_eq!(norm_texto("Pokémon X!"), "pokemonx");
    }
}
