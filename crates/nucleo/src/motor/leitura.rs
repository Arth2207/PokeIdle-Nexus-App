//! Rotas `GET` do motor (etapa 3), com o mesmo contrato do legado.
//!
//! Sem conta ao vivo no Nexus (veja o módulo pai), cada rota reproduz o legado sem nenhuma
//! janela conectada: as que pedem `nick` respondem 404 `{erro: "conta"}`, e as que agregam
//! contas usam só o que está guardado no `D`.

use std::collections::HashMap;

use serde_json::{json, Map, Value};

use super::veredito::{key_idx, Especies};
use crate::combate::especie::Pokedex;

use super::regras::ContaViva;
use super::{caminho, config, existe, num_ou_0, ou, Relogio};
use crate::js::{self, json_num, mais, verdadeiro};

/// Resposta HTTP do legado: status e corpo.
#[derive(Debug, Clone, PartialEq)]
pub struct Resposta {
    pub status: u16,
    pub corpo: Value,
}

impl Resposta {
    fn ok(corpo: Value) -> Self {
        Self { status: 200, corpo }
    }
}

/// Onde o JS lançaria `TypeError` (campo ausente lido como lista). O legado responde 500
/// `{erro: "erro interno"}`.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("{0}")]
pub struct ErroInterno(pub String);

type Res = Result<Resposta, ErroInterno>;

/// Rotas que começam procurando a conta conectada pelo `nick`: `vender/previa`,
/// `equipe/melhor`, `builds/prioridade`, `builds/sugerir`, `builds/teorica`, `pedras/diag`.
pub fn sem_conta() -> Resposta {
    Resposta {
        status: 404,
        corpo: json!({ "erro": "conta" }),
    }
}

