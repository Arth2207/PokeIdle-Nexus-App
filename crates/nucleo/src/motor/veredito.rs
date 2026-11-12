//! `web/veredito.js` do legado: a regra de guardar ou dispensar capturas. É a mesma que a
//! página Capturas usa; aqui ela alimenta a regra de vender do motor.
//!
//! A pokédex (`BYD` do JS) chega por parâmetro como [`Especies`]: o tipo `Pokedex` da fase 2
//! mais o preço base de NPC (`p` do `pokedex.json`), que `EspecieCombate` não guarda.
//! Sem I/O.

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use serde_json::Value;

use crate::combate::combatente::{Combatente, Opcoes};
use crate::combate::especie::{Categoria, EspecieCombate, Pokedex};
use crate::combate::tipos::Constantes;
use crate::js;

/// `STATN` do `sim.js`.
pub const STAT_N: [&str; 6] = ["hp", "atk", "def", "spa", "spd", "spe"];
/// `STATL` do `sim.js`.
pub const STAT_L: [&str; 6] = ["HP", "ATK", "DEF", "SpATK", "SpDEF", "SPD"];

static CONSTANTES: LazyLock<Constantes> = LazyLock::new(Constantes::wiki);

/// `BYD` do legado: espécies por id, com o preço base de NPC (`sp.p`).
#[derive(Debug, Clone, Copy)]
pub struct Especies<'a> {
    pub dex: &'a Pokedex,
    precos: Option<&'a HashMap<i64, f64>>,
}

impl<'a> Especies<'a> {
    /// Sem preços de NPC (`sp.p` vale 0 em todas).
    pub fn sem_precos(dex: &'a Pokedex) -> Self {
        Self { dex, precos: None }
    }

    pub fn com_precos(dex: &'a Pokedex, precos: &'a HashMap<i64, f64>) -> Self {
        Self {
            dex,
            precos: Some(precos),
        }
    }

    /// Lê o campo `p` de cada espécie do `pokedex.json` cru (lista de objetos).
    pub fn precos_de_json(bruta: &Value) -> HashMap<i64, f64> {
        let mut m = HashMap::new();
        if let Value::Array(a) = bruta {
            for e in a {
                if let (Some(d), Some(p)) = (
                    e.get("d").and_then(Value::as_i64),
                    e.get("p").and_then(Value::as_f64),
                ) {
                    m.insert(d, p);
                }
            }
        }
        m
    }

    /// `BYD.get(+speciesId)`: só acha quando o número é um inteiro.
    pub fn por_id(&self, species_id: Option<&Value>) -> Option<&'a EspecieCombate> {
        let f = js::mais(species_id);
        (f.is_finite() && f.fract() == 0.0 && f.abs() < 9e15)
            .then(|| self.dex.por_id(f as i64))
            .flatten()
    }

    /// `sp.p || 0`.
    pub fn preco(&self, id: i64) -> f64 {
        let p = self.precos.and_then(|m| m.get(&id)).copied().unwrap_or(0.0);
        if p.is_nan() {
            0.0
        } else {
            p
        }
    }
}

// ---------- formatação ----------

