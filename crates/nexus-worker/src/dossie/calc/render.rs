//! Porta do `dossie-render.js`: cada função devolve o mesmo HTML do legado, byte a byte.
//! Os textos de modelo (template literals do JS) foram copiados com os mesmos espaços e
//! quebras de linha. `format!` em Rust faz o papel do `${...}` do JS.

use std::collections::HashSet;
use std::f64::consts::PI;

use nucleo::combate::combatente::{Atributos, Combatente};
use nucleo::combate::especie::{Categoria, EspecieCombate};
use nucleo::combate::tipos::{Constantes, Tipo};
use nucleo::js::{numero, round, to_fixed};
use nucleo::motor::veredito::{local_br, nf};
use serde_json::{Map, Value};

use super::sim::{bst, categoria_principal, estagio, lutar, nivel_captura, pct, rotulo, veredito};
use super::{Dados, Entradas};
use crate::dossie::js as jsv;

const TIPOS: [Tipo; 18] = Tipo::TODOS;
/// `PT`: nome em português, na ordem de `TYPES`.
const PT: [&str; 18] = [
    "Normal",
    "Fogo",
    "Água",
    "Planta",
    "Elétrico",
    "Gelo",
    "Lutador",
    "Veneno",
    "Terra",
    "Voador",
    "Psíquico",
    "Inseto",
    "Pedra",
    "Fantasma",
    "Dragão",
    "Sombrio",
    "Metálico",
    "Fada",
];
/// `TC`: cor de cada tipo, na ordem de `TYPES`.
const TC: [&str; 18] = [
    "#a8a596", "#d9774a", "#5d84cf", "#6ba659", "#d6b83a", "#7fc2c2", "#b0483f", "#9a5c9a",
    "#c4a663", "#9a8fd6", "#d06688", "#98a83a", "#a8975a", "#73659a", "#6c58c8", "#6f6258",
    "#9ea3b3", "#d996aa",
];
const STATL: [&str; 6] = ["HP", "ATK", "DEF", "SpATK", "SpDEF", "SPD"];

fn pt(t: Tipo) -> &'static str {
    PT.get(t.indice()).copied().unwrap_or("")
}
fn tc(t: Tipo) -> &'static str {
    TC.get(t.indice()).copied().unwrap_or("")
}

/// Blocos de saída, na ordem em que o legado os cria (`nodes`, um `Map`).
#[derive(Default)]
pub struct Secoes {
    itens: Vec<(&'static str, Option<String>, Option<String>)>,
}

impl Secoes {
    fn html(&mut self, id: &'static str, s: String) {
        self.itens.push((id, Some(s), None));
    }
    fn texto(&mut self, id: &'static str, s: String) {
        self.itens.push((id, None, Some(s)));
    }
    pub fn para_json(&self) -> Value {
        let mut m = Map::new();
        for (id, html, texto) in &self.itens {
            let mut o = Map::new();
            if let Some(h) = html.as_ref().filter(|h| !h.is_empty()) {
                o.insert("html".into(), Value::String(h.clone()));
            }
            if let Some(t) = texto.as_ref().filter(|t| !t.is_empty()) {
                o.insert("text".into(), Value::String(t.clone()));
            }
            if !o.is_empty() {
                m.insert((*id).to_string(), Value::Object(o));
            }
        }
        Value::Object(m)
    }
}

/// `esc`.
fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            c => o.push(c),
        }
    }
    o
}

fn tag_tipo(t: Tipo) -> String {
    format!(
        "<span class=\"type\" style=\"--c:{}\"><i></i>{}</span>",
        tc(t),
        pt(t)
    )
}

fn tag_mult(t: Tipo, m: f64) -> String {
    let mult = if m % 1.0 != 0.0 { nf(m, 1) } else { numero(m) };
    format!(
        "<span class=\"type\" style=\"--c:{}\"><i></i>{}<em>×{}</em></span>",
        tc(t),
        pt(t),
        mult
    )
}

fn tags(ts: &[Tipo]) -> String {
    ts.iter()
        .map(|&t| tag_tipo(t))
        .collect::<Vec<_>>()
        .join(" ")
}

fn tags_juntas(ts: &[Tipo]) -> String {
    ts.iter().map(|&t| tag_tipo(t)).collect::<String>()
}

fn nomes_pt(ts: &[Tipo], sep: &str) -> String {
    ts.iter().map(|&t| pt(t)).collect::<Vec<_>>().join(sep)
}

fn eh_outland(sp: &EspecieCombate) -> bool {
    sp.id >= 2000 && sp.id < 3000
}

/// `origin(sp)`.
fn origem(sp: &EspecieCombate) -> String {
    match sp.categoria.as_deref() {
        Some("mega") => "Mega Stone (10 fragmentos de boss)".into(),
        Some("boss") => "Boss; captura só na Arena Mística".into(),
        Some("arena_mistica") => "Arena Mística (Mystic Ticket)".into(),
        _ => match sp.nivel_captura {
            None => "Evolução ou troca".into(),
            Some(h) => format!(
                "Hunt Nv {}{}",
                nf(h, 0),
                if eh_outland(sp) { " na Outland" } else { "" }
            ),
        },
    }
}

fn atrib(a: &Atributos, i: usize) -> f64 {
    match i {
        0 => a.hp,
        1 => a.atk,
        2 => a.def,
        3 => a.spa,
        4 => a.spd,
        _ => a.spe,
    }
}

fn eff_raw(k: &Constantes, t: Tipo, def: &[Tipo]) -> f64 {
    k.efetividade_bruta(t, def)
}
fn eff_hunt(k: &Constantes, t: Tipo, def: &[Tipo]) -> f64 {
    k.efetividade_hunt(t, def)
}

