//! Regras do motor: compra de suprimentos (`planoCompra`, `precosEmUso`, `motorCompra`,
//! `motorPocao` do legado).
//!
//! O legado guarda, no objeto da conta, o consumo medido (`conta.cons.ev`) e o maior ouro
//! visto (`conta.ouroMax`). A `ContaViva` ainda não tem o consumo: as funções `*_com` o
//! recebem à parte; as demais usam consumo vazio.

use serde_json::{json, Map, Value};

use super::{ContaViva, Decisao, EstadoLaco, Registro};
use crate::js;
use crate::motor::jogo::{self, norm, num};
use crate::motor::{caminho, config, existe, num_ou_0, ou, Relogio};

/// Um evento de consumo (`conta.cons.ev`): instante, `tipo|nome normalizado`, quantidade.
pub type Consumo = (i64, String, f64);

const HORA: f64 = 3.6e6;

// ---------- helpers de JS ----------

fn v_ou_nulo(v: Option<&Value>) -> Value {
    v.cloned().unwrap_or(Value::Null)
}

/// `Math.min(a, b)`: com `NaN` em qualquer lado, `NaN`.
fn min2(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else {
        a.min(b)
    }
}

/// `Math.max(a, b)`: com `NaN` em qualquer lado, `NaN`.
fn max2(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else {
        a.max(b)
    }
}

/// `a == b` do JS para ids (número ou texto).
fn igual_frouxo(a: Option<&Value>, b: Option<&Value>) -> bool {
    match (a, b) {
        (Some(Value::String(x)), Some(Value::String(y))) => x == y,
        (Some(x), Some(y)) => js::mais(Some(x)) == js::mais(Some(y)),
        _ => false,
    }
}

/// `v.toLocaleString('pt-BR')`: milhar com ponto, vírgula decimal, no máximo 3 casas.
fn fmt_pt(x: f64) -> String {
    if x.is_nan() {
        return "NaN".into();
    }
    if x.is_infinite() {
        return if x > 0.0 { "∞" } else { "-∞" }.into();
    }
    let t = js::to_fixed(x.abs(), 3);
    let (int, frac) = t.split_once('.').unwrap_or((&t, ""));
    let frac = frac.trim_end_matches('0');
    let mut agrupado = String::new();
    for (i, c) in int.chars().enumerate() {
        if i > 0 && (int.len() - i) % 3 == 0 {
            agrupado.push('.');
        }
        agrupado.push(c);
    }
    let neg = x < 0.0 && (agrupado != "0" || !frac.is_empty());
    format!(
        "{}{}{}{}",
        if neg { "-" } else { "" },
        agrupado,
        if frac.is_empty() { "" } else { "," },
        frac
    )
}

fn lista<'a>(v: &'a Value, campo: &str) -> &'a [Value] {
    v.get(campo)
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

/// `coll[id]` do JS, em objeto ou lista.
fn indexar<'a>(coll: &'a Value, id: &Value) -> Option<&'a Value> {
    let chave = js::texto(Some(id));
    match coll {
        Value::Object(o) => o.get(&chave),
        Value::Array(a) => chave.parse::<usize>().ok().and_then(|i| a.get(i)),
        _ => None,
    }
}

/// `estoque(conta, kind, id, nome)`.
fn estoque(conta: &ContaViva, bola: bool, id: Option<&Value>, nome: Option<&Value>) -> Option<f64> {
    let eu = caminho(&conta.ultimo, &["hdx", "eu"]).filter(|e| js::verdadeiro(Some(e)));
    if let (Some(eu), Some(id)) = (eu, id.filter(|i| existe(Some(i)))) {
        let coll = eu
            .get(if bola { "balls" } else { "items" })
            .filter(|c| js::verdadeiro(Some(c)));
        if let Some(v) = coll
            .and_then(|c| indexar(c, id))
            .filter(|v| existe(Some(v)))
        {
            return Some(js::mais(Some(v)));
        }
    }
    let chips = conta.ultimo.get(if bola { "bolas" } else { "pocoes" });
    let chip = chips
        .and_then(Value::as_array)
        .and_then(|a| a.iter().find(|b| b.get("nome") == nome && nome.is_some()));
    chip.and_then(|c| num(c.get("q")))
}

/// `gastosHora(nick)`: ouro gasto em compras na última hora, pelo `D.acoes`.
fn gastos_hora(d: &Value, nick: &str, agora: f64) -> f64 {
    lista(d, "acoes")
        .iter()
        .filter(|a| {
            a.get("nick").and_then(Value::as_str) == Some(nick)
                && a.get("tipo").and_then(Value::as_str) == Some("compra")
                && js::verdadeiro(a.get("ok"))
                && js::mais(a.get("t")) > agora - HORA
        })
        .map(|a| num_ou_0(a.get("custo")))
        .sum()
}

// ---------- suprimentos e escolha ----------

/// Uma bola ou poção ligada, casada com o catálogo da loja (`suprimentos`).
struct Suprimento {
    nome_chip: Value,
    usado1h: f64,
    usado_alguma: bool,
    /// O item do catálogo; `None` = `semCatalogo`.
    item: Option<Value>,
    tem: f64,
}

impl Suprimento {
    fn sem_catalogo(&self) -> bool {
        self.item.is_none()
    }
    fn campo(&self, k: &str) -> Option<&Value> {
        self.item.as_ref().and_then(|i| i.get(k))
    }
}