/// `x.toLocaleString("pt-BR", {maximumFractionDigits: d})`: milhar com ponto, vírgula decimal,
/// arredondamento "meio para longe do zero" sobre a representação decimal mais curta do
/// número (como o ICU), sem zeros à direita. `NaN` vira "NaN" e o infinito "∞".
pub fn local_br(x: f64, d: usize) -> String {
    if x.is_nan() {
        return "NaN".into();
    }
    let sinal = if x.is_sign_negative() { "-" } else { "" };
    if x.is_infinite() {
        return format!("{sinal}∞");
    }
    let s = format!("{}", x.abs());
    let (inteira, fracao) = match s.split_once('.') {
        Some((a, b)) => (a.to_owned(), b.to_owned()),
        None => (s.clone(), String::new()),
    };
    let mut n_int = inteira.len();
    let mut dig: Vec<u8> = inteira
        .bytes()
        .chain(fracao.bytes().take(d))
        .map(|b| b.wrapping_sub(b'0'))
        .collect();
    if fracao.as_bytes().get(d).is_some_and(|&b| b >= b'5') {
        let mut i = dig.len();
        let mut subiu = false;
        while i > 0 {
            i -= 1;
            if let Some(c) = dig.get_mut(i) {
                if *c == 9 {
                    *c = 0;
                } else {
                    *c += 1;
                    subiu = true;
                    break;
                }
            }
        }
        if !subiu {
            dig.insert(0, 1);
            n_int += 1;
        }
    }
    let (parte_int, parte_frac) = dig.split_at(n_int.min(dig.len()));
    let mut texto = String::new();
    for (i, c) in parte_int.iter().enumerate() {
        if i > 0 && (parte_int.len() - i) % 3 == 0 {
            texto.push('.');
        }
        texto.push(char::from(b'0' + c));
    }
    let mut f: Vec<u8> = parte_frac.to_vec();
    while f.last() == Some(&0) {
        f.pop();
    }
    if !f.is_empty() {
        texto.push(',');
        texto.extend(f.iter().map(|c| char::from(b'0' + c)));
    }
    format!("{sinal}{texto}")
}

/// `nf(x, d)` do veredito: "—" para `null` ou não finito.
pub fn nf(x: f64, d: usize) -> String {
    if x.is_finite() {
        local_br(x, d)
    } else {
        "—".into()
    }
}

pub fn nf_opt(x: Option<f64>, d: usize) -> String {
    x.map_or_else(|| "—".into(), |v| nf(v, d))
}

/// `sg(n)`: o número com `+` na frente quando não é negativo.
fn sg(n: f64) -> String {
    format!("{}{}", if n >= 0.0 { "+" } else { "" }, js::numero(n))
}

/// `Math.min` com `NaN` contagiante.
fn min_js(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else {
        a.min(b)
    }
}

/// `pc(a, b)`: `Math.round((a / b - 1) * 100)`.
fn pc(a: f64, b: f64) -> f64 {
    js::round((a / b - 1.0) * 100.0)
}

/// `Array.prototype.sort` do V8 para listas curtas (inserção binária, estável), como
/// `ordenar_como_js` de `leitura.rs`.
pub(crate) fn ordenar_como_js<T>(v: &mut Vec<T>, cmp: impl Fn(&T, &T) -> f64) {
    for i in 1..v.len() {
        let (mut esq, mut dir) = (0, i);
        while esq < dir {
            let meio = esq + ((dir - esq) >> 1);
            if cmp(&v[i], &v[meio]) < 0.0 {
                dir = meio;
            } else {
                esq = meio + 1;
            }
        }
        let x = v.remove(i);
        v.insert(esq, x);
    }
}

// ---------- leitura dos campos do pokémon ----------

/// `ivArr(v)`: seis IVs, de uma lista de 6 ou de um objeto com as chaves do jogo.
pub fn iv_arr(v: Option<&Value>) -> Option<[f64; 6]> {
    match v? {
        Value::Array(a) if a.len() == 6 => {
            let mut r = [0.0; 6];
            for (o, x) in r.iter_mut().zip(a) {
                *o = js::mais(Some(x));
            }
            Some(r)
        }
        Value::Object(o) => {
            // `g(...ks)`: o primeiro valor que não é null nem undefined.
            let g = |ks: &[&str]| ks.iter().find_map(|k| o.get(*k).filter(|x| !x.is_null()));
            let r = [
                g(&["hp"]),
                g(&["atk", "ataque"]),
                g(&["def", "defesa"]),
                g(&["spAtk", "spatk", "spa"]),
                g(&["spDef", "spdef"]),
                g(&["speed", "spe", "spd"]),
            ];
            let mut out = [0.0; 6];
            for (o, x) in out.iter_mut().zip(r) {
                let f = js::mais(Some(x?));
                if !f.is_finite() || f < 0.0 {
                    return None;
                }
                *o = f;
            }
            Some(out)
        }
        _ => None,
    }
}