/// `rodaAmeacas(sp)`.
fn roda_ameacas(sp: &EspecieCombate, k: &Constantes) -> String {
    let (cx, cy, r_grande) = (200.0_f64, 200.0_f64, 150.0_f64);
    let lista: Vec<(Tipo, f64)> = TIPOS
        .iter()
        .map(|&t| (t, eff_raw(k, t, &sp.tipos)))
        .collect();
    let fracos: Vec<Tipo> = lista.iter().filter(|x| x.1 >= 2.0).map(|x| x.0).collect();
    let res: Vec<Tipo> = lista
        .iter()
        .filter(|x| x.1 > 0.0 && x.1 < 1.0)
        .map(|x| x.0)
        .collect();
    let imu: Vec<Tipo> = lista.iter().filter(|x| x.1 == 0.0).map(|x| x.0).collect();
    let mut linhas = String::new();
    let mut nos = String::new();
    for (i, &(t, m)) in lista.iter().enumerate() {
        // `-Math.PI/2 + i*2*Math.PI/18`, na mesma ordem de operações do JS.
        let a = -PI / 2.0 + (i as f64) * 2.0 * PI / 18.0;
        let px = cx + a.cos() * r_grande;
        let py = cy + a.sin() * r_grande;
        let r: f64 = if m >= 4.0 {
            27.0
        } else if m >= 2.0 {
            22.0
        } else if m == 1.0 {
            15.0
        } else if m > 0.0 {
            13.0
        } else {
            11.0
        };
        let cls = if m >= 2.0 {
            "forte"
        } else if m == 1.0 {
            "x1"
        } else if m > 0.0 {
            "res"
        } else {
            "imu"
        };
        let lab = if m >= 4.0 {
            "×4"
        } else if m >= 2.0 {
            "×2"
        } else if m == 1.0 {
            ""
        } else if m == 0.0 {
            "×0"
        } else if m <= 0.25 {
            "¼"
        } else {
            "½"
        };
        let (pxs, pys) = (to_fixed(px, 1), to_fixed(py, 1));
        if m >= 2.0 {
            linhas.push_str(&format!(
                "<line x1=\"{cx}\" y1=\"{cy}\" x2=\"{pxs}\" y2=\"{pys}\" stroke=\"{}\" stroke-width=\"{}\" stroke-opacity=\".55\" stroke-dasharray=\"{}\"/>",
                tc(t),
                if m >= 4.0 { "3" } else { "1.6" },
                if m >= 4.0 { "" } else { "3 5" },
            ));
        }
        let halo = if m >= 2.0 {
            format!(
                "<circle class=\"halo\" cx=\"{pxs}\" cy=\"{pys}\" r=\"{}\" fill=\"{}\" fill-opacity=\".25\"/>",
                numero(r + 9.0),
                tc(t)
            )
        } else {
            String::new()
        };
        let anel = if m < 1.0 {
            format!(
                "<circle cx=\"{pxs}\" cy=\"{pys}\" r=\"{}\" fill=\"none\" stroke=\"var(--ink-3)\" stroke-width=\"1\" stroke-dasharray=\"{}\"/>",
                numero(r + 3.0),
                if m == 0.0 { "2 3" } else { "" }
            )
        } else {
            String::new()
        };
        let nome = pt(t);
        let curto = if nome.chars().count() > 6 {
            format!("{}.", nome.chars().take(5).collect::<String>())
        } else {
            nome.to_string()
        };
        let titulo = if m == 0.0 {
            "não causa dano".to_string()
        } else {
            format!("×{}", numero(m).replace('.', ","))
        };
        let y1 = to_fixed(py + if lab.is_empty() { 3.5 } else { -1.0 }, 1);
        let rotulo2 = if lab.is_empty() {
            String::new()
        } else {
            format!(
                "<text x=\"{pxs}\" y=\"{}\" text-anchor=\"middle\" style=\"font-size:{}px\">{lab}</text>",
                to_fixed(py + if m >= 2.0 { 10.0 } else { 3.5 }, 1),
                if m >= 2.0 { 11 } else { 10 }
            )
        };
        nos.push_str(&format!(
            "<g class=\"no {cls}\"><title>{nome}: {titulo}</title>{halo}{anel}<circle cx=\"{pxs}\" cy=\"{pys}\" r=\"{}\" fill=\"{}\"/>\n      <text x=\"{pxs}\" y=\"{y1}\" text-anchor=\"middle\">{}</text>{rotulo2}</g>",
            numero(r),
            tc(t),
            if m >= 2.0 || m == 1.0 { curto.as_str() } else { "" },
        ));
    }
    let c1: Vec<&str> = sp.tipos.iter().map(|&t| tc(t)).collect();
    let c10 = c1.first().copied().unwrap_or("undefined");
    let c11 = c1.get(1).copied().unwrap_or(c10);
    let imune = if imu.is_empty() {
        String::new()
    } else {
        format!(" · imune a {}", nomes_pt(&imu, ", "))
    };
    let batem = if fracos.is_empty() {
        "nenhum".to_string()
    } else {
        nomes_pt(&fracos, ", ")
    };
    format!(
        "<svg viewBox=\"0 0 400 400\" role=\"img\" aria-label=\"Roda de ameaças: {} tipos batem forte nele, {} batem fraco\">\n    <defs><radialGradient id=\"nucleo\"><stop offset=\"0\" stop-color=\"{c10}\" stop-opacity=\".55\"/><stop offset=\"1\" stop-color=\"{c11}\" stop-opacity=\"0\"/></radialGradient></defs>\n    <circle cx=\"{cx}\" cy=\"{cy}\" r=\"{r_grande}\" fill=\"none\" stroke=\"var(--hair-2)\" stroke-width=\"1\"/>\n    <circle cx=\"{cx}\" cy=\"{cy}\" r=\"{}\" fill=\"none\" stroke=\"var(--hair)\" stroke-width=\"1\" stroke-dasharray=\"2 6\"/>\n    {linhas}\n    <circle cx=\"{cx}\" cy=\"{cy}\" r=\"78\" fill=\"url(#nucleo)\"/>\n    <circle cx=\"{cx}\" cy=\"{cy}\" r=\"56\" fill=\"var(--room)\" fill-opacity=\".85\" stroke=\"{c10}\" stroke-width=\"2\"/>\n    <text class=\"centro\" x=\"{cx}\" y=\"{}\" text-anchor=\"middle\">{}</text>\n    <text class=\"centro2\" x=\"{cx}\" y=\"{}\" text-anchor=\"middle\">AMEAÇAS</text>\n    {nos}</svg>\n    <div class=\"leg\"><span><i style=\"background:var(--bad)\"></i>batem forte: {batem}</span><span><i style=\"background:var(--ink-3)\"></i>resiste a {}{imune}</span></div>",
        fracos.len(),
        res.len() + imu.len(),
        numero(r_grande - 40.0),
        numero(cy + 8.0),
        fracos.len(),
        numero(cy + 28.0),
        res.len(),
    )
}