fn suprimentos(conta: &ContaViva, bola: bool, ev: &[Consumo], agora: i64) -> Vec<Suprimento> {
    let w = &conta.welcome;
    if !js::verdadeiro(Some(w)) {
        return Vec::new();
    }
    let u = &conta.ultimo;
    let chips = lista(u, if bola { "bolas" } else { "pocoes" });
    let catalogo: Vec<&Value> = if bola {
        lista(w, "bolas")
            .iter()
            .filter(|x| {
                js::verdadeiro(x.get("compravel"))
                    && !js::verdadeiro(x.get("inf"))
                    && js::mais(x.get("preco")) > 0.0
            })
            .collect()
    } else {
        lista(w, "itens")
            .iter()
            .filter(|x| {
                x.get("cat").and_then(Value::as_str) == Some("heal")
                    && js::mais(x.get("preco")) > 0.0
            })
            .collect()
    };
    let tipo = if bola { "bola" } else { "pocao" };
    chips
        .iter()
        .filter(|c| js::verdadeiro(c.get("on")))
        .map(|c| {
            let nome = norm(c.get("nome"));
            let k = format!("{tipo}|{nome}");
            let item = catalogo.iter().find(|x| norm(x.get("nome")) == nome);
            let usado1h = ev
                .iter()
                .filter(|e| e.1 == k && ((agora - e.0) as f64) < HORA)
                .map(|e| e.2)
                .sum();
            let tem_chip = num(c.get("q")).filter(|n| *n != 0.0).unwrap_or(0.0);
            let tem = item
                .and_then(|it| estoque(conta, bola, it.get("id"), c.get("nome")))
                .unwrap_or(tem_chip);
            Suprimento {
                nome_chip: v_ou_nulo(c.get("nome")),
                usado1h,
                usado_alguma: ev.iter().any(|e| e.1 == k),
                item: item.map(|i| (*i).clone()),
                tem,
            }
        })
        .collect()
}

/// `escolher(lista)`: o índice escolhido e o motivo. `None` = `it: null`.
fn escolher(l: &[Suprimento]) -> (Option<usize>, &'static str) {
    let ok: Vec<usize> = (0..l.len()).filter(|i| !l[*i].sem_catalogo()).collect();
    // maior valor, o primeiro em caso de empate (o sort do JS é estável)
    let maior = |filtro: &dyn Fn(&Suprimento) -> bool, valor: &dyn Fn(&Suprimento) -> f64| {
        let mut melhor: Option<usize> = None;
        for &i in ok.iter().filter(|i| filtro(&l[**i])) {
            if melhor.is_none_or(|m| valor(&l[i]) > valor(&l[m])) {
                melhor = Some(i);
            }
        }
        melhor
    };
    if let Some(i) = maior(&|x| x.usado1h > 0.0, &|x| x.usado1h) {
        return (Some(i), "é a que você está gastando agora");
    }
    if let Some(i) = maior(&|x| x.tem > 0.0, &|x| x.tem) {
        return (Some(i), "é a ligada com mais estoque");
    }
    if let Some(i) = ok.iter().copied().find(|i| l[*i].usado_alguma) {
        return (Some(i), "foi a última que você usou");
    }
    let como = if l.is_empty() {
        "nenhuma está ligada no jogo"
    } else if !ok.is_empty() {
        "nenhuma das ligadas tem estoque nem foi usada; ligue a que você quer"
    } else {
        "o nome das ligadas não bateu com a loja"
    };
    (None, como)
}

// ---------- planoCompra ----------

/// O que o plano devolve além do JSON: o que o JS guarda fora dele.
struct Plano {
    json: Value,
    ouro_max: f64,
    /// `C.maxGastoHora` como número (`Infinity` sem medida; o JSON grava `null`).
    max_gasto_hora: f64,
}

fn inserir(m: &mut Map<String, Value>, k: &str, v: Option<&Value>) {
    if let Some(v) = v {
        m.insert(k.into(), v.clone());
    }
}