/// `qOf(p)`: `+(quality ?? qualidade) || null`.
pub fn q_of(p: &Value) -> Option<f64> {
    let v = js::mais(js::nulo_ou(p.get("quality"), p.get("qualidade")));
    (v != 0.0 && !v.is_nan()).then_some(v)
}

/// `potOf(p)`: o primeiro dígito de `String(potencia ?? "1")`, ou 1.
pub fn pot_of(p: &Value) -> f64 {
    let padrao = Value::String("1".into());
    let t = js::texto(js::nulo_ou(p.get("potencia"), Some(&padrao)));
    t.chars()
        .find(char::is_ascii_digit)
        .and_then(|c| c.to_digit(10))
        .map_or(1.0, f64::from)
}

/// `keyIdx(sp)`: 1 (ATK) se o poder dos golpes físicos for pelo menos o dos especiais, senão 3.
pub fn key_idx(sp: &EspecieCombate) -> usize {
    let (mut f, mut e) = (0.0, 0.0);
    for m in &sp.golpes {
        if m.poder > 0.0 && m.poder < 600.0 {
            if m.categoria == Categoria::Fisico {
                f += m.poder;
            } else {
                e += m.poder;
            }
        }
    }
    if f >= e {
        1
    } else {
        3
    }
}

/// O `x` do JS: um pokémon cru já lido (`{p, sp, lvl, q, iv, pot, shiny}`).
#[derive(Debug, Clone)]
pub struct Candidato<'a> {
    pub p: &'a Value,
    pub lvl: f64,
    pub q: Option<f64>,
    pub iv: Option<[f64; 6]>,
    pub pot: f64,
    pub shiny: bool,
}

impl<'a> Candidato<'a> {
    pub fn de(p: &'a Value) -> Self {
        Self {
            p,
            lvl: js::mais(p.get("level")),
            q: q_of(p),
            iv: iv_arr(p.get("ivs")),
            pot: pot_of(p),
            shiny: js::verdadeiro(p.get("shiny")),
        }
    }
}

fn atrib(c: &Combatente, k: usize) -> f64 {
    match k {
        0 => c.st.hp,
        1 => c.st.atk,
        2 => c.st.def,
        3 => c.st.spa,
        4 => c.st.spd,
        _ => c.st.spe,
    }
}

/// `forca(sp, x, lvl)`: `makeMon` com os padrões do veredito (qualidade 1,1, IV 16, P1).
pub fn forca(sp: &EspecieCombate, x: &Candidato, lvl: f64) -> Combatente {
    let q = x.q.filter(|q| *q != 0.0 && !q.is_nan()).unwrap_or(1.1);
    let pot = if x.pot == 0.0 || x.pot.is_nan() {
        1
    } else {
        x.pot as u8
    };
    let op = Opcoes {
        qualidade: q,
        ivs: x.iv.unwrap_or([16.0; 6]),
        potencia: pot,
        shiny: x.shiny,
        ..Opcoes::default()
    };
    Combatente::novo(sp, lvl, &op, &CONSTANTES)
}

/// Força no Nv 100: o stat de ataque pesa 60%, a resistência (HP × defesas) 40%.
fn pontos(sp: &EspecieCombate, kn: usize, x: &Candidato) -> f64 {
    let m = forca(sp, x, 100.0);
    js::pow(atrib(&m, kn), 0.6) * js::pow(m.hp * (m.st.def + m.st.spd), 0.2)
}

// ---------- avaliar ----------