fn maiuscula_inicial(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(p) => p.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// `renderIdent()`.
fn ident(sp: &EspecieCombate, k: &Constantes) -> String {
    let num = format!("{:0>4}", sp.id);
    let cl = nivel_captura(sp).filter(|&c| c != 0.0 && !c.is_nan());
    let st = estagio(sp);
    let fisico = categoria_principal(sp) == Categoria::Fisico;
    let n600 = sp.golpes.iter().filter(|m| m.poder >= 600.0).count();
    format!(
        "<div class=\"num\" aria-hidden=\"true\">{num}</div><div class=\"tag-num\">Nº {num}{}{}</div>\n   <h1>{}</h1><div class=\"types\">{}</div>\n   <div class=\"roda\">{}</div><dl class=\"facts\"><div><dt>Origem</dt><dd>{}</dd></div><div><dt>Captura no</dt><dd>{}</dd></div><div><dt>BST</dt><dd>{}</dd></div>\n   <div><dt>Ataca com</dt><dd>{}</dd></div>{}<div><dt>Golpes de 600</dt><dd>{n600}</dd></div></dl>",
        if sp.lendaria() { " · Lendário, ×4 nos stats" } else { "" },
        if eh_outland(sp) { " · Variante de Outland, não evolui" } else { "" },
        esc(&sp.nome),
        tags_juntas(&sp.tipos),
        roda_ameacas(sp, k),
        esc(&origem(sp)),
        cl.map_or("—".to_string(), |c| format!("Nv {}", nf(c, 0))),
        numero(bst(sp)),
        if fisico { "Físico, usa ATK" } else { "Especial, usa SpATK" },
        st.map_or(String::new(), |(nome, _)| format!(
            "<div><dt>Estágio</dt><dd>{}</dd></div>",
            maiuscula_inicial(nome)
        )),
    )
}

/// `renderEscada(sp, lv)`.
fn escada(d: &Dados, sp: &EspecieCombate, lv: f64) -> String {
    let cadeia = d
        .evolucoes
        .iter()
        .find(|c| c.iter().any(|(n, _)| n.as_str() == Some(sp.nome.as_str())));
    let Some(cad) = cadeia else {
        let extra = match estagio(sp) {
            Some((nome, _)) => format!(
                "Pelo estágio ({}), a regra geral fora de Kanto é: forma do meio no Nv 40, última forma no Nv 100.",
                esc(nome)
            ),
            None => String::new(),
        };
        return format!(
            "<p class=\"note\">{} não está no guia de evoluções da comunidade (só Kanto e algumas cadeias). {extra}</p>",
            esc(&sp.nome)
        );
    };
    // `x.evolui||"?"` e `x.evolui||0`
    let evolui_txt = |v: &Value| {
        if jsv::verdade(Some(v)) {
            jsv::texto(Some(v))
        } else {
            "?".to_string()
        }
    };
    let evolui_num = |v: &Value| {
        if jsv::verdade(Some(v)) {
            jsv::numero(Some(v))
        } else {
            0.0
        }
    };
    let dd: Vec<(&Value, &Value, &EspecieCombate)> = cad
        .iter()
        .filter_map(|(n, e)| {
            let sp2 = d.motor.dex.por_nome(n.as_str()?)?;
            Some((n, e, sp2))
        })
        .collect();
    let mx = dd
        .iter()
        .map(|x| bst(x.2))
        .fold(f64::NEG_INFINITY, f64::max);
    let i0 = dd
        .iter()
        .position(|x| x.0.as_str() == Some(sp.nome.as_str()));
    let prox = i0.map_or(dd.first(), |i| dd.get(i + 1));
    let mut graus = String::new();
    for (i, (n, e, s2)) in dd.iter().enumerate() {
        let b = bst(s2);
        let h = 60.0 + 140.0 * b / mx;
        let salto = if i > 0 {
            dd.get(i - 1)
                .map(|ant| round((b / bst(ant.2) - 1.0) * 100.0))
        } else {
            None
        };
        let eu = Some(i) == i0;
        graus.push_str(&format!(
            "<div class=\"degrau {}\" style=\"height:{}px;--a:{}%\">{}{}\n      <small>{}</small><span class=\"dn\">{}</span><span class=\"bst\">{}</span><small>BST · {}</small></div>",
            if eu { "eu" } else { "" },
            numero(h),
            10 + 18 * i,
            salto.map_or(String::new(), |s| format!(
                "<span class=\"salto\">Nv {} · +{}%</span>",
                evolui_txt(e),
                numero(s)
            )),
            if eu { format!("<span class=\"pin\">você · Nv {}</span>", nf(lv, 0)) } else { String::new() },
            if i > 0 { format!("evolui no Nv {}", evolui_txt(e)) } else { "base".to_string() },
            esc(jsv::texto(Some(n)).as_str()),
            numero(b),
            nomes_pt(&s2.tipos, "/"),
        ));
    }
    let progresso = match prox {
        Some((pn, pe, ps)) => {
            if lv >= evolui_num(pe) {
                format!(
                    "Seu {} já tem nível para virar <b>{}</b> (Nv {}): o BST sobe {}%.",
                    esc(&sp.nome),
                    esc(&jsv::texto(Some(pn))),
                    jsv::texto(Some(pe)),
                    numero(round((bst(ps) / bst(sp) - 1.0) * 100.0))
                )
            } else {
                format!(
                    "Faltam <b>{} níveis</b> para virar {} (Nv {}).",
                    nf(evolui_num(pe) - lv, 0),
                    esc(&jsv::texto(Some(pn))),
                    jsv::texto(Some(pe))
                )
            }
        }
        None => format!("{} é o último degrau desta cadeia.", esc(&sp.nome)),
    };
    format!(
        "<div class=\"escada\">{graus}</div><div class=\"escada-base\"></div>\n    <p class=\"escada-prog\">{progresso} Fonte: guia da comunidade; confirme a seta \"Precisa de Nv X\" no jogo antes de gastar pedra.</p>"
    )
}

fn sinal(n: f64) -> &'static str {
    if n >= 0.0 {
        "+"
    } else {
        ""
    }
}