fn plano_interno(
    d: &Value,
    r: &Relogio,
    conta: &ContaViva,
    ev: &[Consumo],
    ouro_max_antes: f64,
) -> Plano {
    let cfg = config::efetiva(d, &conta.nick);
    let c = cfg.get("compra").cloned().unwrap_or(Value::Null);
    if !js::verdadeiro(c.get("auto")) {
        return Plano {
            max_gasto_hora: js::mais(c.get("maxGastoHora")),
            json: c,
            ouro_max: ouro_max_antes,
        };
    }
    let agora = r.agora_ms;
    let agora_f = agora as f64;
    let u = &conta.ultimo;
    let w_cat = &conta.welcome;
    let tem_w = js::verdadeiro(Some(w_cat));
    let w = jogo::janela(conta, HORA, agora_f, agora_f);
    let ligadas = |arr: Option<&Value>| -> Vec<Value> {
        arr.and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter(|b| js::verdadeiro(b.get("on")))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    };
    let bolas_l = suprimentos(conta, true, ev, agora);
    let pocoes_l = suprimentos(conta, false, ev, agora);
    let (ib, bola_como) = escolher(&bolas_l);
    let (ip, pocao_como0) = escolher(&pocoes_l);
    let bola: Option<Value> = ib.and_then(|i| bolas_l[i].item.clone());
    let mut pocao: Option<Value> = ip.and_then(|i| pocoes_l[i].item.clone());
    let mut pocao_como = pocao_como0;
    let soma = |arr: Option<&Value>| -> f64 {
        ligadas(arr)
            .iter()
            .map(|c| num(c.get("q")).filter(|n| *n != 0.0).unwrap_or(0.0))
            .sum()
    };
    // Qual poção compensa (veja o comentário do legado): cura útil = mín(cura, HP que falta).
    let hp_max = {
        let hp = ou(caminho(u, &["ativo", "hp"]), &Value::Null);
        let t = if js::verdadeiro(Some(hp)) {
            js::texto(Some(hp))
        } else {
            String::new()
        };
        t.split('/')
            .nth(1)
            .and_then(jogo::num_texto)
            .filter(|n| *n != 0.0)
    };
    let lim = min2(
        100.0,
        max2(
            10.0,
            num(u.get("limiar")).filter(|n| *n != 0.0).unwrap_or(50.0),
        ),
    );
    let falta = hp_max.map(|h| h * (1.0 - lim / 100.0));
    let cura_de = |x: &Value, hp: f64| -> f64 {
        let pct = js::mais(x.get("healPct"));
        if pct > 0.0 {
            pct * hp
        } else {
            js::mais(x.get("healAmount"))
        }
    };
    let cura_util = |x: &Value| -> Option<f64> {
        let hp = hp_max?;
        Some(min2(cura_de(x, hp), falta.unwrap_or(f64::NAN)))
    };
    let uso_pot_h = {
        let ev_soma: f64 = ev
            .iter()
            .filter(|e| e.1.starts_with("pocao|") && ((agora - e.0) as f64) < HORA)
            .map(|e| e.2)
            .sum();
        if ev_soma != 0.0 {
            ev_soma
        } else if w.kills >= 10 && w.kph.is_some_and(|k| k != 0.0) {
            w.kph.unwrap_or(0.0) * w.pot_kill.filter(|p| *p != 0.0).unwrap_or(0.0)
        } else {
            0.0
        }
    };
    let cura_h: Option<f64> = match (&pocao, hp_max) {
        (Some(p), Some(_)) if uso_pot_h != 0.0 => {
            Some(uso_pot_h * cura_util(p).unwrap_or(f64::NAN))
        }
        _ => None,
    };
    let mut comparar: Vec<(f64, Value)> = Vec::new();
    if let (true, Some(hp)) = (tem_w, hp_max) {
        for x in lista(w_cat, "itens") {
            let pct = js::mais(x.get("healPct"));
            if x.get("cat").and_then(Value::as_str) != Some("heal")
                || js::mais(x.get("preco")) <= 0.0
                || js::mais(x.get("preco")).is_nan()
                || !(pct > 0.0 || js::mais(x.get("healAmount")) > 0.0)
            {
                continue;
            }
            let util = cura_util(x).unwrap_or(f64::NAN);
            let cura_n = cura_de(x, hp);
            let preco = js::mais(x.get("preco"));
            let por_h = cura_h
                .filter(|c| *c != 0.0 && !c.is_nan())
                .map(|c| c / util);
            let custo_mil = preco / util * 1000.0;
            let mut o = Map::new();
            inserir(&mut o, "id", x.get("id"));
            inserir(&mut o, "nome", x.get("nome"));
            inserir(&mut o, "preco", x.get("preco"));
            o.insert(
                "cura".into(),
                if pct > 0.0 {
                    js::json_num(pct * hp)
                } else {
                    x.get("healAmount").cloned().unwrap_or(Value::Null)
                },
            );
            if pct <= 0.0 && x.get("healAmount").is_none() {
                o.remove("cura");
            }
            inserir(&mut o, "curaPct", x.get("healPct"));
            o.insert("util".into(), js::json_num(util));
            o.insert("desperdicio".into(), js::json_num(1.0 - util / cura_n));
            o.insert("porH".into(), por_h.map_or(Value::Null, js::json_num));
            o.insert(
                "custoH".into(),
                por_h.map_or(Value::Null, |p| js::json_num(p * preco)),
            );
            o.insert("custoPorMilHp".into(), js::json_num(custo_mil));
            o.insert(
                "ligada".into(),
                json!(pocoes_l.iter().any(|p| p.campo("id") == x.get("id"))),
            );
            o.insert(
                "emUso".into(),
                json!(pocao.as_ref().is_some_and(|p| p.get("id") == x.get("id"))),
            );
            comparar.push((custo_mil, Value::Object(o)));
        }
        // `sort((a, b) => a.custoPorMilHp - b.custoPorMilHp)`: estável; NaN conta como empate.
        // Inserção simples (sem o risco de pânico do sort com ordem inconsistente).
        for i in 1..comparar.len() {
            let mut j = i;
            while j > 0 && comparar[j - 1].0 - comparar[j].0 > 0.0 {
                comparar.swap(j - 1, j);
                j -= 1;
            }
        }
    }
    let comparar: Vec<Value> = comparar.into_iter().map(|c| c.1).collect();
    let melhor_pocao = comparar.first().cloned();
    // com "Poção mais barata" ligada, a compra repõe a mais barata
    if let (Some(m), true) = (
        &melhor_pocao,
        js::verdadeiro(caminho(&cfg, &["pocao", "ativo"])),
    ) {
        let it = lista(w_cat, "itens")
            .iter()
            .find(|x| js::texto(x.get("id")) == js::texto(m.get("id")));
        if let Some(it) = it {
            let difere = pocao
                .as_ref()
                .is_none_or(|p| js::texto(p.get("id")) != js::texto(it.get("id")));
            if difere {
                pocao = Some(it.clone());
                pocao_como = "é a mais barata por HP curado";
            }
        }
    }
    let medido = w.kills >= 10 && w.kph.is_some_and(|k| k != 0.0);
    let kph = w.kph.unwrap_or(0.0);
    let falso0 = |v: Option<f64>| v.filter(|x| *x != 0.0).unwrap_or(0.0);
    let uso_bola = if medido {
        kph * falso0(w.bolas_kill)
    } else {
        0.0
    };
    let uso_pot = if medido {
        kph * falso0(w.pot_kill)
    } else {
        0.0
    };
    let ouro = num(u.get("gold")).filter(|n| *n != 0.0).unwrap_or(0.0);
    let ouro_max = max2(ouro_max_antes, ouro);
    let loot_h = if medido {
        kph * falso0(w.gold_kill)
    } else {
        0.0
    };
    let preco_de = |p: &Option<Value>| p.as_ref().map_or(0.0, |p| num_ou_0(p.get("preco")));
    let gasto_h = uso_bola * preco_de(&bola) + uso_pot * preco_de(&pocao);
    let arred = |x: f64, m: f64| (x / m).ceil() * m;
    let max_gasto = if medido {
        max2(
            max2(js::round(loot_h * 0.8), js::round(gasto_h * 3.0)),
            500_000.0,
        )
    } else {
        f64::INFINITY
    };
    let ligadas_json = |l: &[Suprimento]| -> Value {
        Value::Array(
            l.iter()
                .map(|x| {
                    let mut o = Map::new();
                    let nome = ou(x.campo("nome"), &x.nome_chip);
                    if !matches!(nome, Value::Null) || x.campo("nome").is_some() {
                        o.insert("nome".into(), nome.clone());
                    }
                    o.insert("tem".into(), js::json_num(x.tem));
                    o.insert("usado1h".into(), js::json_num(x.usado1h));
                    o.insert("semCatalogo".into(), json!(x.sem_catalogo()));
                    Value::Object(o)
                })
                .collect(),
        )
    };
    let n = |x: f64| js::json_num(x);
    let mut o = match c {
        Value::Object(m) => m,
        _ => Map::new(),
    };
    let mut p = |k: &str, v: Value| {
        o.insert(k.into(), v);
    };
    p("auto", json!(true));
    p("medido", json!(medido));
    p("usoBola", n(uso_bola));
    p("usoPot", n(uso_pot));
    p("bolaComo", json!(bola_como));
    p("pocaoComo", json!(pocao_como));
    p("hpMax", hp_max.map_or(Value::Null, n));
    p("limiar", n(lim));
    p("curaH", cura_h.map_or(Value::Null, n));
    p("melhorPocao", melhor_pocao.clone().unwrap_or(Value::Null));
    p("bolaTotal", n(soma(u.get("bolas"))));
    p("pocaoTotal", n(soma(u.get("pocoes"))));
    p("comparar", Value::Array(comparar));
    p("bolasLigadas", ligadas_json(&bolas_l));
    p("pocoesLigadas", ligadas_json(&pocoes_l));
    let mut opt = |k: &str, v: Option<&Value>, vazio: Value| {
        if let Some(v) = v {
            o.insert(k.into(), v.clone());
        } else if !matches!(vazio, Value::Bool(false)) {
            o.insert(k.into(), vazio);
        }
    };
    opt(
        "bolaId",
        bola.as_ref().and_then(|b| b.get("id")),
        Value::Null,
    );
    opt(
        "bolaNome",
        bola.as_ref().and_then(|b| b.get("nome")),
        Value::Null,
    );
    let bola_preco = bola.as_ref().map(|b| b.get("preco"));
    opt(
        "bolaPreco",
        bola_preco.unwrap_or(Some(&json!(0))),
        json!(false),
    );
    opt(
        "pocaoId",
        pocao.as_ref().and_then(|b| b.get("id")),
        Value::Null,
    );
    opt(
        "pocaoNome",
        pocao.as_ref().and_then(|b| b.get("nome")),
        Value::Null,
    );
    let pocao_preco = pocao.as_ref().map(|b| b.get("preco"));
    opt(
        "pocaoPreco",
        pocao_preco.unwrap_or(Some(&json!(0))),
        json!(false),
    );
    let mut p = |k: &str, v: Value| {
        o.insert(k.into(), v);
    };
    p(
        "bolaMin",
        n(if medido {
            max2(100.0, arred(uso_bola, 50.0))
        } else {
            300.0
        }),
    );
    p(
        "bolaLote",
        n(if medido {
            min2(5000.0, max2(200.0, arred(uso_bola * 3.0, 100.0)))
        } else {
            500.0
        }),
    );
    p(
        "pocaoMin",
        n(if medido {
            max2(50.0, arred(uso_pot, 50.0))
        } else {
            200.0
        }),
    );
    p(
        "pocaoLote",
        n(if medido {
            min2(5000.0, max2(100.0, arred(uso_pot * 3.0, 50.0)))
        } else {
            300.0
        }),
    );
    p("reservaOuro", n(max2(200_000.0, js::round(ouro_max * 0.2))));
    p("maxGastoHora", n(max_gasto));
    Plano {
        json: Value::Object(o),
        ouro_max,
        max_gasto_hora: max_gasto,
    }
}