#[derive(Debug, Clone)]
pub struct Avaliacao {
    pub k: usize,
    pub def_i: usize,
    pub me: Combatente,
    pub avg: Combatente,
    pub pct: f64,
    /// `[rótulo, passou (None = sem dado), valor]`.
    pub mins: Vec<(String, Option<bool>, String)>,
    pub npc: f64,
    /// `[cor, título, texto]`.
    pub v: [String; 3],
    /// Nível e qualidade do melhor outro da mesma espécie.
    pub melhor: Option<(f64, Option<f64>)>,
    pub melhor_pct: Option<f64>,
    pub qtd: usize,
}

/// `avaliar(x, {caixa})`. `caixa` são os pokémon da conta (só os da mesma espécie contam).
pub fn avaliar(sp: &EspecieCombate, preco: f64, x: &Candidato, caixa: &[&Value]) -> Avaliacao {
    // `kn = STATN[k]`: os índices de `keyIdx` (1 e 3) são os do próprio `STATN`.
    let k = key_idx(sp);
    let def_i = if sp.base[2] >= sp.base[4] { 2 } else { 4 };
    let me = forca(sp, x, x.lvl);
    let avg_x = Candidato {
        p: x.p,
        lvl: x.lvl,
        q: Some(1.1),
        iv: None,
        pot: 1.0,
        shiny: false,
    };
    let avg = forca(sp, &avg_x, x.lvl);
    let pct = js::round((atrib(&me, k) / atrib(&avg, k) - 1.0) * 100.0);
    let mesmos: Vec<Candidato> = caixa
        .iter()
        .filter(|p| js::mais(p.get("speciesId")) == sp.id as f64)
        .map(|p| Candidato::de(p))
        .collect();
    let meu = pontos(sp, k, x);
    // `outros.sort((u, v) => sc(v) - sc(u))[0]`, com a força calculada uma vez por candidato.
    let mut outros: Vec<(f64, &Candidato)> = mesmos
        .iter()
        .filter(|o| !mesmo_id(o.p, x.p))
        .map(|o| (pontos(sp, k, o), o))
        .collect();
    ordenar_como_js(&mut outros, |u, v| v.0 - u.0);
    let melhor = outros.first().copied();
    let melhor_pct = melhor.map(|(s, _)| js::round((s / meu - 1.0) * 1000.0) / 10.0);
    let iv = x.iv;
    let sem = || "sem dado".to_owned();
    let mins = vec![
        (
            "Qualidade 1,3 ou mais".to_owned(),
            Some(x.q.is_some_and(|q| q >= 1.3)),
            nf_opt(x.q, 3),
        ),
        (
            format!("IV de {} 20 ou mais", STAT_L[k]),
            iv.map(|v| v[k] >= 20.0),
            iv.map_or_else(sem, |v| js::numero(v[k])),
        ),
        (
            "IV de HP 16 ou mais".to_owned(),
            iv.map(|v| v[0] >= 16.0),
            iv.map_or_else(sem, |v| js::numero(v[0])),
        ),
        (
            format!("IV de {} 16 ou mais", STAT_L[def_i]),
            iv.map(|v| v[def_i] >= 16.0),
            iv.map_or_else(sem, |v| js::numero(v[def_i])),
        ),
    ];
    let npc = js::round(
        preco
            * (1.0 + min_js(x.lvl, 100.0) / 50.0)
            * x.q.unwrap_or(1.0)
            * if x.shiny { 10.0 } else { 1.0 },
    );
    let nome = &sp.nome;
    let v: [String; 3] = if x.shiny || x.pot >= 4.0 {
        [
            "gold".into(),
            "Guardar: é raro".into(),
            if x.shiny {
                "Shiny tem ×3 nos stats e só se vende por Gemas.".into()
            } else {
                "P4 ou P5 vale mais que qualquer qualidade.".into()
            },
        ]
    } else if let Some(((_, m), _)) = melhor.zip(melhor_pct).filter(|(_, mp)| mp.abs() < 3.0) {
        // empate técnico: diz qual dos dois leva vantagem e onde
        let mm = forca(sp, m, 100.0);
        let eu = forca(sp, x, 100.0);
        let dk = pc(atrib(&eu, k), atrib(&mm, k));
        let db = pc(
            eu.hp * (eu.st.def + eu.st.spd),
            mm.hp * (mm.st.def + mm.st.spd),
        );
        let quem = if dk >= 0.0 {
            "fique com este se ele for o atacante"
        } else {
            "o outro bate mais"
        };
        [
            "warn".into(),
            "Empate com outro que você tem".into(),
            format!(
                "Este e o seu {nome} Nv {} (qualidade {}) ficam a menos de 3% de diferença. Este tem {} {}% e resistência {}% contra o outro: {quem}. Dá para usar qualquer um dos dois; dispense o que sobrar.",
                nf(m.lvl, 0),
                nf_opt(m.q, 3),
                STAT_L[k],
                sg(dk),
                sg(db),
            ),
        ]
    } else if melhor.is_none() || melhor_pct.is_some_and(|p| p < 0.0) {
        if pct >= 8.0 {
            let mais_forte = melhor_pct.map_or_else(String::new, |mp| {
                format!(
                    ", e {}% mais forte que o segundo melhor da sua caixa",
                    nf(-mp, 1)
                )
            });
            [
                "ok".into(),
                "Manter: é o melhor que você tem".into(),
                format!(
                    "{} {}% sobre um {nome} médio{mais_forte}.",
                    STAT_L[k],
                    sg(pct)
                ),
            ]
        } else {
            [
                "warn".into(),
                "Manter por enquanto".into(),
                format!(
                    "É o melhor {nome} que você tem, mas está {}% contra um exemplar médio. Troque quando vier um com qualidade 1,3 ou mais.",
                    sg(pct)
                ),
            ]
        }
    } else {
        let (lvl, q) = melhor.map_or((f64::NAN, None), |(_, m)| (m.lvl, m.q));
        [
            "bad".into(),
            "Dispensar".into(),
            format!(
                "O seu melhor {nome} (Nv {}, qualidade {}) é {}% mais forte. Mande para a Oferenda (5 do mesmo tipo dão a pedra) ou venda no NPC por cerca de {} de ouro.",
                nf(lvl, 0),
                nf_opt(q, 3),
                nf_opt(melhor_pct, 1),
                nf(npc, 0)
            ),
        ]
    };
    Avaliacao {
        k,
        def_i,
        me,
        avg,
        pct,
        mins,
        npc,
        v,
        melhor: melhor.map(|(_, m)| (m.lvl, m.q)),
        melhor_pct,
        qtd: mesmos.len(),
    }
}