/// `renderVerdict`.
fn veredito_html(
    sp: &EspecieCombate,
    me: &Combatente,
    avg: &Combatente,
    top: &Combatente,
    k_idx: usize,
) -> String {
    let dk = pct(atrib(&me.st, k_idx), atrib(&avg.st, k_idx));
    let bulk =
        round((pct(me.hp, avg.hp) + pct(me.st.def, avg.st.def) + pct(me.st.spd, avg.st.spd)) / 3.0);
    let de_top = round(100.0 * atrib(&me.st, k_idx) / atrib(&top.st, k_idx));
    let (chamada, cls) = if dk >= 25.0 {
        ("Fique com ele.", "ok")
    } else if dk >= 8.0 {
        ("Bom. Use e continue caçando um melhor.", "ok")
    } else if dk >= -5.0 {
        ("Na média. Serve até aparecer um melhor.", "warn")
    } else {
        ("Abaixo da média. Mande para a Oferenda ou o NPC.", "bad")
    };
    let sl = STATL.get(k_idx).copied().unwrap_or("");
    format!(
        "<div class=\"verdict\"><div class=\"pct {cls}\">{}{}%</div>\n     <div><p class=\"call\">{chamada}</p><p class=\"muted\" style=\"color:var(--ink-2)\">{sl} do seu contra um {} médio no mesmo nível. É o stat que define o dano dele.</p></div></div>\n   <div class=\"trio\"><div><small>Resistência vs médio</small><b>{}{}%</b></div><div><small>{sl} do máximo possível</small><b>{}%</b></div><div><small>HP de combate</small><b>{}</b></div></div>",
        sinal(dk),
        numero(dk),
        esc(&sp.nome),
        sinal(bulk),
        numero(bulk),
        numero(de_top),
        nf(me.hp, 0),
    )
}

/// `renderStats`.
fn stats_html(me: &Combatente, avg: &Combatente, top: &Combatente, k_idx: usize) -> String {
    let mx = (0..6)
        .map(|i| atrib(&top.st, i))
        .fold(f64::NEG_INFINITY, f64::max);
    let (r, cx, cy) = (110.0_f64, 150.0_f64, 140.0_f64);
    let ponto = |i: usize, f: f64| -> (f64, f64) {
        let a = -PI / 2.0 + (i as f64) * PI / 3.0;
        (cx + a.cos() * r * f, cy + a.sin() * r * f)
    };
    let junta = |(x, y): (f64, f64)| format!("{},{}", numero(x), numero(y));
    let poly = |v: &Combatente| {
        (0..6)
            .map(|i| {
                let f = (atrib(&v.st, i) / atrib(&top.st, i)).max(0.04);
                let (x, y) = ponto(i, f);
                format!("{},{}", to_fixed(x, 1), to_fixed(y, 1))
            })
            .collect::<Vec<_>>()
            .join(" ")
    };
    let grade: String = [1.0, 0.75, 0.5, 0.25]
        .iter()
        .map(|&f| {
            format!(
                "<polygon points=\"{}\" fill=\"none\" stroke=\"var(--hair-2)\" stroke-width=\"1\"/>",
                (0..6).map(|i| junta(ponto(i, f))).collect::<Vec<_>>().join(" ")
            )
        })
        .collect();
    let eixos: String = (0..6)
        .map(|i| {
            let (x, y) = ponto(i, 1.0);
            format!(
                "<line x1=\"{cx}\" y1=\"{cy}\" x2=\"{}\" y2=\"{}\" stroke=\"var(--hair)\"/>",
                numero(x),
                numero(y)
            )
        })
        .collect();
    let rotulos: String = (0..6)
        .map(|i| {
            let (x, y) = ponto(i, 1.2);
            format!(
                "<text x=\"{}\" y=\"{}\" text-anchor=\"middle\" class=\"{}\">{}</text>",
                numero(x),
                numero(y + 4.0),
                if i == k_idx { "k" } else { "" },
                STATL.get(i).copied().unwrap_or("")
            )
        })
        .collect();
    let radar = format!(
        "<svg class=\"radar\" viewBox=\"0 0 300 280\" role=\"img\" aria-label=\"Radar dos stats contra o máximo\"><defs><linearGradient id=\"rg\" x1=\"0\" y1=\"0\" x2=\"1\" y2=\"1\"><stop offset=\"0\" stop-color=\"var(--t1)\"/><stop offset=\"1\" stop-color=\"var(--t2)\"/></linearGradient></defs>\n    {grade}\n    {eixos}\n    <polygon points=\"{}\" fill=\"none\" stroke=\"var(--ink-2)\" stroke-width=\"1.5\" stroke-dasharray=\"4 4\"/>\n    <polygon points=\"{}\" fill=\"url(#rg)\" fill-opacity=\".45\" stroke=\"url(#rg)\" stroke-width=\"2.5\" stroke-linejoin=\"round\"/>\n    {rotulos}</svg>",
        poly(avg),
        poly(me),
    );
    let linhas: String = (0..6)
        .map(|i| {
            let (m, a, t) = (atrib(&me.st, i), atrib(&avg.st, i), atrib(&top.st, i));
            let l = STATL.get(i).copied().unwrap_or("");
            format!(
                "<div class=\"st\"><span class=\"nm {}\">{l}</span>\n    <div class=\"track\" aria-label=\"{l}: seu {}, médio {}, máximo {}\"><span class=\"fill\" style=\"width:{}%\"></span><span class=\"avg\" style=\"left:{}%\"></span><span class=\"top\" style=\"left:calc({}% - 2px)\"></span></div>\n    <span class=\"v\">{}</span></div>",
                if i == k_idx { "key" } else { "" },
                numero(m),
                numero(a),
                numero(t),
                numero(100.0 * m / mx),
                numero(100.0 * a / mx),
                numero(100.0 * t / mx),
                nf(m, 0),
            )
        })
        .collect();
    format!(
        "<div class=\"radar-wrap\">{radar}<div class=\"stats\">{linhas}</div></div><p class=\"note\">Contorno tracejado no radar: exemplar médio. O ponto ao lado do nome marca o stat de ataque dele. Resistência é a média de HP, DEF e SpDEF contra o exemplar médio.</p>"
    )
}