/// `planoCompra(conta)`, sem consumo medido e sem `ouroMax` anterior.
pub fn plano_compra(d: &Value, r: &Relogio, conta: &ContaViva) -> Value {
    plano_compra_com(d, r, conta, &[], 0.0).0
}

/// `planoCompra` com o consumo (`conta.cons.ev`) e o `conta.ouroMax` de antes. Devolve o
/// plano e o novo `ouroMax` (o legado o grava na conta).
pub fn plano_compra_com(
    d: &Value,
    r: &Relogio,
    conta: &ContaViva,
    ev: &[Consumo],
    ouro_max_antes: f64,
) -> (Value, f64) {
    let p = plano_interno(d, r, conta, ev, ouro_max_antes);
    (p.json, p.ouro_max)
}

/// `precosEmUso(conta)`: `{pot, ball}` com o preço da poção e da bola do plano.
pub fn precos_em_uso(d: &Value, r: &Relogio, conta: &ContaViva) -> Value {
    let p = plano_compra(d, r, conta);
    json!({
        "pot": js::json_num(num_ou_0(p.get("pocaoPreco"))),
        "ball": js::json_num(num_ou_0(p.get("bolaPreco"))),
    })
}

/// Plano do laço: lê e grava `ouroMax` nas marcas da conta.
fn plano_do_laco(
    d: &Value,
    laco: &mut EstadoLaco,
    r: &Relogio,
    conta: &ContaViva,
    ev: &[Consumo],
) -> Plano {
    let p = plano_interno(d, r, conta, ev, laco.marca(&conta.nick, "ouroMax"));
    laco.marcar(&conta.nick, "ouroMax", p.ouro_max);
    p
}