/// `o.p.id === x.p.id` (dois `undefined` são iguais).
fn mesmo_id(a: &Value, b: &Value) -> bool {
    match (a.get("id"), b.get("id")) {
        (None, None) => true,
        (Some(Value::Number(x)), Some(Value::Number(y))) => x.as_f64() == y.as_f64(),
        (Some(x), Some(y)) => x == y,
        _ => false,
    }
}

// ---------- decidir ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Acao {
    Guardar,
    Vender,
}

/// `{acao, motivo, v?, ivT?, npc?}` do `decidir`.
#[derive(Debug, Clone, PartialEq)]
pub struct Veredicto {
    pub acao: Acao,
    pub motivo: String,
    pub v: Option<String>,
    pub iv_t: Option<f64>,
    pub npc: Option<f64>,
}

impl Veredicto {
    fn guardar(motivo: impl Into<String>) -> Self {
        Self {
            acao: Acao::Guardar,
            motivo: motivo.into(),
            v: None,
            iv_t: None,
            npc: None,
        }
    }
}

/// `decidir(p, grupo, C, desejadas, porId)`: `c` é o `venderNpc` da config; `grupo`, os
/// pokémon da mesma espécie (inclui o próprio); `desejadas`, os ids da lista de desejos.
pub fn decidir(
    p: &Value,
    grupo: &[&Value],
    c: &Value,
    desejadas: Option<&HashSet<i64>>,
    esp: &Especies,
) -> Veredicto {
    let sp = esp.por_id(p.get("speciesId"));
    if js::verdadeiro(p.get("prot")) {
        return Veredicto::guardar("protegido: inicial, item segurado, TM ou refino");
    }
    let Some(sp) = sp else {
        return Veredicto::guardar("espécie fora da Pokedex do app");
    };
    let x = Candidato::de(p);
    let (Some(iv), Some(q)) = (x.iv, x.q) else {
        return Veredicto::guardar("sem IV ou qualidade: não dá para avaliar");
    };
    let r = avaliar(sp, esp.preco(sp.id), &x, grupo);
    let iv_t = iv.iter().fold(0.0, |s, n| s + n);
    let num = |k: &str| js::mais(c.get(k));
    let verd = |k: &str| js::verdadeiro(c.get(k));
    let texto = |k: &str| js::texto(c.get(k));
    let desejada = desejadas.is_some_and(|s| {
        let f = js::mais(p.get("speciesId"));
        f.fract() == 0.0 && s.contains(&(f as i64))
    });
    let salva: Option<String> = if verd("sempreGuardarShiny") && x.shiny {
        Some("shiny".into())
    } else if verd("sempreGuardarDesejo") && desejada {
        Some("espécie da lista de desejos dos bosses".into())
    } else if num("minIvTotal") > 0.0 && iv_t >= num("minIvTotal") {
        Some(format!(
            "IV total {} (mínimo {})",
            js::numero(iv_t),
            texto("minIvTotal")
        ))
    } else if num("minQualidade") > 0.0 && q >= num("minQualidade") {
        Some(format!(
            "qualidade {} (mínimo {})",
            nf(q, 3),
            texto("minQualidade")
        ))
    } else if num("minPotencia") > 0.0 && x.pot >= num("minPotencia") {
        Some(format!(
            "potência P{} (mínimo P{})",
            js::numero(x.pot),
            texto("minPotencia")
        ))
    } else {
        None
    };
    let cor = r.v[0].clone();
    if cor == "bad" {
        if let Some(s) = salva {
            return Veredicto {
                acao: Acao::Guardar,
                motivo: format!("Capturas diz dispensar, mas: {s}"),
                v: Some(cor),
                iv_t: Some(iv_t),
                npc: None,
            };
        }
        return Veredicto {
            acao: Acao::Vender,
            motivo: r.v[2].clone(),
            v: Some(cor),
            iv_t: Some(iv_t),
            npc: Some(r.npc),
        };
    }
    Veredicto {
        acao: Acao::Guardar,
        motivo: r.v[1].clone(),
        v: Some(cor),
        iv_t: Some(iv_t),
        npc: None,
    }
}