/// `Array.prototype.sort` do V8 para listas curtas (inserção binária, estável). Reproduz a
/// ordem do legado inclusive quando o comparador devolve `NaN`, sem o risco de pânico do
/// `sort_by` com ordem não total.
fn ordenar_como_js<T>(v: &mut Vec<T>, cmp: impl Fn(&T, &T) -> f64) {
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

/// `Math.max(a, b)`: com `NaN` de qualquer lado, `NaN`.
fn max_js(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else {
        a.max(b)
    }
}

/// `Object.entries(v)`. `null`/`undefined` lançam `TypeError`.
fn entradas(v: Option<&Value>, o_que: &str) -> Result<Vec<(String, Value)>, ErroInterno> {
    match v {
        None | Some(Value::Null) => Err(ErroInterno(format!(
            "Cannot convert undefined or null to object ({o_que})"
        ))),
        Some(Value::Object(m)) => Ok(m.iter().map(|(k, x)| (k.clone(), x.clone())).collect()),
        Some(Value::Array(a)) => Ok(a
            .iter()
            .enumerate()
            .map(|(i, x)| (i.to_string(), x.clone()))
            .collect()),
        Some(Value::String(s)) => Ok(s
            .encode_utf16()
            .enumerate()
            .map(|(i, u)| (i.to_string(), Value::String(String::from_utf16_lossy(&[u]))))
            .collect()),
        Some(_) => Ok(Vec::new()),
    }
}

/// `lista.slice(0, n)` exigindo lista.
fn fatia(v: Option<&Value>, n: usize, o_que: &str) -> Result<Value, ErroInterno> {
    match v {
        Some(Value::Array(a)) => Ok(Value::Array(a.iter().take(n).cloned().collect())),
        _ => Err(ErroInterno(format!("{o_que}.slice is not a function"))),
    }
}

fn acoes(d: &Value) -> &[Value] {
    d.get("acoes")
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

/// `GET /api/builds?nick=`. Sem `nick`, o JS procura a chave `"null"`.
pub fn builds(d: &Value, nick: Option<&str>) -> Res {
    let todas = d
        .get("builds")
        .filter(|b| existe(Some(b)))
        .ok_or_else(|| ErroInterno("D.builds é undefined".into()))?;
    let vazio = json!({});
    let b = ou(todas.get(nick.unwrap_or("null")), &vazio);
    Ok(Resposta::ok(json!({ "builds": b })))
}

/// `GET /api/historico`.
pub fn historico(d: &Value, r: &Relogio) -> Res {
    let hoje = r.hoje();
    let vazio = json!({});
    let acoes_hoje: Vec<Value> = acoes(d)
        .iter()
        .filter(|a| r.dia_de(a.get("t")).as_deref() == Some(hoje.as_str()))
        .take(200)
        .cloned()
        .collect();
    let mut carimbos = Map::new();
    for (n, c) in entradas(Some(ou(d.get("carimbos"), &vazio)), "D.carimbos")? {
        if !existe(Some(&c)) {
            return Err(ErroInterno(format!("carimbos de {n} é null")));
        }
        let do_dia = ou(c.get(&hoje), &Value::Array(Vec::new())).clone();
        carimbos.insert(n, do_dia);
    }
    Ok(Resposta::ok(json!({
        "agora": r.agora_ms,
        "hoje": hoje,
        "ritmo": {},
        "huntAtual": {},
        "chanceShiny": 1.0 / 24_000.0,
        "medidas": ou(d.get("medidas"), &vazio),
        "medHist": ou(d.get("medHist"), &vazio),
        "horas": ou(d.get("horas"), &vazio),
        "carimbos": carimbos,
        "tipos": {},
        "acoes": acoes_hoje,
        "dia": ou(d.get("dia"), &vazio),
    })))
}

/// O que o financeiro usa do `resumoDia(nick, k)`.
struct ResumoDia {
    gasto: f64,
    trocas: f64,
    drops: Value,
}

/// `resumoDia(nick, k)`: `None` se não houver o dia.
fn resumo_dia(
    d: &Value,
    r: &Relogio,
    nick: &str,
    k: &str,
) -> Result<Option<ResumoDia>, ErroInterno> {
    let Some(dd) = caminho(d, &["dia", nick, k]).filter(|x| verdadeiro(Some(x))) else {
        return Ok(None);
    };
    let do_dia = |a: &Value| {
        a.get("nick").and_then(Value::as_str) == Some(nick)
            && r.dia_de(a.get("t")).as_deref() == Some(k)
    };
    let do_tipo = |a: &&Value, tipo: &str| {
        do_dia(a) && a.get("tipo").and_then(Value::as_str) == Some(tipo) && verdadeiro(a.get("ok"))
    };
    let gasto_log = acoes(d)
        .iter()
        .filter(|a| do_tipo(a, "compra"))
        .fold(0.0, |s, a| s + num_ou_0(a.get("custo")));
    let gasto = max_js(num_ou_0(dd.get("compras")), gasto_log);
    let n_trocas = acoes(d).iter().filter(|a| do_tipo(a, "hunt")).count() as f64;
    let trocas = max_js(num_ou_0(dd.get("trocas")), n_trocas);
    let mut drops = entradas(dd.get("drops"), "drops do dia")?;
    ordenar_como_js(&mut drops, |a, b| mais(Some(&b.1)) - mais(Some(&a.1)));
    // `d.capturas.slice(-8)`: sem a lista, o JS lança.
    if !matches!(dd.get("capturas"), Some(Value::Array(_) | Value::String(_))) {
        return Err(ErroInterno("d.capturas.slice is not a function".into()));
    }
    Ok(Some(ResumoDia {
        gasto,
        trocas,
        drops: Value::Array(
            drops
                .into_iter()
                .take(8)
                .map(|(n, q)| json!([n, q]))
                .collect(),
        ),
    }))
}

/// Copia os campos que existem (`undefined` some do JSON; `null` fica).
fn campos(de: &Value, nomes: &[&str], destino: &mut Map<String, Value>) {
    for n in nomes {
        if let Some(v) = de.get(*n) {
            destino.insert((*n).to_owned(), v.clone());
        }
    }
}

/// `dia(nick, k)` do financeiro, sem conta conectada (preços de poção e bola em 0).
fn dia_financeiro(d: &Value, r: &Relogio, nick: &str, k: &str) -> Result<Value, ErroInterno> {
    let dd = caminho(d, &["dia", nick, k]);
    let sd = caminho(d, &["saldo", nick, k]);
    if !verdadeiro(dd) && !verdadeiro(sd) {
        return Ok(Value::Null);
    }
    let sd = sd.filter(|x| verdadeiro(Some(x)));
    let rd = match dd.filter(|x| verdadeiro(Some(x))) {
        Some(_) => resumo_dia(d, r, nick, k)?,
        None => None,
    };
    let zero = json!(0);
    let campo = |c: &str| dd.and_then(|x| x.get(c));
    let loot = ou(campo("ouro"), &zero).clone();
    let compras = rd.as_ref().map_or(0.0, |x| x.gasto);
    let compras = if verdadeiro(Some(&json_num(compras))) {
        compras
    } else {
        0.0
    };
    let delta = sd.map(|s| mais(s.get("fim")) - mais(s.get("ini")));
    let loot_j = sd
        .filter(|s| existe(s.get("lootIni")))
        .map(|s| mais(Some(&loot)) - mais(s.get("lootIni")));
    let compras_j = sd
        .filter(|s| existe(s.get("comprasIni")))
        .map(|s| num_ou_0(campo("compras")) - mais(s.get("comprasIni")));
    let saldo = sd.map_or(Value::Null, |s| {
        let mut o = Map::new();
        campos(s, &["ini", "fim", "min", "max", "tIni", "tFim"], &mut o);
        o.insert("delta".into(), delta.map_or(Value::Null, json_num));
        Value::Object(o)
    });
    let janela = sd.map_or(Value::Null, |s| {
        let mut o = Map::new();
        if let Some(v) = s.get("tIni") {
            o.insert("de".into(), v.clone());
        }
        if let Some(v) = s.get("tFim") {
            o.insert("ate".into(), v.clone());
        }
        o.insert("loot".into(), loot_j.map_or(Value::Null, json_num));
        o.insert("compras".into(), compras_j.map_or(Value::Null, json_num));
        let inteiro = r
            .meia_noite(k)
            .is_some_and(|m| mais(s.get("tIni")) - (m as f64) < 600_000.0);
        o.insert("diaInteiro".into(), Value::Bool(inteiro));
        Value::Object(o)
    });
    // `delta - lootJ + comprasJ`; `null` vale 0 na conta.
    let outros = match (delta, loot_j) {
        (Some(dl), Some(lj)) => json_num(dl - lj + compras_j.unwrap_or(0.0)),
        _ => Value::Null,
    };
    let consumo = js::round(num_ou_0(campo("pot")) * 0.0 + num_ou_0(campo("bolas")) * 0.0);
    let mut o = Map::new();
    o.insert("dia".into(), json!(k));
    o.insert("loot".into(), loot);
    o.insert("compras".into(), json_num(compras));
    o.insert("consumo".into(), json_num(consumo));
    o.insert("saldo".into(), saldo);
    o.insert("janela".into(), janela);
    o.insert("outros".into(), outros);
    for c in [
        "xp", "xpPk", "kills", "pot", "bolas", "capt", "shiny", "desm",
    ] {
        o.insert(c.into(), ou(campo(c), &zero).clone());
    }
    let trocas = rd.as_ref().map_or(0.0, |x| x.trocas);
    let trocas = if verdadeiro(Some(&json_num(trocas))) {
        trocas
    } else {
        0.0
    };
    o.insert("trocas".into(), json_num(trocas));
    o.insert(
        "drops".into(),
        rd.map_or_else(|| Value::Array(Vec::new()), |x| x.drops),
    );
    o.insert(
        "comprasItens".into(),
        ou(campo("comprasItens"), &json!({})).clone(),
    );
    Ok(Value::Object(o))
}

fn chave_hora(dia: &str, h: i64) -> String {
    format!("{dia} {h:02}")
}

/// Saldo de ontem na mesma hora (`saldoNaMesmaHora`).
fn saldo_na_mesma_hora(d: &Value, nick: &str, ontem: &str, hora_ref: i64) -> Value {
    let vazio = json!({});
    let horas = ou(caminho(d, &["horas", nick]), &vazio);
    for h in (0..=hora_ref).rev() {
        if let Some(b) = horas.get(chave_hora(ontem, h)) {
            if verdadeiro(Some(b)) && existe(b.get("saldo")) {
                return json!({ "saldo": b.get("saldo"), "real": true });
            }
        }
    }
    let Some(sd) = caminho(d, &["saldo", nick, ontem]).filter(|x| verdadeiro(Some(x))) else {
        return Value::Null;
    };
    if !existe(sd.get("fim")) || !existe(sd.get("ini")) {
        return Value::Null;
    }
    let (mut tot, mut apos) = (0.0, 0.0);
    for h in 0..24 {
        let o = num_ou_0(horas.get(chave_hora(ontem, h)).and_then(|b| b.get("ouro")));
        tot += o;
        if h > hora_ref {
            apos += o;
        }
    }
    let fim = mais(sd.get("fim"));
    let outros = (fim - mais(sd.get("ini"))) - tot;
    let frac = if tot > 0.0 {
        apos / tot
    } else {
        (23 - hora_ref).max(0) as f64 / 24.0
    };
    json!({ "saldo": json_num(js::round(fim - apos - outros * frac)), "real": false })
}

/// Parcial de ontem até a hora de agora (`ontemParcial`).
fn ontem_parcial(d: &Value, nick: &str, ontem: &str, hora_ref: i64) -> Value {
    const CAMPOS: [&str; 5] = ["xp", "ouro", "kills", "pot", "desm"];
    let vazio = json!({});
    let horas = ou(caminho(d, &["horas", nick]), &vazio);
    let mut soma = [0.0; 5];
    let mut tem = false;
    for h in 0..=hora_ref {
        let Some(b) = horas
            .get(chave_hora(ontem, h))
            .filter(|b| verdadeiro(Some(b)))
        else {
            continue;
        };
        tem = true;
        for (s, c) in soma.iter_mut().zip(CAMPOS) {
            *s += num_ou_0(b.get(c));
        }
    }
    if !tem {
        return Value::Null;
    }
    Value::Object(
        CAMPOS
            .iter()
            .zip(soma)
            .map(|(c, s)| ((*c).to_owned(), json_num(s)))
            .collect(),
    )
}

fn chaves(v: Option<&Value>) -> Vec<String> {
    match v {
        Some(Value::Object(m)) => m.keys().cloned().collect(),
        _ => Vec::new(),
    }
}

/// `GET /api/financeiro`.
pub fn financeiro(d: &Value, r: &Relogio) -> Res {
    let (hoje, ontem, hora_ref) = (r.hoje(), r.ontem(), r.hora());
    let mut nicks: Vec<String> = Vec::new();
    for n in chaves(d.get("dia"))
        .into_iter()
        .chain(chaves(d.get("saldo")))
    {
        let tracos = n.chars().all(|c| c == '—' || c == '-');
        if n.encode_utf16().count() > 1 && !tracos && !nicks.contains(&n) {
            nicks.push(n);
        }
    }
    let mut contas = Vec::with_capacity(nicks.len());
    for nick in &nicks {
        let mut dias = chaves(caminho(d, &["dia", nick]));
        for k in chaves(caminho(d, &["saldo", nick])) {
            if !dias.contains(&k) {
                dias.push(k);
            }
        }
        // `.sort()` do JS compara em UTF-16.
        dias.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
        let ultimos = &dias[dias.len().saturating_sub(8)..];
        let mut semana = Vec::new();
        for k in ultimos {
            let x = dia_financeiro(d, r, nick, k)?;
            if verdadeiro(Some(&x)) {
                semana.push(x);
            }
        }
        contas.push(json!({
            "nick": nick,
            "tipos": [],
            "online": false,
            "ouroAgora": null,
            "hoje": dia_financeiro(d, r, nick, &hoje)?,
            "ontem": dia_financeiro(d, r, nick, &ontem)?,
            "semana": semana,
            "vsOntem": saldo_na_mesma_hora(d, nick, &ontem, hora_ref),
            "ontemParcial": ontem_parcial(d, nick, &ontem, hora_ref),
        }));
    }
    Ok(Resposta::ok(json!({
        "hoje": hoje,
        "ontem": ontem,
        "agora": r.agora_ms,
        "contas": contas,
    })))
}

/// `planoRaros()` sem conta conectada: só a meta do radar e os dias até o fim do mês.
pub fn raros(d: &Value, r: &Relogio) -> Resposta {
    Resposta::ok(plano_raros(d, r, &[]))
}

/// `ehRaro(id, nome, ouro, gema)`: consumível comum não é raro, mas o filtro por nome não
/// derruba item com preço de gema no Mercado.
fn eh_raro(id: f64, nome: &Value, ouro: Option<&Value>, gema: Option<&Value>) -> bool {
    let n = if verdadeiro(Some(nome)) {
        js::texto(Some(nome)).to_lowercase()
    } else {
        String::new()
    };
    if ["potion", "revive", "ball", "ether", "elixir"]
        .iter()
        .any(|p| n.contains(p))
    {
        return false;
    }
    let tem_gema = mais(gema) > 0.0;
    if n.ends_with("stone") && !tem_gema {
        return false;
    }
    id >= 60000.0 || num_ou_0(ouro) >= 1e6 || mais(gema) > 0.0
}

struct LinhaRaro {
    id: f64,
    nome: Value,
    qtd: f64,
    contas: Map<String, Value>,
}

/// `String(nome).replace(/\s*\(prov[áa]vel\)\s*$/i, '')`.
fn sem_provavel(nome: &str) -> String {
    let aparado = nome.trim_end();
    let minusculo = aparado.to_lowercase();
    if minusculo.ends_with("(provável)") || minusculo.ends_with("(provavel)") {
        let n = aparado.chars().count().saturating_sub(10);
        return aparado
            .chars()
            .take(n)
            .collect::<String>()
            .trim_end()
            .to_owned();
    }
    nome.to_owned()
}

/// Média de preço (`medias[id]?.<moeda>?.media || null`).
fn media_de(a: Option<&Value>, moeda: &str) -> Option<f64> {
    let v = a.and_then(|x| caminho(x, &[moeda, "media"]));
    verdadeiro(v).then(|| mais(v))
}

fn ou_nulo(x: Option<f64>) -> Value {
    x.map_or(Value::Null, json_num)
}

/// `planoRaros()` com as contas ao vivo: raros no inventário, preço de hoje contra a média de
/// 7 dias, projeção até o fim do mês e total de gemas por conta.
pub fn plano_raros(d: &Value, r: &Relogio, contas: &[ContaViva]) -> Value {
    let cs: Vec<&ContaViva> = contas
        .iter()
        .filter(|c| !c.nick.is_empty() && verdadeiro(caminho(&c.ultimo, &["hdx", "eu"])))
        .collect();
    let padrao = config::padrao();
    let base = config::base(d, cs.first().map_or("", |c| c.nick.as_str()));
    let radar = match base.get("radar") {
        Some(x) if verdadeiro(Some(x)) => x.clone(),
        _ => padrao.get("radar").cloned().unwrap_or(Value::Null),
    };
    let achar_itens = |chave: &str| {
        cs.iter()
            .filter_map(|c| caminho(&c.ultimo, &["hdx", chave, "itens"]))
            .find(|x| verdadeiro(Some(x)))
            .cloned()
            .unwrap_or_else(|| json!({}))
    };
    let m7 = achar_itens("medias");
    let m1 = achar_itens("medias1");
    // Map(id numérico -> nome); o último que aparece vence.
    let mut nomes: std::collections::HashMap<String, Value> = Default::default();
    for c in &cs {
        if let Some(Value::Array(a)) = c.welcome.get("itens") {
            for i in a {
                if let Some(Value::Number(n)) = i.get("id") {
                    let chave = js::numero(n.as_f64().unwrap_or(f64::NAN));
                    nomes.insert(chave, i.get("nome").cloned().unwrap_or(Value::Null));
                }
            }
        }
    }
    let mut linhas: Vec<LinhaRaro> = Vec::new();
    for c in &cs {
        let Some(Value::Object(itens)) = caminho(&c.ultimo, &["hdx", "eu", "items"]) else {
            continue;
        };
        for (k, q) in itens {
            let positivo = mais(Some(q)) > 0.0;
            if !positivo {
                continue;
            }
            let id = js::mais(Some(&Value::String(k.clone())));
            let chave = js::numero(id);
            let n = match nomes.get(&chave) {
                Some(x) if verdadeiro(Some(x)) => x.clone(),
                _ if id == 70000.0 => Value::String("Bronze Boss Token (provável)".into()),
                _ => Value::String(format!("#{k}")),
            };
            let a = m7.get(&chave);
            let ouro = a.and_then(|x| caminho(x, &["gold", "media"]));
            let gema = a.and_then(|x| caminho(x, &["orb", "media"]));
            if !eh_raro(id, &n, ouro, gema) {
                continue;
            }
            let pos = linhas
                .iter()
                .position(|l| l.id == id || (l.id.is_nan() && id.is_nan()));
            let l = match pos {
                Some(p) => &mut linhas[p],
                None => {
                    linhas.push(LinhaRaro {
                        id,
                        nome: n,
                        qtd: 0.0,
                        contas: Map::new(),
                    });
                    let ultimo = linhas.len() - 1;
                    &mut linhas[ultimo]
                }
            };
            l.qtd += mais(Some(q));
            l.contas.insert(c.nick.clone(), json_num(mais(Some(q))));
        }
    }
    // drops/dia por nome de item (média dos últimos 7 dias completos com dados)
    let hoje = r.hoje();
    let dias_restantes = r.dias_restantes_mes();
    let mut drops_por_nome: std::collections::HashMap<String, f64> = Default::default();
    for c in &cs {
        let Some(Value::Object(por)) = d.get("dia").and_then(|x| x.get(&c.nick)) else {
            continue;
        };
        let mut chaves: Vec<&String> = por.keys().collect();
        chaves.sort();
        let ini = chaves.len().saturating_sub(7);
        let completos: Vec<&&String> = chaves[ini..]
            .iter()
            .filter(|k| k.as_str() < hoje.as_str())
            .collect();
        if completos.is_empty() {
            continue;
        }
        let mut soma: Vec<(String, f64)> = Vec::new();
        for k in &completos {
            if let Some(Value::Object(dr)) = por.get(k.as_str()).and_then(|x| x.get("drops")) {
                for (n, q) in dr {
                    let q = mais(Some(q));
                    match soma.iter_mut().find(|(x, _)| x == n) {
                        Some((_, t)) => *t += q,
                        None => soma.push((n.clone(), q)),
                    }
                }
            }
        }
        for (n, total) in soma {
            *drops_por_nome.entry(n).or_insert(0.0) += total / completos.len() as f64;
        }
    }
    let mut itens: Vec<Map<String, Value>> = Vec::new();
    for l in &linhas {
        let chave = js::numero(l.id);
        let a = m7.get(&chave);
        let b = m1.get(&chave);
        let (g7, g1) = (media_de(a, "gold"), media_de(b, "gold"));
        let (o7, o1) = (media_de(a, "orb"), media_de(b, "orb"));
        let var = |x: Option<f64>, y: Option<f64>| match (x, y) {
            (Some(x), Some(y)) if x != 0.0 && y != 0.0 => Some((y / x - 1.0) * 100.0),
            _ => None,
        };
        let v = var(g7, g1)
            .unwrap_or(-999.0)
            .max(var(o7, o1).unwrap_or(-999.0));
        let nome = js::texto(Some(&l.nome));
        let drops_dia = drops_por_nome
            .get(&nome)
            .copied()
            .filter(|x| *x != 0.0)
            .or_else(|| {
                drops_por_nome
                    .get(&sem_provavel(&nome))
                    .copied()
                    .filter(|x| *x != 0.0)
            });
        let qtd_fim = drops_dia.map(|dd| js::round(l.qtd + dd * dias_restantes as f64));
        let gemas_proj = match (o7, qtd_fim) {
            (Some(o), Some(q)) => Some(js::round(o * q)),
            _ => None,
        };
        let alta = mais(radar.get("altaPct"));
        let sinal = if v <= -999.0 {
            "normal"
        } else if v >= alta {
            "vender"
        } else if v <= -alta {
            "barato"
        } else {
            "normal"
        };
        let mut o = Map::new();
        o.insert("id".into(), json_num(l.id));
        o.insert("nome".into(), l.nome.clone());
        o.insert("qtd".into(), json_num(l.qtd));
        o.insert("contas".into(), Value::Object(l.contas.clone()));
        o.insert("ouroMedio".into(), ou_nulo(g7));
        o.insert("ouroHoje".into(), ou_nulo(g1));
        o.insert("gemaMedia".into(), ou_nulo(o7));
        o.insert("gemaHoje".into(), ou_nulo(o1));
        o.insert("valorOuro".into(), ou_nulo(g7.map(|g| g * l.qtd)));
        o.insert("valorGemas".into(), ou_nulo(o7.map(|g| g * l.qtd)));
        o.insert(
            "varPct".into(),
            ou_nulo((v > -999.0).then(|| js::round(v * 10.0) / 10.0)),
        );
        o.insert("sinal".into(), json!(sinal));
        o.insert(
            "dropsDia".into(),
            ou_nulo(drops_dia.map(|x| js::round(x * 10.0) / 10.0)),
        );
        o.insert("qtdFimMes".into(), ou_nulo(qtd_fim));
        o.insert("gemasProjetadas".into(), ou_nulo(gemas_proj));
        itens.push(o);
    }
    let valor_gemas = |i: &Map<String, Value>| num_ou_0(i.get("valorGemas"));
    ordenar_como_js(&mut itens, |x, y| valor_gemas(y) - valor_gemas(x));
    let gemas = itens.iter().fold(0.0, |t, i| t + valor_gemas(i));
    // Totais por conta: cada conta tem seu próprio cofre de gemas
    let mut por_conta: Vec<(String, f64, f64, f64)> = Vec::new();
    for c in &cs {
        if !por_conta.iter().any(|p| p.0 == c.nick) {
            por_conta.push((c.nick.clone(), 0.0, 0.0, 0.0));
        }
    }
    for i in &itens {
        let gm = num_ou_0(i.get("gemaMedia"));
        if let Some(Value::Object(cc)) = i.get("contas") {
            for (n, q) in cc {
                let q = mais(Some(q));
                let p = match por_conta.iter().position(|p| &p.0 == n) {
                    Some(p) => p,
                    None => {
                        por_conta.push((n.clone(), 0.0, 0.0, 0.0));
                        por_conta.len() - 1
                    }
                };
                por_conta[p].1 += q;
                por_conta[p].2 += 1.0;
                por_conta[p].3 += gm * q;
            }
        }
    }
    let mut por_conta: Vec<Value> = por_conta
        .into_iter()
        .map(|(nick, qtd, n, g)| {
            json!({"nick": nick, "qtd": json_num(qtd), "itens": json_num(n),
                "gemas": json_num(js::round(g))})
        })
        .collect();
    ordenar_como_js(&mut por_conta, |a, b| {
        num_ou_0(b.get("gemas")) - num_ou_0(a.get("gemas"))
    });
    let proj_total = itens.iter().fold(0.0, |t, i| {
        let p = num_ou_0(i.get("gemasProjetadas"));
        t + if p != 0.0 { p } else { valor_gemas(i) }
    });
    let meta = mais(radar.get("metaGemas"));
    json!({
        "itens": itens,
        "porConta": por_conta,
        "gemasSeVendesse": json_num(js::round(gemas)),
        "gemasProjetadas": json_num(js::round(proj_total)),
        "metaRealista": json_num(js::round(proj_total * 0.8)),
        "diasRestantes": dias_restantes,
        "meta": radar.get("metaGemas"),
        "falta": json_num(max_js(0.0, js::round(meta - gemas))),
        "altaPct": radar.get("altaPct"),
        "ativo": radar.get("ativo"),
        "temPrecos": m7.as_object().is_some_and(|o| !o.is_empty()),
    })
}

/// `planoPedras()`: refino da equipe, falta e sobra de pedras entre as contas. `contas` na
/// ordem do `Map` do legado; `esp` é a pokédex (`BYD`).
pub fn plano_pedras(contas: &[ContaViva], esp: &Especies) -> Value {
    let vivas: Vec<&ContaViva> = contas
        .iter()
        .filter(|c| !c.nick.is_empty() && verdadeiro(caminho(&c.ultimo, &["hdx", "pokemons"])))
        .collect();
    let vazio = Map::new();
    let mapa: &Map<String, Value> = vivas
        .iter()
        .find_map(|c| {
            c.welcome
                .get("pedrasEvolucao")
                .filter(|v| verdadeiro(Some(v)))
        })
        .and_then(Value::as_object)
        .unwrap_or(&vazio);
    let medias: Map<String, Value> = vivas
        .iter()
        .find_map(|c| {
            caminho(&c.ultimo, &["hdx", "medias", "itens"]).filter(|v| verdadeiro(Some(v)))
        })
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let mut inv: HashMap<String, Map<String, Value>> = HashMap::new();
    for c in &vivas {
        let itens = caminho(&c.ultimo, &["hdx", "eu", "items"])
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        inv.insert(c.nick.clone(), itens);
    }
    // saldo que vai sendo "gasto" no plano
    let mut virt: HashMap<String, HashMap<String, f64>> = inv
        .iter()
        .map(|(n, m)| {
            let saldos = m
                .iter()
                .map(|(k, v)| (k.clone(), num_ou_0(Some(v))))
                .collect();
            (n.clone(), saldos)
        })
        .collect();
    let escolha = |t: &str| -> Option<&Value> {
        [mapa.get(t), mapa.get("NORMAL")]
            .into_iter()
            .flatten()
            .find(|v| verdadeiro(Some(v)))
    };
    let tem_pedra = |id: f64| {
        mapa.values().any(|m| {
            m.get("itemId")
                .is_some_and(|x| mais(Some(x)) == id && x.is_number())
        })
    };
    let mut necessarias: Vec<f64> = Vec::new();
    let mut linhas: Vec<LinhaPedra> = Vec::new();
    for c in &vivas {
        let Some(Value::Array(pokes)) = caminho(&c.ultimo, &["hdx", "pokemons"]) else {
            continue;
        };
        for p in pokes.iter().filter(|x| existe(x.get("slot"))) {
            let Some(sp) = esp.por_id(p.get("speciesId")) else {
                continue;
            };
            let ki = key_idx(sp);
            let stat = if ki == 3 { "spAtk" } else { "atk" };
            let refino = p
                .get("refino")
                .filter(|v| verdadeiro(Some(v)))
                .and_then(|r| r.get(stat));
            let n = {
                let x = mais(refino);
                if x == 0.0 || x.is_nan() {
                    0.0
                } else {
                    x
                }
            };
            let custo = custo_degrau(n + 1.0);
            let mut tipos: Vec<&'static str> = Vec::new();
            for t in &sp.tipos {
                if !tipos.contains(&t.nome()) {
                    tipos.push(t.nome());
                }
            }
            let mut ids: Vec<f64> = Vec::new();
            for t in &tipos {
                let id = escolha(t).and_then(|m| m.get("itemId"));
                if verdadeiro(id) {
                    let f = mais(id);
                    if !ids.iter().any(|x| *x == f || (x.is_nan() && f.is_nan())) {
                        ids.push(f);
                    }
                }
            }
            necessarias.extend(&ids);
            let nomes_pedras = tipos
                .iter()
                .map(|t| {
                    escolha(t)
                        .and_then(|m| m.get("nome"))
                        .filter(|v| verdadeiro(Some(v)))
                        .cloned()
                        .unwrap_or_else(|| Value::String((*t).to_owned()))
                })
                .collect();
            linhas.push(LinhaPedra {
                nick: c.nick.clone(),
                id: p.get("id").cloned(),
                nome: sp.nome.clone(),
                stat,
                nivel_refino: n,
                custo,
                tipos,
                ids,
                nomes_pedras,
                tem: 0.0,
                falta: 0.0,
                situacao: "falta",
                uso: None,
            });
        }
    }
    ordenar_como_js(&mut linhas, |a, b| a.custo - b.custo);
    let pilha = |virt: &HashMap<String, HashMap<String, f64>>, nick: &str, i: f64| {
        virt.get(nick)
            .and_then(|m| m.get(&js::numero(i)))
            .copied()
            .unwrap_or(0.0)
    };
    let saldo = |virt: &HashMap<String, HashMap<String, f64>>, nick: &str, ids: &[f64]| {
        ids.iter().fold(0.0, |t, i| t + pilha(virt, nick, *i))
    };
    let mut transferencias: Vec<Value> = Vec::new();
    for l in &mut linhas {
        let proprio = saldo(&virt, &l.nick, &l.ids);
        let todos = vivas
            .iter()
            .fold(0.0, |t, c| t + saldo(&virt, &c.nick, &l.ids));
        l.tem = proprio;
        l.falta = max_js(0.0, l.custo - proprio);
        l.situacao = if proprio >= l.custo {
            "pode"
        } else if todos >= l.custo {
            "juntando"
        } else {
            "falta"
        };
        if l.situacao == "pode" {
            // reserva as pedras no saldo virtual, maior pilha primeiro (mesma ordem do servidor)
            let mut f = l.custo;
            let mut uso = Vec::new();
            let mut ordem = l.ids.clone();
            ordenar_como_js(&mut ordem, |a, b| {
                pilha(&virt, &l.nick, *b) - pilha(&virt, &l.nick, *a)
            });
            for i in ordem {
                let u = min_js(pilha(&virt, &l.nick, i), f);
                if u > 0.0 {
                    uso.push((i, u));
                    if let Some(s) = virt
                        .get_mut(&l.nick)
                        .and_then(|m| m.get_mut(&js::numero(i)))
                    {
                        *s -= u;
                    }
                    f -= u;
                }
                if f <= 0.0 {
                    break;
                }
            }
            l.uso = Some(uso);
        } else if l.situacao == "juntando" {
            let mut f = l.falta;
            'contas: for c in &vivas {
                if c.nick == l.nick {
                    continue;
                }
                for &i in &l.ids {
                    let u = min_js(pilha(&virt, &c.nick, i), f);
                    if u > 0.0 {
                        transferencias.push(json!({
                            "de": c.nick, "para": l.nick, "itemId": json_num(i),
                            "qtd": json_num(u), "paraQuem": l.nome,
                        }));
                        if let Some(s) = virt
                            .get_mut(&c.nick)
                            .and_then(|m| m.get_mut(&js::numero(i)))
                        {
                            *s -= u;
                        }
                        f -= u;
                    }
                    if f <= 0.0 {
                        break;
                    }
                }
                if f <= 0.0 {
                    break 'contas;
                }
            }
        }
    }
    let mut sobras: Vec<Value> = Vec::new();
    for c in &vivas {
        let Some(itens) = inv.get(&c.nick) else {
            continue;
        };
        for (i, q) in entradas_js(itens) {
            let id = mais(Some(&Value::String(i.clone())));
            if !tem_pedra(id) || mais(Some(q)) <= 0.0 {
                continue;
            }
            if necessarias.contains(&id) {
                continue;
            }
            let m = medias
                .get(&js::numero(id))
                .and_then(|x| caminho(x, &["gold", "media"]))
                .filter(|v| verdadeiro(Some(v)))
                .cloned();
            let total = m
                .as_ref()
                .map_or(Value::Null, |m| json_num(mais(Some(m)) * mais(Some(q))));
            sobras.push(json!({
                "nick": c.nick, "itemId": json_num(id), "qtd": q,
                "precoMedio": m, "total": total,
            }));
        }
    }
    let equipe: Vec<Value> = linhas.iter().map(LinhaPedra::json).collect();
    let contas_json: Vec<Value> = vivas
        .iter()
        .map(|c| {
            let pedras = inv
                .get(&c.nick)
                .map(|m| {
                    entradas_js(m)
                        .into_iter()
                        .filter(|(i, _)| tem_pedra(mais(Some(&Value::String((*i).clone())))))
                        .map(|(i, v)| (i.clone(), v.clone()))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            json!({ "nick": c.nick, "pedras": objeto_js(pedras) })
        })
        .collect();
    let tipo_de_pedra = objeto_js(
        mapa.values()
            .map(|m| {
                (
                    js::texto(m.get("itemId")),
                    m.get("nome").cloned().unwrap_or(Value::Null),
                )
            })
            .collect(),
    );
    json!({
        "contas": contas_json,
        "equipe": equipe,
        "transferencias": transferencias,
        "sobras": sobras,
        "temPrecos": !medias.is_empty(),
        "tipoDePedra": tipo_de_pedra,
    })
}

/// `custoDegrau(n)`: `REFINO_BASE * (3n² − 3n + 1)`, com `REFINO_BASE = 500`.
pub fn custo_degrau(n: f64) -> f64 {
    500.0 * (3.0 * n * n - 3.0 * n + 1.0)
}

/// Uma linha do plano de pedras (o `L` do legado). `ids` e `uso` são ids de item.
struct LinhaPedra {
    nick: String,
    id: Option<Value>,
    nome: String,
    stat: &'static str,
    nivel_refino: f64,
    custo: f64,
    tipos: Vec<&'static str>,
    ids: Vec<f64>,
    nomes_pedras: Vec<Value>,
    tem: f64,
    falta: f64,
    situacao: &'static str,
    uso: Option<Vec<(f64, f64)>>,
}

impl LinhaPedra {
    fn json(&self) -> Value {
        let mut o = Map::new();
        o.insert("nick".into(), json!(self.nick));
        if let Some(id) = &self.id {
            o.insert("id".into(), id.clone());
        }
        o.insert("nome".into(), json!(self.nome));
        o.insert("stat".into(), json!(self.stat));
        o.insert("nivelRefino".into(), json_num(self.nivel_refino));
        o.insert("custo".into(), json_num(self.custo));
        o.insert("tipos".into(), json!(self.tipos));
        o.insert(
            "ids".into(),
            Value::Array(self.ids.iter().map(|i| json_num(*i)).collect()),
        );
        o.insert(
            "nomesPedras".into(),
            Value::Array(self.nomes_pedras.clone()),
        );
        o.insert("tem".into(), json_num(self.tem));
        o.insert("falta".into(), json_num(self.falta));
        o.insert("situacao".into(), json!(self.situacao));
        if let Some(u) = &self.uso {
            let pares = u
                .iter()
                .map(|(i, q)| (js::numero(*i), json_num(*q)))
                .collect();
            o.insert("uso".into(), objeto_js(pares));
        }
        Value::Object(o)
    }
}

/// `Math.min` com `NaN` contagiante.
fn min_js(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else {
        a.min(b)
    }
}

/// A chave é um índice de array do JS (`"0"`, `"12"`, sem zero à esquerda)? Essas chaves
/// saem primeiro, em ordem numérica, nos objetos do JS.
fn chave_indice(k: &str) -> Option<u32> {
    let ok = !k.is_empty()
        && k.bytes().all(|b| b.is_ascii_digit())
        && (k == "0" || !k.starts_with('0'))
        && k.len() <= 10;
    ok.then(|| k.parse::<u32>().ok())
        .flatten()
        .filter(|n| *n != u32::MAX)
}

/// `Object.entries(o)`: índices em ordem numérica, depois as demais chaves na inserção.
pub(crate) fn entradas_js(o: &Map<String, Value>) -> Vec<(&String, &Value)> {
    let mut idx: Vec<(u32, &String, &Value)> = Vec::new();
    let mut resto = Vec::new();
    for (k, v) in o {
        match chave_indice(k) {
            Some(n) => idx.push((n, k, v)),
            None => resto.push((k, v)),
        }
    }
    idx.sort_by_key(|(n, _, _)| *n);
    idx.into_iter()
        .map(|(_, k, v)| (k, v))
        .chain(resto)
        .collect()
}

/// `Object.fromEntries(pares)`: chave repetida fica com o último valor, na posição da
/// primeira, e os índices de array vêm primeiro.
pub(crate) fn objeto_js(pares: Vec<(String, Value)>) -> Value {
    let mut o = Map::new();
    for (k, v) in pares {
        o.insert(k, v);
    }
    Value::Object(
        entradas_js(&o)
            .into_iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
    )
}

/// `GET /api/pedras/plano` sem nenhuma conta conectada.
pub fn pedras_plano() -> Resposta {
    pedras_plano_contas(&[], &Especies::sem_precos(&Pokedex::default()))
}

/// `GET /api/pedras/plano`.
pub fn pedras_plano_contas(contas: &[ContaViva], esp: &Especies) -> Resposta {
    Resposta::ok(plano_pedras(contas, esp))
}

/// `GET /api/espolio` sem nenhuma conta conectada.
pub fn espolio() -> Resposta {
    let vazio = config::carregar(&json!({}));
    espolio_contas(&vazio, &[], &Especies::sem_precos(&Pokedex::default()))
}

/// `GET /api/espolio`: o que cada conta juntou, o valor médio de mercado (ouro) e o plano
/// de pedras.
pub fn espolio_contas(d: &Value, contas: &[ContaViva], esp: &Especies) -> Resposta {
    let medias: Map<String, Value> = contas
        .iter()
        .find_map(|c| {
            caminho(&c.ultimo, &["hdx", "medias", "itens"]).filter(|v| verdadeiro(Some(v)))
        })
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let mut out = Vec::new();
    for c in contas.iter().filter(|c| !c.nick.is_empty()) {
        let mut nomes: HashMap<String, Value> = HashMap::new();
        if let Some(Value::Array(a)) = c.welcome.get("itens") {
            for i in a {
                if let Some(Value::Number(n)) = i.get("id") {
                    nomes.insert(
                        js::numero(n.as_f64().unwrap_or(f64::NAN)),
                        i.get("nome").cloned().unwrap_or(Value::Null),
                    );
                }
            }
        }
        let vazio = Map::new();
        let it = caminho(&c.ultimo, &["hdx", "eu", "items"])
            .and_then(Value::as_object)
            .unwrap_or(&vazio);
        // (valor, linha): o valor (`m * q` ou nulo) ordena a lista
        let mut lista: Vec<(f64, Value)> = Vec::new();
        for (id, q) in entradas_js(it) {
            if mais(Some(q)).is_nan() || mais(Some(q)) <= 0.0 {
                continue;
            }
            let m = medias
                .get(id)
                .and_then(|x| caminho(x, &["gold", "media"]))
                .filter(|v| verdadeiro(Some(v)));
            let valor = m.map(|m| mais(Some(m)) * mais(Some(q)));
            let nome = nomes
                .get(&js::numero(mais(Some(&Value::String(id.clone())))))
                .filter(|v| verdadeiro(Some(v)))
                .cloned()
                .unwrap_or_else(|| Value::String(format!("#{id}")));
            let chave = valor.filter(|v| *v != 0.0 && !v.is_nan()).unwrap_or(0.0);
            lista.push((
                chave,
                json!({
                    "id": json_num(mais(Some(&Value::String(id.clone())))),
                    "nome": nome, "qtd": q, "precoMedio": m,
                    "valor": valor.map_or(Value::Null, json_num),
                }),
            ));
        }
        ordenar_como_js(&mut lista, |a, b| b.0 - a.0);
        let total = lista.iter().fold(0.0, |t, x| t + x.0);
        let papel = config::base(d, &c.nick).pointer("/papel/tipo").cloned();
        let mut linha = Map::new();
        linha.insert("nick".into(), json!(c.nick));
        if let Some(p) = papel {
            linha.insert("papel".into(), p);
        }
        linha.insert("total".into(), json_num(total));
        linha.insert(
            "itens".into(),
            Value::Array(lista.into_iter().map(|x| x.1).collect()),
        );
        out.push(Value::Object(linha));
    }
    Resposta::ok(json!({ "contas": out, "pedras": plano_pedras(contas, esp) }))
}

/// `GET /api/equipe/melhor/todas`.
pub fn equipe_melhor_todas() -> Resposta {
    Resposta::ok(json!({}))
}

/// `GET /api/combate`: sem conta aberta.
pub fn combate() -> Resposta {
    Resposta::ok(json!({ "erro": "Nenhuma conta aberta.", "contas": [] }))
}

/// `GET /api/diagnostico`.
pub fn diagnostico() -> Resposta {
    Resposta::ok(json!({ "diagnostico": {} }))
}

/// `GET /api/estado` sem conta conectada.
pub fn estado(d: &Value, r: &Relogio) -> Res {
    Ok(Resposta::ok(json!({
        "pausa": verdadeiro(d.get("pausa")),
        "idleboard": "sem-contas",
        "porta": 0,
        "agora": r.agora_ms,
        "contas": [],
        "acoes": fatia(d.get("acoes"), 60, "D.acoes")?,
        "mercadoAvisos": fatia(d.get("mercadoAvisos"), 20, "D.mercadoAvisos")?,
        "mercadoRejeitado": fatia(d.get("mercadoRejeitado"), 20, "D.mercadoRejeitado")?,
    })))
}

/// `GET /api/cache-linhas`.
pub fn cache_linhas(d: &Value) -> Res {
    let vazio = json!({});
    let mut cache = Vec::new();
    for (n, c) in entradas(Some(ou(d.get("linhasCache"), &vazio)), "D.linhasCache")? {
        if !existe(Some(&c)) {
            return Err(ErroInterno(format!("cache de {n} é null")));
        }
        let mut o = Map::new();
        o.insert("n".into(), Value::String(n));
        if let Some(t) = c.get("t") {
            o.insert("t".into(), t.clone());
        }
        let linhas = match ou(c.get("l"), &Value::Array(Vec::new())) {
            Value::Array(a) => Some(a.len()),
            Value::String(s) => Some(s.encode_utf16().count()),
            _ => None,
        };
        if let Some(n) = linhas {
            o.insert("linhas".into(), json!(n));
        }
        cache.push(Value::Object(o));
    }
    Ok(Resposta::ok(json!({ "cache": cache, "contas": [] })))
}

/// `GET /api/backup`.
pub fn backup(d: &Value) -> Resposta {
    let mut o = Map::new();
    campos(
        d,
        &["config", "medidas", "bloqueio", "dia", "acoes", "builds"],
        &mut o,
    );
    Resposta::ok(Value::Object(o))
}

#[cfg(test)]
mod testes {
    use super::*;

    const R: Relogio = Relogio {
        // 2026-11-12 13:00 UTC
        agora_ms: 1_794_488_400_000,
        fuso_ms: 0,
    };

    fn dia(drops: Value) -> Value {
        json!({"ouro": 100, "compras": 10, "kills": 3, "drops": drops, "capturas": []})
    }

    #[test]
    fn sort_como_v8_mantem_empates_na_ordem() {
        let mut v = vec![("a", 1.0), ("b", 765.0), ("c", 5.0), ("d", 5.0)];
        ordenar_como_js(&mut v, |x, y| y.1 - x.1);
        assert_eq!(
            v.iter().map(|x| x.0).collect::<String>(),
            "bcda",
            "empate c/d na ordem de inserção"
        );
        let mut n = vec![1.0, f64::NAN, 3.0];
        ordenar_como_js(&mut n, |x, y| y - x);
        assert_eq!(n.len(), 3);
    }

    #[test]
    fn financeiro_ordem_dos_nicks_e_drops() {
        let d = config::carregar(&json!({
            "dia": {"Zé": {"2026-11-12": dia(json!({"x": 2, "y": 9, "z": 2}))}, "—": {}},
            "saldo": {"Ana": {"2026-11-11": {"ini": 0, "fim": 50, "tIni": 1_794_355_200_000_u64, "tFim": 1, "lootIni": 0, "comprasIni": 0}}},
            "acoes": [{"t": 1_794_488_000_000_u64, "nick": "Zé", "tipo": "compra", "ok": true, "custo": 40}]
        }));
        let r = financeiro(&d, &R).map(|x| x.corpo).unwrap_or_default();
        let nicks: Vec<&str> = r["contas"]
            .as_array()
            .map(|a| a.iter().filter_map(|c| c["nick"].as_str()).collect())
            .unwrap_or_default();
        assert_eq!(nicks, ["Zé", "Ana"]);
        let hoje = &r["contas"][0]["hoje"];
        assert_eq!(hoje["compras"], json!(40));
        assert_eq!(hoje["drops"], json!([["y", 9], ["x", 2], ["z", 2]]));
        assert_eq!(hoje["saldo"], Value::Null);
        let ontem = &r["contas"][1]["ontem"];
        assert_eq!(ontem["saldo"]["delta"], json!(50));
        assert_eq!(ontem["janela"]["diaInteiro"], json!(true));
        assert_eq!(ontem["outros"], json!(50));
        // 50 − 50 × 10/24 (fração das horas que faltam): 29.
        assert_eq!(
            r["contas"][1]["vsOntem"],
            json!({"saldo": 29, "real": false})
        );
    }

    #[test]
    fn financeiro_sem_drops_e_erro_interno() {
        let d =
            config::carregar(&json!({"dia": {"Ana": {"2026-11-12": {"ouro": 1, "capturas": []}}}}));
        assert!(financeiro(&d, &R).is_err());
    }

    #[test]
    fn builds_e_estado() {
        let d = config::carregar(&json!({"builds": {"Ana": {"k": 1}, "null": 2}, "pausa": 1}));
        assert_eq!(
            builds(&d, Some("Ana")).map(|r| r.corpo),
            Ok(json!({"builds": {"k": 1}}))
        );
        assert_eq!(builds(&d, None).map(|r| r.corpo), Ok(json!({"builds": 2})));
        assert_eq!(
            builds(&d, Some("Bia")).map(|r| r.corpo),
            Ok(json!({"builds": {}}))
        );
        let e = estado(&d, &R).map(|r| r.corpo).unwrap_or_default();
        assert_eq!(e["pausa"], json!(true));
        assert_eq!(e["idleboard"], json!("sem-contas"));
        assert_eq!(sem_conta().status, 404);
    }

    #[test]
    fn historico_filtra_acoes_do_dia() {
        let d = config::carregar(&json!({
            "acoes": [{"t": 1_794_488_000_000_u64, "x": 1}, {"t": 1, "x": 2}],
            "carimbos": {"Ana": {"2026-11-12": [1]}, "Bia": {}}
        }));
        let h = historico(&d, &R).map(|r| r.corpo).unwrap_or_default();
        assert_eq!(h["acoes"], json!([{"t": 1_794_488_000_000_u64, "x": 1}]));
        assert_eq!(h["carimbos"], json!({"Ana": [1], "Bia": []}));
        assert_eq!(h["medidas"], json!({}));
    }

    #[test]
    fn raros_sem_contas_usa_meta_padrao() {
        let r = raros(&config::carregar(&json!({})), &R).corpo;
        assert_eq!(r["falta"], json!(1000));
        assert_eq!(r["diasRestantes"], json!(18));
    }
}