// ---------- motorCompra ----------

pub fn motor_compra(
    d: &Value,
    laco: &mut EstadoLaco,
    r: &Relogio,
    conta: &ContaViva,
) -> Vec<Decisao> {
    motor_compra_com(d, laco, r, conta, &[])
}

/// `motorCompra` com o consumo medido. Em sombra a compra é dada como `ok`; veja o relatório
/// da etapa 4 para o que a etapa 5 (laço) precisa tratar depois da resposta do jogo.
pub fn motor_compra_com(
    d: &Value,
    laco: &mut EstadoLaco,
    r: &Relogio,
    conta: &ContaViva,
    ev: &[Consumo],
) -> Vec<Decisao> {
    let nick = conta.nick.as_str();
    let plano = plano_do_laco(d, laco, r, conta, ev);
    let c = &plano.json;
    if !js::verdadeiro(c.get("ativo")) || !js::verdadeiro(Some(&conta.welcome)) {
        return Vec::new();
    }
    let auto = js::verdadeiro(c.get("auto"));
    let ouro = num(caminho(&conta.ultimo, &["gold"]));
    let agora = r.agora_ms as f64;
    let mut saida = Vec::new();
    let alvos = [
        ("ball", "bolaId", "bolaMin", "bolaLote", "bolaTotal"),
        ("item", "pocaoId", "pocaoMin", "pocaoLote", "pocaoTotal"),
    ];
    for (kind, k_id, k_min, k_lote, k_total) in alvos {
        let id = c.get(k_id).filter(|i| existe(Some(i)));
        let Some(id) = id else { continue };
        let ball = kind == "ball";
        let cat = lista(&conta.welcome, if ball { "bolas" } else { "itens" })
            .iter()
            .find(|x| igual_frouxo(x.get("id"), Some(id)));
        let Some(cat) = cat else { continue };
        let preco = js::mais(cat.get("preco"));
        if preco.is_nan() || preco <= 0.0 || (ball && !js::verdadeiro(cat.get("compravel"))) {
            continue;
        }
        let Some(tem) = estoque(conta, ball, Some(id), cat.get("nome")) else {
            continue;
        };
        let min = js::mais(c.get(k_min));
        let total = c.get(k_total).filter(|t| existe(Some(t)));
        let segue = if auto {
            total.is_none() || js::mais(total) >= min
        } else {
            tem >= min
        };
        if segue {
            continue; // tem estoque de sobra nas ligadas: não compra
        }
        let qty = max2(1.0, min2(js::mais(c.get(k_lote)), 5000.0));
        let custo = qty * preco;
        let nome = js::texto(cat.get("nome"));
        let reserva = js::mais(c.get("reservaOuro"));
        // Ouro abaixo da reserva: só pula esta compra
        if ouro.is_none_or(|o| o - custo < reserva) {
            let aviso = laco.marca(nick, "avisoReserva");
            if aviso == 0.0 || agora - aviso > 30.0 * 60e3 {
                laco.marcar(nick, "avisoReserva", agora);
                saida.push(Decisao::Registro {
                    nick: nick.into(),
                    tipo: "aviso".into(),
                    texto: format!(
                        "Não comprei {} {}: o ouro ficaria abaixo da reserva de {}. Tento de novo quando o ouro subir.",
                        js::numero(qty), nome, fmt_pt(reserva)
                    ),
                    ok: false,
                });
            }
            continue;
        }
        if gastos_hora(d, nick, agora) + custo > plano.max_gasto_hora {
            saida.push(Decisao::Registro {
                nick: nick.into(),
                tipo: "aviso".into(),
                texto: format!(
                    "Teto de {} de ouro por hora atingido; compra de {} adiada.",
                    fmt_pt(plano.max_gasto_hora),
                    nome
                ),
                ok: false,
            });
            return saida;
        }
        let extra = if auto {
            let como = c.get(if ball { "bolaComo" } else { "pocaoComo" });
            format!(
                "; {}; somando todas as ligadas, {}",
                js::texto(como),
                js::texto(total)
            )
        } else {
            String::new()
        };
        // Depende da resposta do jogo (etapa 5): `a` em `D.acoes` com `custo` e `conf`, os
        // contadores do dia (`compras`, `comprasItens`) e a conferência 15 s depois.
        saida.push(Decisao::Comando {
            nick: nick.into(),
            comando: json!({"t": "shop.buy", "kind": kind, "id": cat.get("id").cloned().unwrap_or(Value::Null), "qty": js::json_num(qty)}),
            registro: Some(Registro {
                custo: Some(custo),
                tipo: "compra".into(),
                texto_ok: format!(
                    "Comprou {} {} por {} de ouro (tinha {}{}).",
                    js::numero(qty),
                    nome,
                    fmt_pt(custo),
                    js::numero(tem),
                    extra
                ),
                texto_falha: format!("Falhou ao comprar {nome} ({{r}})."),
            }),
        });
        return saida; // uma compra por ciclo
    }
    saida
}

