//! Rotas `GET` do motor (etapa 3), com o mesmo contrato do legado.
//!
//! Sem conta ao vivo no Nexus (veja o módulo pai), cada rota reproduz o legado sem nenhuma
//! janela conectada: as que pedem `nick` respondem 404 `{erro: "conta"}`, e as que agregam
//! contas usam só o que está guardado no `D`.

use serde_json::{json, Map, Value};

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
    let radar = config::base(d, "")
        .get("radar")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let meta = mais(radar.get("metaGemas"));
    Resposta::ok(json!({
        "itens": [],
        "porConta": [],
        "gemasSeVendesse": 0,
        "gemasProjetadas": 0,
        "metaRealista": 0,
        "diasRestantes": r.dias_restantes_mes(),
        "meta": radar.get("metaGemas"),
        "falta": json_num(max_js(0.0, js::round(meta - 0.0))),
        "altaPct": radar.get("altaPct"),
        "ativo": radar.get("ativo"),
        "temPrecos": false,
    }))
}

/// `planoPedras()` sem conta conectada.
pub fn plano_pedras() -> Value {
    json!({
        "contas": [], "equipe": [], "transferencias": [], "sobras": [],
        "temPrecos": false, "tipoDePedra": {}
    })
}

/// `GET /api/pedras/plano`.
pub fn pedras_plano() -> Resposta {
    Resposta::ok(plano_pedras())
}

/// `GET /api/espolio`.
pub fn espolio() -> Resposta {
    Resposta::ok(json!({ "contas": [], "pedras": plano_pedras() }))
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