/// `renderImportant`.
fn importante(sp: &EspecieCombate, k_idx: usize, k: &Constantes) -> String {
    let key = STATL.get(k_idx).copied().unwrap_or("");
    let def = if sp.base[2] >= sp.base[4] {
        "DEF"
    } else {
        "SpDEF"
    };
    let fracos: Vec<Tipo> = TIPOS
        .iter()
        .copied()
        .filter(|&t| eff_hunt(k, t, &sp.tipos) >= 2.5)
        .collect();
    // `[...new Set(lista)]`: sem repetição, na ordem da primeira aparição.
    let mut fortes: Vec<Tipo> = Vec::new();
    for t in sp
        .golpes
        .iter()
        .filter(|m| m.poder > 0.0)
        .map(|m| m.tipo)
        .chain(sp.tipos.first().copied())
    {
        if !fortes.contains(&t) {
            fortes.push(t);
        }
    }
    let mut bate_forte: Vec<Tipo> = Vec::new();
    for &t in &fortes {
        for &dft in &TIPOS {
            if eff_raw(k, t, &[dft]) >= 2.0 && !bate_forte.contains(&dft) {
                bate_forte.push(dft);
            }
        }
    }
    let fisico = k_idx == 1;
    let chips_forte = if bate_forte.is_empty() {
        "—".to_string()
    } else {
        bate_forte
            .iter()
            .map(|&t| {
                let m = fortes
                    .iter()
                    .map(|&a| eff_raw(k, a, &[t]))
                    .fold(f64::NEG_INFINITY, f64::max);
                tag_mult(t, m)
            })
            .collect()
    };
    let chips_fraco = if fracos.is_empty() {
        "nenhum tipo".to_string()
    } else {
        fracos
            .iter()
            .map(|&t| tag_mult(t, eff_hunt(k, t, &sp.tipos)))
            .collect()
    };
    format!(
        "<p style=\"margin:0\">Os golpes de dano dele são {}: o <b>{key}</b> (base {}) decide o dano. {}</p>\n   <div class=\"mins\">\n    <div><small>Qualidade mínima</small><b>1,3</b><span>sai em ~26% das capturas, cerca de 4</span></div>\n    <div><small>IV de {key}</small><b>20+</b><span>a média é 16,5</span></div>\n    <div><small>IV de HP e {def}</small><b>16+</b><span>define quanto aguenta antes da poção</span></div>\n    <div><small>IV de SPD</small><b>livre</b><span>só encurta a recarga</span></div>\n   </div>\n   <div><b>Para comparar dois {}</b><ol class=\"prio\"><li>Maior qualidade.</li><li>Empate de qualidade (até 0,05): maior IV de {key}.</li><li>Depois IV de HP e {def}.</li><li>P2 e P3 desempatam. P4, P5 ou shiny passam na frente de tudo.</li></ol></div>\n   <div class=\"mult\">\n    <div class=\"mlab\"><b>Bate forte</b><small>esses tipos apanham ×2+ dos golpes dele</small></div>\n    <div class=\"chips\">{chips_forte}</div>\n    <div class=\"mlab\"><b>Sofre de</b><small>esses tipos batem ×2,5+ nele</small></div>\n    <div class=\"chips\">{chips_fraco}</div>\n   </div>",
        if fisico { "físicos" } else { "especiais" },
        numero(sp.base.get(k_idx).copied().unwrap_or(f64::NAN)),
        if fisico { "SpATK não ajuda em nada." } else { "ATK só pesa na Investida, que é física." },
        esc(&sp.nome),
    )
}