// ---------- motorPocao ----------

pub fn motor_pocao(
    d: &Value,
    laco: &mut EstadoLaco,
    r: &Relogio,
    conta: &ContaViva,
) -> Vec<Decisao> {
    motor_pocao_com(d, laco, r, conta, &[])
}

/// `motorPocao` com o consumo medido. `pocaoFalhas` sobe a cada comando enviado, qualquer que
/// seja a resposta (como no legado), então a sombra não precisa supor `ok` aqui.
pub fn motor_pocao_com(
    d: &Value,
    laco: &mut EstadoLaco,
    r: &Relogio,
    conta: &ContaViva,
    ev: &[Consumo],
) -> Vec<Decisao> {
    let nick = conta.nick.as_str();
    let cfg = config::efetiva(d, nick);
    if !js::verdadeiro(caminho(&cfg, &["pocao", "ativo"])) || !js::verdadeiro(Some(&conta.welcome))
    {
        return Vec::new();
    }
    let Some(a) =
        caminho(&conta.ultimo, &["hdx", "eu", "automation"]).filter(|a| js::verdadeiro(Some(a)))
    else {
        return Vec::new();
    };
    let Some(ids) = a.get("potionIds").and_then(Value::as_array) else {
        return Vec::new();
    };
    let agora = r.agora_ms as f64;
    if agora - laco.marca(nick, "pocaoMexida") < 10.0 * 60e3 {
        return Vec::new();
    }
    let plano = plano_do_laco(d, laco, r, conta, ev);
    let Some(m) = plano
        .json
        .get("melhorPocao")
        .filter(|m| js::verdadeiro(Some(m)))
    else {
        return Vec::new();
    };
    let itens = caminho(&conta.ultimo, &["hdx", "eu", "items"]).filter(|i| js::verdadeiro(Some(i)));
    let tem_de = |id: &Value| -> bool {
        // `+(itens[id] ?? 0) || 0` > 0
        let v = itens
            .and_then(|i| indexar(i, id))
            .filter(|v| existe(Some(v)));
        let n = js::mais(Some(v.unwrap_or(&json!(0))));
        !n.is_nan() && n > 0.0
    };
    let s = |v: &Value| js::texto(Some(v));
    let m_id = m.get("id").cloned().unwrap_or(Value::Null);
    let outras: Vec<&Value> = ids.iter().filter(|id| s(id) != s(&m_id)).collect();
    let mut nova: Vec<Value> = outras
        .iter()
        .filter(|i| tem_de(i))
        .map(|i| (*i).clone())
        .collect();
    nova.push(if matches!(ids.first(), Some(Value::String(_))) {
        Value::String(s(&m_id))
    } else {
        m_id.clone()
    });
    nova.extend(outras.iter().filter(|i| !tem_de(i)).map(|i| (*i).clone()));
    let junta = |v: &[Value]| v.iter().map(s).collect::<Vec<_>>().join(",");
    if junta(&nova) == junta(ids) {
        laco.marcar(nick, "pocaoFalhas", 0.0);
        return Vec::new();
    }
    if laco.marca(nick, "pocaoFalhas") >= 3.0 {
        return Vec::new(); // o jogo não aceitou três vezes: para de insistir
    }
    let mut cmd = Map::new();
    cmd.insert("t".into(), json!("auto.set"));
    if let Value::Object(o) = a {
        for (k, v) in o {
            cmd.insert(k.clone(), v.clone());
        }
    }
    cmd.insert("potionIds".into(), Value::Array(nova.clone()));
    laco.marcar(nick, "pocaoMexida", agora);
    let falhas = laco.marca(nick, "pocaoFalhas");
    laco.marcar(nick, "pocaoFalhas", falhas + 1.0);
    let nome = |id: &Value| -> String {
        let n = lista(&conta.welcome, "itens")
            .iter()
            .find(|i| i.get("id").map(s) == Some(s(id)))
            .and_then(|i| i.get("nome"));
        match n.filter(|n| js::verdadeiro(Some(n))) {
            Some(n) => js::texto(Some(n)),
            None => s(id),
        }
    };
    let sobra = outras.iter().any(|i| tem_de(i));
    let ordem = nova.iter().map(nome).collect::<Vec<_>>().join(" → ");
    let custo_mil = js::round(js::mais(m.get("custoPorMilHp")));
    let texto_ok = format!(
        "Ordem das poções: {}. {} é a mais barata por HP curado ({} de ouro por 1.000 HP){}.",
        ordem,
        js::texto(m.get("nome")),
        fmt_pt(custo_mil),
        if sobra {
            "; antes o jogo gasta as sobras que você já tem"
        } else {
            ""
        }
    );
    vec![Decisao::Comando {
        nick: nick.into(),
        comando: Value::Object(cmd),
        registro: Some(Registro {
            custo: None,
            tipo: "pocao".into(),
            texto_ok,
            texto_falha: "Não consegui mudar a ordem das poções ({r}).".into(),
        }),
    }]
}