// ---------- substituição ----------

/// Onde o pokémon está (`localDe` do `modeloLocal`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Local {
    Equipe,
    Colecao,
    Depot,
}

/// `{velho, novo, pct}`: índices dentro do grupo.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Troca {
    pub velho: usize,
    pub novo: usize,
    pub pct: f64,
}

/// `substituicao(grupo, localDe, porId)`.
pub fn substituicao(
    grupo: &[&Value],
    local_de: &dyn Fn(&Value) -> Local,
    esp: &Especies,
) -> Option<Troca> {
    let sp = esp.por_id(grupo.first()?.get("speciesId"))?;
    let kn = key_idx(sp);
    let xs: Vec<Candidato> = grupo.iter().map(|p| Candidato::de(p)).collect();
    if xs.iter().any(|x| x.iv.is_none() || x.q.is_none()) {
        return None;
    }
    let sc: Vec<f64> = xs.iter().map(|x| pontos(sp, kn, x)).collect();
    let col: Vec<usize> = (0..xs.len())
        .filter(|&i| local_de(xs[i].p) == Local::Colecao)
        .collect();
    let [velho] = col[..] else {
        return None;
    };
    let v = &xs[velho];
    if v.shiny || v.pot >= 4.0 || js::verdadeiro(v.p.get("prot")) {
        return None;
    }
    // `reduce((a, b) => sc(b) > sc(a) ? b : a)`
    let melhor = (1..xs.len()).fold(0, |a, b| if sc[b] > sc[a] { b } else { a });
    if melhor == velho || local_de(xs[melhor].p) != Local::Depot {
        return None;
    }
    let pct = js::round((sc[melhor] / sc[velho] - 1.0) * 1000.0) / 10.0;
    if pct < 3.0 {
        return None;
    }
    Some(Troca {
        velho,
        novo: melhor,
        pct,
    })
}