/// `huntPool(lv)`: índices das hunts na faixa de nível.
fn pool_hunts(d: &Dados, lv: f64) -> Vec<usize> {
    let lo = round(lv * 0.6).max(1.0);
    let hi = lv + 10.0;
    let mut pool: Vec<usize> = d
        .especies()
        .iter()
        .enumerate()
        .filter(|(_, s)| {
            s.categoria.as_deref() == Some("pokebola")
                && s.nivel_captura.is_some_and(|h| h >= lo && h <= hi)
        })
        .map(|(i, _)| i)
        .collect();
    if pool.len() > 200 {
        let h = |i: &usize| {
            d.especies()
                .get(*i)
                .and_then(|s| s.nivel_captura)
                .unwrap_or(f64::NAN)
        };
        // `sort` estável do JS: `sort_by` do Rust também é estável.
        pool.sort_by(|a, b| {
            (lv - h(a))
                .abs()
                .partial_cmp(&(lv - h(b)).abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        pool.truncate(200);
    }
    pool
}

/// `renderHunts`: devolve (texto do lede, html da tabela).
fn hunts(d: &Dados, me: &Combatente, k: &Constantes) -> (String, String) {
    let pool = pool_hunts(d, me.nivel);
    let linhas: Vec<(&EspecieCombate, super::sim::Luta)> = pool
        .iter()
        .filter_map(|&i| d.especies().get(i))
        .map(|w| {
            let h = w.nivel_captura.unwrap_or(f64::NAN);
            let foe = Combatente::novo(
                w,
                h,
                &nucleo::combate::combatente::Opcoes {
                    selvagem: true,
                    ..Default::default()
                },
                k,
            );
            (w, lutar(me, &foe, 180.0, k))
        })
        .collect();
    let h_de = |x: &(&EspecieCombate, super::sim::Luta)| x.0.nivel_captura.unwrap_or(f64::NAN);
    let cmp = |a: f64, b: f64| a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal);
    let mut seguras: Vec<&(&EspecieCombate, super::sim::Luta)> =
        linhas.iter().filter(|x| x.1.burst < 0.5 * x.1.hp).collect();
    seguras.sort_by(|a, b| cmp(b.1.kph * h_de(b) * h_de(b), a.1.kph * h_de(a) * h_de(a)));
    let mut ruins: Vec<&(&EspecieCombate, super::sim::Luta)> =
        linhas.iter().filter(|x| x.1.burst >= x.1.hp).collect();
    ruins.sort_by(|a, b| cmp(b.1.burst / b.1.hp, a.1.burst / a.1.hp));
    let lede = format!(
        "{} hunts entre o Nv {} e o Nv {} simuladas com o seu exemplar. {} são seguras (no pior intervalo de 5 s ele leva menos de 50% do HP). No topo ficam as que somam mais abates por hora em hunts de nível mais alto, que pagam mais.",
        linhas.len(),
        nf(round(me.nivel * 0.6).max(1.0), 0),
        nf(me.nivel + 10.0, 0),
        seguras.len(),
    );
    let linha = |x: &(&EspecieCombate, super::sim::Luta)| {
        let (w, r) = (x.0, &x.1);
        let (vv, c) = veredito(r);
        let b = r.burst / r.hp;
        format!(
            "<tr class=\"clk\" data-w=\"{}\" data-l=\"{}\"><td>{}</td><td class=\"r\">{}</td><td>{}</td><td class=\"r\">×{}</td><td class=\"r\">×{}</td><td class=\"r\">{}</td><td><span class=\"mini\"><span style=\"width:{}%;background:var(--{c})\"></span><i></i></span> {}%</td><td><span class=\"pill {c}\">{}</span></td></tr>",
            esc(&w.nome),
            numero(h_de(x)),
            esc(&w.nome),
            nf(h_de(x), 0),
            tags(&w.tipos),
            nf(r.bate, 3),
            nf(r.apanha, 3),
            nf(r.kph, 0),
            numero((b * 100.0 / 1.5).min(100.0)),
            nf(100.0 * b, 0),
            rotulo(vv),
        )
    };
    let cabeca = "<thead><tr><th>Hunt</th><th class=\"r\">Nv</th><th>Tipos</th><th class=\"r\">Bate</th><th class=\"r\">Apanha</th><th class=\"r\">Abates/h</th><th>Pior 5 s</th><th></th></tr></thead>";
    let tabela = |v: &[&(&EspecieCombate, super::sim::Luta)], n: usize| {
        format!(
            "<div class=\"tw\"><table>{cabeca}<tbody>{}</tbody></table></div>",
            v.iter().take(n).map(|x| linha(x)).collect::<String>()
        )
    };
    let mut html = if seguras.is_empty() {
        "<p class=\"note\">Nenhuma hunt segura nessa faixa. Suba o nível dele ou teste uma hunt mais baixa abaixo.</p>".to_string()
    } else {
        tabela(&seguras, 8)
    };
    if !ruins.is_empty() {
        html.push_str(&format!(
            "<p style=\"margin:6px 0 0;font-weight:700\">Evite</p>{}",
            tabela(&ruins, 4)
        ));
    }
    (lede, html)
}

/// `renderBuild`.
fn build(d: &Dados, si: usize, v: &Entradas, k: &Constantes) -> String {
    let Some(sp) = d.especies().get(si) else {
        return String::new();
    };
    let esp = |i: usize| d.especies().get(i);
    let fracos: Vec<Tipo> = TIPOS
        .iter()
        .copied()
        .filter(|&t| eff_hunt(k, t, &sp.tipos) >= 2.5)
        .collect();
    let cego: Vec<Tipo> = TIPOS
        .iter()
        .copied()
        .filter(|&dft| {
            let mut melhor = sp
                .tipos
                .first()
                .map_or(f64::NAN, |&t0| eff_hunt(k, t0, &[dft]));
            for m in sp.golpes.iter().filter(|m| m.poder > 0.0) {
                melhor = melhor.max(eff_hunt(k, m.tipo, &[dft]));
            }
            melhor < 1.0
        })
        .collect();
    let alcance = v.lv.max(150.0);
    let mut escolhidos: Vec<(usize, Vec<Tipo>, Vec<Tipo>)> = Vec::new();
    let mut cobertos_f: HashSet<Tipo> = HashSet::new();
    let mut cobertos_c: HashSet<Tipo> = HashSet::new();
    let mut usados: HashSet<Tipo> = sp.tipos.iter().copied().collect();
    for _ in 0..4 {
        // fraquezas do time inteiro: um tipo fica coberto quando alguém aguenta (×0,333 ou menos)
        let time: Vec<&EspecieCombate> = std::iter::once(sp)
            .chain(escolhidos.iter().filter_map(|c| esp(c.0)))
            .collect();
        let tw: Vec<Tipo> = TIPOS
            .iter()
            .copied()
            .filter(|&t| {
                time.iter().any(|m| eff_hunt(k, t, &m.tipos) >= 2.5)
                    && !time.iter().any(|m| eff_hunt(k, t, &m.tipos) <= 0.333)
            })
            .collect();
        let mut melhor: Option<(usize, f64, Vec<Tipo>, Vec<Tipo>)> = None;
        for &ci in &d.candidatas {
            let Some(c) = esp(ci) else { continue };
            let ch = c.nivel_captura.unwrap_or(f64::NAN);
            if ci == si || escolhidos.iter().any(|e| e.0 == ci) || ch > alcance * 1.5 {
                continue;
            }
            let res: Vec<Tipo> = fracos
                .iter()
                .copied()
                .filter(|&t| eff_hunt(k, t, &c.tipos) <= 0.333 && !cobertos_f.contains(&t))
                .collect();
            let time_res = tw
                .iter()
                .filter(|&&t| eff_hunt(k, t, &c.tipos) <= 0.333)
                .count();
            let nova_fraq = TIPOS
                .iter()
                .filter(|&&t| {
                    eff_hunt(k, t, &c.tipos) >= 2.5
                        && !time.iter().any(|m| eff_hunt(k, t, &m.tipos) <= 0.333)
                })
                .count();
            let off: Vec<Tipo> = cego
                .iter()
                .copied()
                .filter(|&dft| {
                    !cobertos_c.contains(&dft)
                        && (c
                            .tipos
                            .first()
                            .is_some_and(|&t0| eff_hunt(k, t0, &[dft]) >= 2.5)
                            || c.golpes
                                .iter()
                                .any(|m| m.poder > 0.0 && eff_hunt(k, m.tipo, &[dft]) >= 2.5))
                })
                .collect();
            let dup = c.tipos.iter().filter(|t| usados.contains(t)).count();
            let sc = res.len() as f64 * 2.0 + time_res as f64 * 1.2 + off.len() as f64
                - nova_fraq as f64 * 0.6
                - dup as f64 * 1.5
                + (bst(c) - 470.0) / 90.0
                - if ch > alcance { 3.0 } else { 0.0 }
                + 0.0;
            if melhor.as_ref().is_none_or(|m| sc > m.1) {
                melhor = Some((ci, sc, res, off));
            }
        }
        let Some((ci, _, res, off)) = melhor else {
            break;
        };
        cobertos_f.extend(res.iter().copied());
        cobertos_c.extend(off.iter().copied());
        if let Some(c) = esp(ci) {
            usados.extend(c.tipos.iter().copied());
        }
        escolhidos.push((ci, res, off));
    }
    let parceiros: String = escolhidos
        .iter()
        .filter_map(|(ci, res, off)| {
            let c = esp(*ci)?;
            Some(format!(
                "<div class=\"partner\"><b>{}</b><span>{}</span>\n     <div class=\"why\">{} &nbsp;{}{}{}</div></div>",
                esc(&c.nome),
                esc(&origem(c)),
                tags(&c.tipos),
                if res.is_empty() { String::new() } else { format!("Aguenta {}. ", nomes_pt(res, ", ")) },
                if off.is_empty() { String::new() } else { format!("Bate forte em {}.", nomes_pt(off, ", ")) },
                if res.is_empty() && off.is_empty() { format!("Reforço de stats (BST {}).", numero(bst(c))) } else { String::new() },
            ))
        })
        .collect();
    format!(
        "{parceiros}<p class=\"note\">{} A escolha é por tipo e BST. Simule cada parceiro selecionando ele na busca.</p>",
        if fracos.is_empty() {
            "Ele não tem fraqueza forte.".to_string()
        } else {
            format!(
                "Ele sofre de {}; {} desses tipos ficam cobertos.",
                nomes_pt(&fracos, ", "),
                cobertos_f.len()
            )
        }
    )
}

/// `renderMoves`.
fn golpes(sp: &EspecieCombate) -> String {
    let mut ms: Vec<_> = sp.golpes.iter().collect();
    ms.sort_by(|a, b| {
        b.poder
            .partial_cmp(&a.poder)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let linhas: String = ms
        .iter()
        .map(|m| {
            format!(
                "<tr><td>{}{}</td><td>{}</td><td>{}</td><td class=\"r\">{}</td><td class=\"r\">{} s</td></tr>",
                esc(&m.nome),
                if sp.tipos.contains(&m.tipo) {
                    " <span style=\"color:var(--ink-3);font-size:12px\">mesmo tipo, ×1,5</span>"
                } else {
                    ""
                },
                tag_tipo(m.tipo),
                if m.categoria == Categoria::Fisico { "Físico" } else { "Especial" },
                numero(m.poder),
                nf(m.recarga, 1),
            )
        })
        .collect();
    let t0 = sp.tipos.first().copied().map_or(String::new(), tag_tipo);
    format!(
        "<table><thead><tr><th>Golpe</th><th>Tipo</th><th>Categoria</th><th class=\"r\">Poder</th><th class=\"r\">Recarga</th></tr></thead><tbody>\n   {linhas}\n   <tr><td>Investida <span style=\"color:var(--ink-3);font-size:12px\">automática</span></td><td>{t0}</td><td>Físico</td><td class=\"r\">30</td><td class=\"r\">2 s</td></tr></tbody></table>"
    )
}

/// `renderDefCap`.
fn defesa_captura(d: &Dados, si: usize, sp: &EspecieCombate, k: &Constantes) -> String {
    // `bk[e]`: tipos agrupados pela efetividade; depois do `sort`, do maior para o menor.
    let mut grupos: Vec<(f64, Vec<Tipo>)> = Vec::new();
    for &t in &TIPOS {
        let e = eff_hunt(k, t, &sp.tipos);
        match grupos.iter_mut().find(|g| g.0 == e) {
            Some(g) => g.1.push(t),
            None => grupos.push((e, vec![t])),
        }
    }
    grupos.retain(|g| g.0 != 1.0);
    grupos.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let def: String = grupos
        .iter()
        .map(|(e, ts)| {
            format!(
                "<b>×{}</b><div class=\"chips\">{}</div>",
                nf(*e, 3),
                tags_juntas(ts)
            )
        })
        .collect();
    let bolas: [(&str, Option<f64>); 5] = [
        ("Poke Ball", Some(5.0)),
        ("Great Ball", Some(20.0)),
        ("Super Ball", Some(50.0)),
        ("Ultra Ball", Some(130.0)),
        ("Beast Ball", None),
    ];
    let (b, a) = d
        .captura
        .get(si)
        .map(|(b, a)| (b.as_slice(), a.as_slice()))
        .unwrap_or_default();
    let cap = if b.is_empty() {
        "<p class=\"note\">Não é capturado com bola.</p>".to_string()
    } else {
        let linhas: String = bolas
            .iter()
            .enumerate()
            .map(|(i, (n, pr))| {
                let ab = a.get(i).copied().flatten();
                let ouro = match pr {
                    Some(p) => nf(ab.unwrap_or(0.0) * p, 0),
                    None => "diamante".to_string(),
                };
                format!(
                    "<tr><td>{n}</td><td class=\"r\">{}%</td><td class=\"r\">{}</td><td class=\"r\">{ouro}</td></tr>",
                    b.get(i).copied().flatten().map_or("—".to_string(), |x| nf(x, 2)),
                    ab.map_or("—".to_string(), |x| nf(x, 0)),
                )
            })
            .collect();
        format!(
            "<div class=\"tw\"><table><thead><tr><th>Bola</th><th class=\"r\">Chance</th><th class=\"r\">Abates por captura</th><th class=\"r\">Ouro</th></tr></thead><tbody>{linhas}</tbody></table></div>"
        )
    };
    format!(
        "<div><p style=\"margin:0 0 8px;font-weight:600\">Dano que entra nele na hunt</p><div class=\"mult\">{}</div></div><div><p style=\"margin:0 0 8px;font-weight:600\">Captura</p>{cap}</div>",
        if def.is_empty() { "<span>Neutro contra tudo.</span>" } else { def.as_str() }
    )
}

/// Corpo do dossiê (normal): `renderIdent(); recompute();`.
pub fn dossie(d: &Dados, si: usize, v: &Entradas, bosses: String, s: &mut Secoes) {
    let Some(sp) = d.especies().get(si) else {
        return;
    };
    let k = d.motor.modelo.constantes();
    s.html("ident", ident(sp, k));
    let me = Combatente::novo(sp, v.lv, &v.opcoes(), k);
    let avg = Combatente::novo(
        sp,
        v.lv,
        &nucleo::combate::combatente::Opcoes {
            qualidade: 1.1,
            ..Default::default()
        },
        k,
    );
    let top = Combatente::novo(
        sp,
        v.lv,
        &nucleo::combate::combatente::Opcoes {
            qualidade: 1.8,
            ivs: [32.0; 6],
            ..Default::default()
        },
        k,
    );
    let k_idx = if categoria_principal(sp) == Categoria::Fisico {
        1
    } else {
        3
    };
    s.html("verdict", veredito_html(sp, &me, &avg, &top, k_idx));
    s.html("stats", stats_html(&me, &avg, &top, k_idx));
    s.html("imp", importante(sp, k_idx, k));
    s.html("escada", escada(d, sp, v.lv));
    let (lede, tabela) = hunts(d, &me, k);
    s.texto("hunt-lede", lede);
    s.html("hunts", tabela);
    s.html("bosses", bosses);
    s.html("build", build(d, si, v, k));
    s.html("moves", golpes(sp));
    s.html("defcap", defesa_captura(d, si, sp, k));
}

/// `runTest()`: `sp` é o exemplar, `w` o selvagem, com nível e qualidade do teste.
pub fn teste(
    d: &Dados,
    sp: &EspecieCombate,
    v: &Entradas,
    w: &EspecieCombate,
    lv: f64,
    q: f64,
    s: &mut Secoes,
) {
    let k = d.motor.modelo.constantes();
    let me = Combatente::novo(sp, v.lv, &v.opcoes(), k);
    let foe = Combatente::novo(
        w,
        lv,
        &nucleo::combate::combatente::Opcoes {
            qualidade: q,
            selvagem: true,
            ..Default::default()
        },
        k,
    );
    let r = lutar(&me, &foe, 300.0, k);
    let (vv, c) = veredito(&r);
    s.html(
        "t-out",
        format!(
            "<div class=\"vs\" style=\"margin-top:4px\"><div><small>Veredito</small><b style=\"color:var(--{c})\">{}</b></div>\n   <div><small>Tempo por abate</small><b>{}</b></div><div><small>Abates por hora</small><b>{}</b></div><div><small>Pior intervalo de 5 s</small><b>{}% do HP</b></div></div>\n   <p class=\"note\" style=\"margin-top:8px\">{} Nv {} contra {} Nv {}: bate ×{} com {}, apanha até ×{} de {}. Sem poção, cai em {}.</p>",
            rotulo(vv),
            match r.tkill {
                Some(t) if t != 0.0 => format!("{} s", nf(t, 1)),
                _ => "mais de 5 min".to_string(),
            },
            nf(r.kph, 0),
            nf(100.0 * r.burst / r.hp, 0),
            esc(&sp.nome),
            nf(me.nivel, 0),
            esc(&w.nome),
            nf(foe.nivel, 0),
            nf(r.bate, 3),
            pt(r.bate_t),
            nf(r.apanha, 3),
            pt(r.apanha_t),
            if r.t_die.is_finite() { format!("{} s", nf(r.t_die, 0)) } else { "nunca".to_string() },
        ),
    );
}

/// `bossesHTML(sp, v)`: os 6 bosses em que o exemplar entrega mais dano proporcional ao HP.
pub fn bosses_html(d: &Dados, sp: &EspecieCombate, v: &Entradas, vivos: &[Value]) -> String {
    use nucleo::combate::combatente::de_ficha;
    let modelo = &d.motor.modelo;
    let k = modelo.constantes();
    let mut linhas: Vec<(String, f64, f64, f64, f64, f64)> = Vec::new();
    for b in d.motor.lista_bosses(vivos) {
        let Some((foe, _)) = modelo.boss(&b.para_combate(), &d.motor.dex) else {
            continue;
        };
        let ficha = serde_json::json!({
            "speciesId": sp.id, "level": b.team_level, "quality": v.q, "potencia": v.p,
            "shiny": v.sh,
            "ivs": {"hp": v.iv[0], "atk": v.iv[1], "def": v.iv[2], "spAtk": v.iv[3], "spDef": v.iv[4], "speed": v.iv[5]},
        });
        let Some((me, _)) = de_ficha(&d.motor.dex, &ficha, k) else {
            continue;
        };
        let r = modelo.avaliar(&me, &foe, 1.0);
        linhas.push((
            b.nome.clone(),
            b.team_level,
            r.bate.mult,
            r.apanha.mult,
            r.d / foe.hp,
            0.0,
        ));
    }
    linhas.sort_by(|a, b| {
        (b.4 - a.4)
            .partial_cmp(&0.0)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let corpo: String = linhas
        .iter()
        .take(6)
        .map(|(nome, tl, bate, apanha, share, _)| {
            format!(
                "<tr><td>{}</td><td>{}</td><td>×{}</td><td>×{}</td><td>{}%</td></tr>",
                esc(nome),
                numero(*tl),
                numero(*bate),
                numero(*apanha),
                local_br(share * 100.0, 1)
            )
        })
        .collect();
    format!(
        "<div class=\"tw\"><table><thead><tr><th>Boss</th><th>Nv</th><th>Bate</th><th>Apanha</th><th>Dano / HP do boss</th></tr></thead><tbody>{corpo}</tbody></table></div><p class=\"note\">Os 6 bosses em que este exemplar entrega mais dano proporcional ao HP estimado, no nível exigido pelo boss. Considera equipe de 5 nesse nível. HP e golpes do boss são estimados pela espécie; confira a equipe completa na aba Equipes.</p>"
    )
}