#[cfg(test)]
mod testes {
    use super::*;

    fn rel() -> Relogio {
        Relogio {
            agora_ms: 10_000_000_000,
            fuso_ms: -3 * 3_600_000,
        }
    }

    fn dados(compra: Value) -> Value {
        config::carregar(&json!({"config": {"Ana": {"compra": compra}}}))
    }

    fn conta(ouro: i64, bolas: i64) -> ContaViva {
        ContaViva {
            nick: "Ana".into(),
            ultimo: json!({
                "gold": ouro,
                "ativo": {"hp": "100/1000"},
                "hdx": {"eu": {"balls": {"4": bolas}, "items": {"70": 5, "71": 0},
                    "automation": {"potionIds": [71, 70], "x": 1}}},
                "bolas": [{"nome": "Poke Ball", "q": bolas, "on": true}],
                "pocoes": [{"nome": "Potion A", "q": 5, "on": true},
                           {"nome": "Potion B", "q": 0, "on": true}]
            }),
            welcome: json!({
                "bolas": [{"id": 4, "nome": "Poke Ball", "preco": 100, "compravel": true}],
                "itens": [
                    {"id": 70, "nome": "Potion A", "cat": "heal", "preco": 500, "healAmount": 200},
                    {"id": 71, "nome": "Potion B", "cat": "heal", "preco": 100, "healAmount": 200}
                ]
            }),
            ..Default::default()
        }
    }

    #[test]
    fn formata_pt_br() {
        assert_eq!(fmt_pt(1_234_567.0), "1.234.567");
        assert_eq!(fmt_pt(1234.5678), "1.234,568");
        assert_eq!(fmt_pt(0.5), "0,5");
        assert_eq!(fmt_pt(-1500.0), "-1.500");
        assert_eq!(fmt_pt(999.0), "999");
        assert_eq!(fmt_pt(f64::NAN), "NaN");
    }

    #[test]
    fn plano_manual_devolve_a_config() {
        let d = dados(json!({"auto": false, "ativo": true}));
        let p = plano_compra(&d, &rel(), &conta(1, 1));
        assert_eq!(p.get("auto"), Some(&json!(false)));
        assert_eq!(p.get("bolaMin"), Some(&json!(300)));
    }

    #[test]
    fn plano_auto_sem_medida() {
        let d = dados(json!({"auto": true, "ativo": true}));
        let p = plano_compra(&d, &rel(), &conta(5_000_000, 50));
        assert_eq!(p["medido"], json!(false));
        assert_eq!(p["bolaId"], json!(4));
        assert_eq!(p["bolaTotal"], json!(50));
        assert_eq!(p["pocaoTotal"], json!(5));
        // B custa 100 por 200 de cura útil (limiar 50% de 1000 = falta 500): mais barata
        assert_eq!(p["melhorPocao"]["id"], json!(71));
        assert_eq!(p["pocaoId"], json!(71));
        assert_eq!(p["pocaoComo"], json!("é a mais barata por HP curado"));
        assert_eq!(p["maxGastoHora"], Value::Null);
        assert_eq!(p["reservaOuro"], json!(1_000_000));
        let e = precos_em_uso(&d, &rel(), &conta(5_000_000, 50));
        assert_eq!(e, json!({"pot": 100, "ball": 100}));
    }

    #[test]
    fn compra_bola_e_respeita_reserva() {
        let d = dados(json!({"auto": true, "ativo": true}));
        let mut laco = EstadoLaco::default();
        let v = motor_compra(&d, &mut laco, &rel(), &conta(5_000_000, 50));
        let Some(Decisao::Comando {
            comando, registro, ..
        }) = v.first()
        else {
            panic!("esperava comando: {v:?}");
        };
        assert_eq!(
            comando,
            &json!({"t": "shop.buy", "kind": "ball", "id": 4, "qty": 500})
        );
        let texto = registro
            .as_ref()
            .map(|x| x.texto_ok.clone())
            .unwrap_or_default();
        assert_eq!(
            texto,
            "Comprou 500 Poke Ball por 50.000 de ouro (tinha 50; é a ligada com mais estoque; somando todas as ligadas, 50)."
        );
        // ouro baixo: aviso uma vez, depois silêncio por 30 min
        let mut laco = EstadoLaco::default();
        let v = motor_compra(&d, &mut laco, &rel(), &conta(240_000, 50));
        assert!(matches!(&v[0], Decisao::Registro { tipo, ok: false, .. } if tipo == "aviso"));
        let de_novo = motor_compra(&d, &mut laco, &rel(), &conta(240_000, 50));
        assert!(
            !matches!(de_novo.first(), Some(Decisao::Registro { tipo, .. }) if tipo == "aviso")
        );
    }