// ---------- SHA-1 (o hash da prévia de venda do legado) ----------

/// `createHash('sha1').update(texto).digest('hex')`.
pub fn sha1_hex(texto: &str) -> String {
    let mut h: [u32; 5] = [
        0x6745_2301,
        0xEFCD_AB89,
        0x98BA_DCFE,
        0x1032_5476,
        0xC3D2_E1F0,
    ];
    let mut msg = texto.as_bytes().to_vec();
    let bits = (msg.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bits.to_be_bytes());
    for bloco in msg.chunks(64) {
        let mut w = [0u32; 80];
        for (i, b) in bloco.chunks(4).enumerate() {
            let mut a = [0u8; 4];
            a.copy_from_slice(b);
            w[i] = u32::from_be_bytes(a);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = h;
        for (i, wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | (!b & d), 0x5A82_7999_u32),
                20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
                _ => (b ^ c ^ d, 0xCA62_C1D6),
            };
            let t = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(*wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = t;
        }
        for (x, y) in h.iter_mut().zip([a, b, c, d, e]) {
            *x = x.wrapping_add(y);
        }
    }
    h.iter().map(|x| format!("{x:08x}")).collect()
}

#[cfg(test)]
mod testes {
    use super::*;
    use serde_json::json;

    fn dex() -> Pokedex {
        Pokedex::de_valor(json!([
            {"d": 1, "n": "Bulbasaur", "t": ["GRASS"], "s": [45, 49, 49, 65, 65, 45], "h": 20,
             "p": 3000, "c": "pokebola", "b": [0.3, 0.6, 0.9, 1.2, 2.4],
             "m": [["Razor Leaf", 55, 19.4, "F", "GRASS"], ["Solar Beam", 20, 31.2, "E", "GRASS"]]}
        ]))
        .unwrap()
    }

    fn poke(id: i64, lvl: i64, q: f64, iv: i64) -> Value {
        json!({"id": id, "speciesId": 1, "level": lvl, "quality": q,
               "ivs": [iv, iv, iv, iv, iv, iv], "potencia": 1})
    }

    #[test]
    fn formato_pt_br() {
        assert_eq!(local_br(1234567.891, 3), "1.234.567,891");
        assert_eq!(local_br(1234.5, 0), "1.235");
        assert_eq!(local_br(1.005, 2), "1,01");
        assert_eq!(local_br(0.9996, 3), "1");
        assert_eq!(local_br(999.9999, 3), "1.000");
        assert_eq!(local_br(-2.5, 0), "-3");
        assert_eq!(local_br(1.272, 3), "1,272");
        assert_eq!(local_br(100.0, 1), "100");
        assert_eq!(local_br(-0.0001, 3), "-0");
        assert_eq!(nf(f64::NAN, 1), "—");
        assert_eq!(local_br(f64::NAN, 3), "NaN");
    }