    #[test]
    fn pocao_reordena_uma_vez() {
        let d = dados(json!({"auto": true}));
        let mut laco = EstadoLaco::default();
        let c = conta(5_000_000, 500);
        let v = motor_pocao(&d, &mut laco, &rel(), &c);
        let Some(Decisao::Comando {
            comando, registro, ..
        }) = v.first()
        else {
            panic!("esperava comando: {v:?}");
        };
        // 70 tem estoque (5), 71 não: sobra primeiro, a mais barata (71) por último
        assert_eq!(comando["potionIds"], json!([70, 71]));
        assert_eq!(comando["x"], json!(1));
        assert_eq!(comando["t"], json!("auto.set"));
        let ok = registro
            .as_ref()
            .map(|x| x.texto_ok.clone())
            .unwrap_or_default();
        assert_eq!(
            ok,
            "Ordem das poções: Potion A → Potion B. Potion B é a mais barata por HP curado (500 de ouro por 1.000 HP); antes o jogo gasta as sobras que você já tem."
        );
        assert_eq!(laco.marca("Ana", "pocaoFalhas"), 1.0);
        // dentro de 10 min não mexe
        assert!(motor_pocao(&d, &mut laco, &rel(), &c).is_empty());
    }
}

/// Validação com dados reais (fixtures fora do repositório): `NEXUS_FIXTURES_MOTOR` aponta
/// para a pasta com `nexus-janela.json`, `nexus-dados.json` e `estado.json`.
#[cfg(test)]
mod fixture {
    use super::*;

    fn primeira_diferenca(caminho: &str, a: &Value, b: &Value) -> Option<String> {
        match (a, b) {
            (Value::Object(x), Value::Object(y)) => {
                for (k, v) in x {
                    match y.get(k) {
                        None => return Some(format!("{caminho}/{k}: só no Rust ({v})")),
                        Some(w) => {
                            if let Some(d) = primeira_diferenca(&format!("{caminho}/{k}"), v, w) {
                                return Some(d);
                            }
                        }
                    }
                }
                y.keys()
                    .find(|k| !x.contains_key(*k))
                    .map(|k| format!("{caminho}/{k}: só no legado"))
            }
            (Value::Array(x), Value::Array(y)) => {
                for (i, (v, w)) in x.iter().zip(y).enumerate() {
                    if let Some(d) = primeira_diferenca(&format!("{caminho}[{i}]"), v, w) {
                        return Some(d);
                    }
                }
                (x.len() != y.len())
                    .then(|| format!("{caminho}: tamanho {} x {}", x.len(), y.len()))
            }
            _ => (a != b).then(|| format!("{caminho}: rust={a} legado={b}")),
        }
    }

    #[test]
    #[ignore = "precisa de NEXUS_FIXTURES_MOTOR"]
    fn plano_compra_igual_ao_legado() {
        let dir = std::env::var("NEXUS_FIXTURES_MOTOR").expect("NEXUS_FIXTURES_MOTOR");
        let ler = |f: &str| -> Value {
            let t = std::fs::read_to_string(format!("{dir}/{f}")).expect("ler fixture");
            serde_json::from_str(&t).expect("json")
        };
        let janela = ler("nexus-janela.json");
        let d = config::carregar(&ler("nexus-dados.json"));
        let estado = ler("estado.json");
        let r = Relogio {
            agora_ms: janela["agora"].as_i64().expect("agora"),
            fuso_ms: -3 * 3_600_000,
        };
        let mut falhas = 0;
        for cj in janela["contas"].as_array().expect("contas") {
            let conta = ContaViva::de_json(cj).expect("conta");
            let legado = estado["contas"]
                .as_array()
                .expect("contas")
                .iter()
                .find(|c| c["nick"] == cj["nick"])
                .map(|c| c["plano"]["compra"].clone())
                .expect("conta no estado");
            // A janela não traz `conta.cons.ev` nem `ouroMax`: reconstrói do que o legado
            // mostrou no plano (usado1h por bola/poção e a reserva = 20% do ouroMax).
            let mut ev: Vec<Consumo> = Vec::new();
            for (campo, tipo) in [("bolasLigadas", "bola"), ("pocoesLigadas", "pocao")] {
                for x in legado[campo].as_array().into_iter().flatten() {
                    let q = x["usado1h"].as_f64().unwrap_or(0.0);
                    if q > 0.0 {
                        let k = format!("{tipo}|{}", norm(x.get("nome")));
                        ev.push((r.agora_ms - 1000, k, q));
                    }
                }
            }
            let ouro_max = legado["reservaOuro"].as_f64().unwrap_or(0.0) / 0.2;
            let rust = plano_compra_com(&d, &r, &conta, &ev, ouro_max).0;
            match primeira_diferenca("", &rust, &legado) {
                None => println!("{}: IGUAL", conta.nick),
                Some(x) => {
                    falhas += 1;
                    println!("{}: {x}", conta.nick);
                }
            }
        }
        assert_eq!(falhas, 0, "contas com diferença");
    }
}