    #[test]
    fn sha1_conhecido() {
        assert_eq!(sha1_hex("abc"), "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(sha1_hex(""), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
    }

    #[test]
    fn campos_do_pokemon() {
        assert_eq!(
            iv_arr(Some(&json!([1, "2", 3, 4, 5, 6]))),
            Some([1., 2., 3., 4., 5., 6.])
        );
        assert_eq!(iv_arr(Some(&json!([1, 2]))), None);
        let o = json!({"hp": 1, "ataque": 2, "def": 3, "spa": 4, "spdef": 5, "spe": 6});
        assert_eq!(iv_arr(Some(&o)), Some([1., 2., 3., 4., 5., 6.]));
        assert_eq!(iv_arr(Some(&json!({"hp": 1}))), None);
        let neg = json!({"hp": -1, "atk": 1, "def": 1, "spAtk": 1, "spDef": 1, "speed": 1});
        assert_eq!(iv_arr(Some(&neg)), None);
        assert_eq!(pot_of(&json!({"potencia": 4})), 4.0);
        assert_eq!(pot_of(&json!({"potencia": "P3"})), 3.0);
        assert_eq!(pot_of(&json!({})), 1.0);
        assert_eq!(pot_of(&json!({"potencia": "x"})), 1.0);
        assert_eq!(q_of(&json!({"qualidade": "1.2"})), Some(1.2));
        assert_eq!(q_of(&json!({"quality": 0})), None);
    }

    #[test]
    fn decidir_casos() {
        let d = dex();
        let e = Especies::sem_precos(&d);
        let c = json!({});
        let a = poke(1, 100, 1.2, 20);
        let b = poke(2, 100, 1.0, 10);
        let grupo = [&a, &b];
        let ra = decidir(&a, &grupo, &c, None, &e);
        assert_eq!(ra.acao, Acao::Guardar);
        assert_eq!(ra.v.as_deref(), Some("ok"));
        let rb = decidir(&b, &grupo, &c, None, &e);
        assert_eq!(rb.acao, Acao::Vender);
        assert!(rb
            .motivo
            .starts_with("O seu melhor Bulbasaur (Nv 100, qualidade 1,2) é "));
        assert_eq!(rb.iv_t, Some(60.0));
        let c2 = json!({"minIvTotal": 50});
        let r2 = decidir(&b, &grupo, &c2, None, &e);
        assert_eq!(r2.acao, Acao::Guardar);
        assert_eq!(
            r2.motivo,
            "Capturas diz dispensar, mas: IV total 60 (mínimo 50)"
        );
        let prot = json!({"id": 3, "speciesId": 1, "prot": 1});
        assert_eq!(
            decidir(&prot, &[&prot], &c, None, &e).motivo,
            "protegido: inicial, item segurado, TM ou refino"
        );
        let fora = json!({"id": 4, "speciesId": 999});
        assert_eq!(
            decidir(&fora, &[&fora], &c, None, &e).motivo,
            "espécie fora da Pokedex do app"
        );
        let sem = json!({"id": 5, "speciesId": 1, "level": 5});
        assert_eq!(
            decidir(&sem, &[&sem], &c, None, &e).motivo,
            "sem IV ou qualidade: não dá para avaliar"
        );
    }

    #[test]
    fn substituicao_basica() {
        let d = dex();
        let e = Especies::sem_precos(&d);
        let velho = poke(1, 100, 1.0, 10);
        let novo = poke(2, 100, 1.3, 25);
        let grupo = [&velho, &novo];
        let local = |p: &Value| {
            if p.get("id") == Some(&json!(1)) {
                Local::Colecao
            } else {
                Local::Depot
            }
        };
        let t = substituicao(&grupo, &local, &e);
        assert!(matches!(
            t,
            Some(Troca {
                velho: 0,
                novo: 1,
                ..
            })
        ));
        let so_depot = |_: &Value| Local::Depot;
        assert!(substituicao(&grupo, &so_depot, &e).is_none());
    }
}
