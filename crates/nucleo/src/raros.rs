//! Raros e shiny: o `raros.js` do legado, sem I/O.
//!
//! O estado é o mesmo objeto do `raros-estado.json` (`raros`, `em`, `hist`, `histEm`, `acc`),
//! guardado como JSON. Aqui ficam o merge dos deltas, a conta de probabilidade do shiny e a
//! montagem do `/api/raros2`; o que depende de banco ou de preço de mercado chega por parâmetro.

use serde_json::{json, Map, Value};

use crate::js;

/// O relay manda no máximo isto; o resto é descartado, como no legado.
const LIMITE_ITENS: usize = 400;
const DIA_MS: f64 = 86_400_000.0;
const HORA_MS: f64 = 3_600_000.0;
/// Meta mensal padrão de gemas por conta e nota mínima padrão das cartas.
const META_PADRAO: f64 = 1000.0;
pub const NOTA_MIN_PADRAO: f64 = 3.5;
/// Chance de shiny quando o histórico não traz a medida (1 em 24 mil).
const CHANCE_SHINY_PADRAO: f64 = 1.0 / 24_000.0;
/// Quantos arremessos de ultra ball custa capturar um shiny: mínimo, mediana e máximo.
const CAPTURA_PADRAO: [f64; 3] = [1_638_229.0, 2_553_200.0, 4_517_516.0];

/// `+x || 0`.
fn num0(v: Option<&Value>) -> f64 {
    let x = js::mais(v);
    if x.is_nan() {
        0.0
    } else {
        x
    }
}

fn numero(v: &Value, campo: &str) -> f64 {
    num0(v.get(campo))
}

/// Dá acesso ao objeto em `v` (ou a um objeto vazio, se `v` não for objeto) e devolve o
/// resultado de `f`. Tira o valor do lugar e o recoloca, para não precisar de caminho de pânico.
fn com_objeto<T>(v: &mut Value, f: impl FnOnce(&mut Map<String, Value>) -> T) -> T {
    let mut m = match std::mem::take(v) {
        Value::Object(m) => m,
        _ => Map::new(),
    };
    let r = f(&mut m);
    *v = Value::Object(m);
    r
}

/// O mesmo para o campo `k` de um objeto (criado se faltar).
fn com_filho<T>(
    m: &mut Map<String, Value>,
    k: &str,
    f: impl FnOnce(&mut Map<String, Value>) -> T,
) -> T {
    com_objeto(m.entry(k).or_insert(Value::Null), f)
}

/// `POST /api/raros/ingest`. Devolve a quantidade de itens guardados.
pub fn ingerir(estado: &mut Value, corpo: &Value, agora: i64) -> Result<usize, &'static str> {
    let itens = corpo
        .pointer("/raros/itens")
        .and_then(Value::as_array)
        .ok_or("raros inválido")?;
    let mut raros = corpo
        .get("raros")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let itens: Vec<Value> = itens.iter().take(LIMITE_ITENS).cloned().collect();
    let n = itens.len();
    raros.insert("itens".into(), Value::Array(itens));

    let historico = corpo.get("historico");
    let dia = historico
        .and_then(|h| h.get("dia"))
        .filter(|d| js::verdadeiro(Some(d)));
    com_objeto(estado, |r| {
        r.insert("raros".into(), Value::Object(raros));
        r.insert("em".into(), Value::from(agora));
        let (Some(h), Some(dia)) = (historico, dia) else {
            return;
        };
        r.insert("hist".into(), h.clone());
        r.insert("histEm".into(), Value::from(agora));
        let Some(dia) = dia.as_object() else {
            return;
        };
        com_filho(r, "acc", |acc| {
            for (nome, dias) in dia {
                let Some(dias) = dias.as_object() else {
                    continue;
                };
                let a = acc
                    .entry(nome.to_lowercase())
                    .or_insert_with(|| json!({ "nome": nome, "dias": {} }));
                com_objeto(a, |a| {
                    com_filho(a, "dias", |a_dias| {
                        for (dia, v) in dias {
                            somar_dia(a_dias, dia, num0(v.get("bolas")), num0(v.get("shiny")));
                        }
                    });
                });
            }
        });
    });
    Ok(n)
}

/// Aplica a leitura bruta do dia: soma a diferença desde a última leitura (`lb`/`ls`) ou,
/// se o contador reiniciou (leitura menor), a leitura inteira.
fn somar_dia(dias: &mut Map<String, Value>, dia: &str, cb: f64, cs: f64) {
    let x = dias.entry(dia).or_insert(Value::Null);
    if !x.is_object() {
        *x = json!({ "bolas": js::json_num(cb), "shiny": js::json_num(cs),
                     "lb": js::json_num(cb), "ls": js::json_num(cs) });
        return;
    }
    let (bolas, shiny) = (numero(x, "bolas"), numero(x, "shiny"));
    let (lb, ls) = (numero(x, "lb"), numero(x, "ls"));
    com_objeto(x, |m| {
        m.insert(
            "bolas".into(),
            js::json_num(bolas + if cb >= lb { cb - lb } else { cb }),
        );
        m.insert(
            "shiny".into(),
            js::json_num(shiny + if cs >= ls { cs - ls } else { cs }),
        );
        m.insert("lb".into(), js::json_num(cb));
        m.insert("ls".into(), js::json_num(cs));
    });
}

/// `POST /api/shiny/registrar`. `hoje` é a data UTC (`AAAA-MM-DD`) usada sem `dia` no corpo.
pub fn registrar_shiny(
    estado: &mut Value,
    corpo: &Value,
    hoje: &str,
) -> Result<Value, &'static str> {
    let nick = corpo.get("nick").filter(|v| js::verdadeiro(Some(v)));
    let n = nick.map(|v| js::texto(Some(v)).to_lowercase());
    let Some(n) = n.filter(|n| !n.is_empty()) else {
        return Err("nick obrigatório");
    };
    let dia = corpo
        .get("dia")
        .filter(|v| js::verdadeiro(Some(v)))
        .map_or_else(|| hoje.to_owned(), |v| js::texto(Some(v)));
    let nome = nick.cloned().unwrap_or(Value::Null);
    let (shiny, bolas) = com_objeto(estado, |r| {
        com_filho(r, "acc", |acc| {
            let a = acc
                .entry(n.clone())
                .or_insert_with(|| json!({ "nome": nome, "dias": {} }));
            com_objeto(a, |a| {
                com_filho(a, "dias", |dias| {
                    let x = dias
                        .entry(dia.clone())
                        .or_insert_with(|| json!({ "bolas": 0, "shiny": 0, "lb": 0, "ls": 0 }));
                    let shiny = numero(x, "shiny") + num0(corpo.get("shiny")).max(0.0);
                    let bolas = numero(x, "bolas") + num0(corpo.get("bolas")).max(0.0);
                    com_objeto(x, |m| {
                        m.insert("shiny".into(), js::json_num(shiny));
                        m.insert("bolas".into(), js::json_num(bolas));
                    });
                    (shiny, bolas)
                })
            })
        })
    });
    Ok(json!({ "ok": true, "nick": n, "dia": dia,
               "shiny": js::json_num(shiny), "bolas": js::json_num(bolas) }))
}

/// Um modo de cálculo do shiny: a chance `p` por arremesso.
struct Modo {
    p: f64,
}

impl Modo {
    /// Arremessos para ter chance `q` de ao menos um shiny.
    fn n(&self, q: f64) -> f64 {
        js::ln(1.0 - q) / js::ln(1.0 - self.p)
    }
}

fn dias_da_conta(a: &Value) -> Vec<(String, f64, f64)> {
    let mut v: Vec<(String, f64, f64)> = a
        .get("dias")
        .and_then(Value::as_object)
        .map(|m| {
            m.iter()
                .map(|(d, x)| (d.clone(), numero(x, "bolas"), numero(x, "shiny")))
                .collect()
        })
        .unwrap_or_default();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v
}

/// `GET /api/shiny`. `captura` é `[mín, mediana, máx]` de `dim_especie`, se o banco tiver.
pub fn shiny(estado: &Value, nick: &str, captura: Option<[f64; 3]>) -> Value {
    let hist = estado.get("hist").filter(|h| js::verdadeiro(Some(h)));
    let acc = estado.get("acc").and_then(Value::as_object);
    if hist.is_none() && acc.is_none_or(Map::is_empty) {
        return json!({ "vazio": true,
            "aviso": "O servidor ainda não recebeu o histórico de arremessos. Reinicie o Nexus com o relay novo (envia a cada 2 min)." });
    }
    let nick = nick.to_lowercase();
    let p_ver = hist
        .and_then(|h| h.get("chanceShiny"))
        .map(|c| js::mais(Some(c)))
        .filter(|c| *c > 0.0)
        .unwrap_or(CHANCE_SHINY_PADRAO);
    let cap = captura.unwrap_or(CAPTURA_PADRAO);
    let ver = Modo { p: p_ver };
    let capturar = Modo { p: 1.0 / cap[1] };

    let mut contas: Vec<(f64, Value)> = Vec::new();
    for (k, a) in acc.into_iter().flatten() {
        if !nick.is_empty() && *k != nick {
            continue;
        }
        let nome = a
            .get("nome")
            .filter(|v| js::verdadeiro(Some(v)))
            .map_or_else(|| k.clone(), |v| js::texto(Some(v)));
        let dias = dias_da_conta(a);
        let (mut desde, mut achou) = (0.0, None);
        for (d, bolas, shiny) in dias.iter().rev() {
            if *shiny > 0.0 {
                achou = Some(d.clone());
                break;
            }
            desde += bolas;
        }
        let total: f64 = dias.iter().map(|d| d.1).sum();
        let shinies: f64 = dias.iter().map(|d| d.2).sum();
        if total == 0.0 && shinies == 0.0 {
            continue;
        }
        let ritmo = hist
            .and_then(|h| h.get("ritmo"))
            .and_then(|r| {
                [&nome, k]
                    .into_iter()
                    .map(|c| r.get(c.as_str()))
                    .find(|v| v.is_some_and(|v| js::verdadeiro(Some(v))))
                    .flatten()
            })
            .cloned()
            .unwrap_or_else(|| json!({}));
        let ou = |campo: &str, padrao: f64| {
            ritmo
                .get(campo)
                .filter(|v| js::verdadeiro(Some(v)))
                .map_or(padrao, |v| js::mais(Some(v)))
        };
        let bph = ou("kph", 0.0) * ou("bolasKill", 1.0).max(1.0);
        let calc = |m: &Modo| {
            let esp = total * m.p;
            let dp = 1.96 * esp.sqrt();
            let eta = |q: f64| {
                let al = m.n(q);
                if desde >= al {
                    js::json_num(0.0)
                } else if bph > 0.0 {
                    js::json_num((al - desde) / bph)
                } else {
                    Value::Null
                }
            };
            let azar = js::pow(1.0 - m.p, desde);
            json!({ "prob": js::json_num(1.0 - azar), "azar": js::json_num(azar),
                    "esperado": js::json_num(esp),
                    "ic": [js::json_num((esp - dp).max(0.0)), js::json_num(esp + dp)],
                    "eta50h": eta(0.5), "eta90h": eta(0.9), "eta99h": eta(0.99) })
        };
        let serie: Vec<Value> = dias
            .iter()
            .skip(dias.len().saturating_sub(30))
            .map(
                |(d, b, s)| json!({ "d": d, "bolas": js::json_num(*b), "shiny": js::json_num(*s) }),
            )
            .collect();
        contas.push((
            desde,
            json!({ "nick": nome, "bolasDesde": js::json_num(desde),
                    "ultimoShiny": achou, "totalBolas": js::json_num(total),
                    "shinies": js::json_num(shinies), "bph": js::json_num(bph),
                    "ver": calc(&ver), "capturar": calc(&capturar), "serie": serie }),
        ));
    }
    contas.sort_by(|a, b| b.0.total_cmp(&a.0));

    let max = contas.iter().map(|c| c.0 * 1.15).fold(1.0, f64::max);
    let curva = |m: &Modo| {
        let mx = m.n(0.99).max(max);
        (0..=80)
            .map(|i| {
                let x = mx * f64::from(i) / 80.0;
                json!([
                    js::json_num(js::round(x)),
                    js::json_num(1.0 - js::pow(1.0 - m.p, x))
                ])
            })
            .collect::<Vec<_>>()
    };
    let modo = |m: &Modo| {
        json!({ "p": js::json_num(m.p), "umEm": js::json_num(js::round(1.0 / m.p)),
                "N50": js::json_num(js::round(m.n(0.5))), "N90": js::json_num(js::round(m.n(0.9))),
                "N99": js::json_num(js::round(m.n(0.99))), "curva": curva(m) })
    };
    let mut modo_capturar = modo(&capturar);
    if let Some(o) = modo_capturar.as_object_mut() {
        o.insert("umEmMin".into(), js::json_num(cap[0]));
        o.insert("umEmMax".into(), js::json_num(cap[2]));
    }
    let pequena = contas
        .iter()
        .any(|c| c.1.get("totalBolas").and_then(Value::as_f64).unwrap_or(0.0) < 5000.0);
    json!({
        "vazio": contas.is_empty(),
        "modos": { "ver": modo(&ver), "capturar": modo_capturar },
        "contas": contas.into_iter().map(|c| c.1).collect::<Vec<_>>(),
        "dadosEm": estado.get("histEm").filter(|v| js::verdadeiro(Some(v))).cloned().unwrap_or(Value::Null),
        "persistido": true,
        "aviso": if pequena { json!("Amostra pequena: poucos arremessos registrados.") } else { Value::Null },
    })
}

// ---------- /api/raros2 ----------

/// As cartas valiosas de uma conta (o retorno do `pokemonsValiosos` do legado).
#[derive(Debug, Clone, Default)]
pub struct Valiosos {
    pub lista: Vec<Value>,
    pub total: usize,
    pub quase: usize,
}

/// Uma linha do histórico de `fato_cofre`.
#[derive(Debug, Clone)]
pub struct LinhaCofre {
    pub ts: i64,
    pub nick: String,
    pub itens: i64,
    pub pokemons: i64,
    pub shiny: i64,
    /// O `total` gravado (o legado grava `itens + pokemons`, sem o shiny).
    pub total: i64,
}

fn verdade(v: Option<&Value>) -> bool {
    js::verdadeiro(v)
}

/// Valor de uma conta nos itens: soma de `gemaMedia * qtd` (sem diferenciar maiúsculas).
fn qtd_do_item(item: &Value, nick: &str) -> f64 {
    item.get("contas")
        .and_then(Value::as_object)
        .and_then(|c| {
            c.iter()
                .find(|(k, _)| k.to_lowercase() == nick.to_lowercase())
        })
        .map_or(0.0, |(_, q)| js::mais(Some(q)))
}

fn itens_do_estado(estado: &Value) -> &[Value] {
    estado
        .pointer("/raros/itens")
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

/// A lista de contas do cofre: as do relay de raros e as do estado de equipes que têm dados,
/// com a primeira grafia vista de cada uma.
pub fn contas_do_cofre(estado: &Value, contas_equipes: &[(String, Value)]) -> Vec<String> {
    let por_conta = estado
        .pointer("/raros/porConta")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice);
    let nome_de = |p: &Value| p.get("nick").map(|n| js::texto(Some(n)));
    let mut com_dados: Vec<String> = por_conta
        .iter()
        .filter(|p| js::mais(p.get("qtd")) > 0.0 || js::mais(p.get("gemas")) > 0.0)
        .filter_map(nome_de)
        .map(|n| n.to_lowercase())
        .collect();
    com_dados.extend(
        contas_equipes
            .iter()
            .filter(|(_, c)| {
                c.get("pokemons")
                    .and_then(Value::as_array)
                    .is_some_and(|p| !p.is_empty())
            })
            .map(|(n, _)| n.to_lowercase()),
    );
    let mut vistos: Vec<String> = Vec::new();
    for n in por_conta
        .iter()
        .filter_map(nome_de)
        .chain(contas_equipes.iter().map(|(n, _)| n.clone()))
    {
        let k = n.to_lowercase();
        if !n.is_empty()
            && k != "conta da ia"
            && com_dados.contains(&k)
            && !vistos.iter().any(|v| v.to_lowercase() == k)
        {
            vistos.push(n);
        }
    }
    vistos
}

/// Parâmetros de `GET /api/raros2`.
pub struct PedidoRaros2<'a> {
    pub nick: &'a str,
    pub nota_min: f64,
    /// `?meta=` (0 ou ausente: usa a do relay, ou 1000).
    pub meta: f64,
    pub agora: i64,
}

/// `GET /api/raros2`, com as cartas valiosas e o histórico já resolvidos pelo chamador.
/// `icone` devolve o ícone de um item pelo id.
pub fn raros2(
    estado: &Value,
    contas: &[String],
    valiosos: &dyn Fn(&str) -> Valiosos,
    historico: &[LinhaCofre],
    icone: &dyn Fn(i64) -> Option<String>,
    p: &PedidoRaros2<'_>,
) -> Value {
    let rr = estado.get("raros").filter(|v| verdade(Some(v)));
    let itens_rr = itens_do_estado(estado);
    let nick = p.nick.to_lowercase();
    let alvo: Vec<&String> = contas
        .iter()
        .filter(|n| nick.is_empty() || n.to_lowercase() == nick)
        .collect();
    let meta_rr = rr.and_then(|r| r.get("meta")).filter(|v| verdade(Some(v)));
    let meta_conta = if p.meta != 0.0 && !p.meta.is_nan() {
        p.meta
    } else {
        meta_rr.map_or(META_PADRAO, |m| js::mais(Some(m)))
    };
    let meta = meta_conta
        * if alvo.is_empty() {
            1.0
        } else {
            alvo.len() as f64
        };

    // Itens raros somados sobre as contas do alvo.
    let mut itens: Vec<(f64, Value)> = Vec::new();
    for i in itens_rr {
        let q: f64 = alvo.iter().map(|n| qtd_do_item(i, n)).sum();
        if q.is_nan() || q <= 0.0 {
            continue;
        }
        let gema_media = i.get("gemaMedia");
        let valor = if verdade(gema_media) {
            js::round(js::mais(gema_media) * q)
        } else {
            0.0
        };
        let mut o = Map::new();
        let id = i.get("id").cloned();
        o.insert("id".into(), id.clone().unwrap_or(Value::Null));
        o.insert(
            "icone".into(),
            id.and_then(|v| v.as_i64())
                .and_then(icone)
                .map_or(Value::Null, Value::from),
        );
        o.insert("qtd".into(), js::json_num(q));
        for campo in [
            "nome",
            "contas",
            "gemaMedia",
            "gemaHoje",
            "ouroMedio",
            "varPct",
            "sinal",
            "dropsDia",
            "qtdFimMes",
        ] {
            if let Some(v) = i.get(campo) {
                o.insert(campo.into(), v.clone());
            }
        }
        o.insert("valorGemas".into(), js::json_num(valor));
        o.insert(
            "gemasProjetadas".into(),
            i.get("gemasProjetadas").cloned().unwrap_or(Value::Null),
        );
        itens.push((valor, Value::Object(o)));
    }
    itens.sort_by(|a, b| b.0.total_cmp(&a.0));

    // Pokémon valiosos das contas do alvo.
    let (mut pk, mut cartas, mut quase) = (Vec::<Value>::new(), 0usize, 0usize);
    for n in &alvo {
        let v = valiosos(n);
        pk.extend(v.lista);
        cartas += v.total;
        quase += v.quase;
    }
    let valor_de = |x: &Value| x.get("valorGemas").and_then(Value::as_f64).unwrap_or(0.0);
    pk.sort_by(|a, b| valor_de(b).total_cmp(&valor_de(a)));
    let eh_shiny = |x: &Value| x.get("shiny").and_then(Value::as_bool).unwrap_or(false);
    let v_itens: f64 = itens.iter().map(|i| i.0).sum();
    let v_pk: f64 = pk.iter().filter(|x| !eh_shiny(x)).map(valor_de).sum();
    let v_sh: f64 = pk.iter().filter(|x| eh_shiny(x)).map(valor_de).sum();
    let total = v_itens + v_pk + v_sh;

    let por_conta: Vec<Value> = contas
        .iter()
        .map(|n| {
            let it: f64 = itens_rr
                .iter()
                .map(|i| {
                    if verdade(i.get("gemaMedia")) {
                        js::mais(i.get("gemaMedia")) * qtd_do_item(i, n)
                    } else {
                        0.0
                    }
                })
                .sum();
            let v = valiosos(n);
            let soma = |sh: bool| -> f64 {
                v.lista
                    .iter()
                    .filter(|x| eh_shiny(x) == sh)
                    .map(valor_de)
                    .sum()
            };
            let (a, b, c) = (js::round(it), js::round(soma(false)), js::round(soma(true)));
            json!({ "nick": n, "itens": js::json_num(a), "pokemons": js::json_num(b),
                    "shiny": js::json_num(c), "total": js::json_num(a + b + c) })
        })
        .collect();

    // Histórico por hora, dos últimos 30 dias (o filtro de data é do chamador).
    let mut por_hora: Vec<(i64, f64, f64, f64)> = Vec::new();
    for h in historico {
        if !nick.is_empty() && h.nick.to_lowercase() != nick {
            continue;
        }
        let k = h.ts.div_euclid(3_600_000);
        let pos = por_hora.iter().position(|x| x.0 == k).unwrap_or_else(|| {
            por_hora.push((k, 0.0, 0.0, 0.0));
            por_hora.len() - 1
        });
        let x = &mut por_hora[pos];
        x.1 += h.total as f64;
        x.2 += h.itens as f64;
        x.3 += (h.pokemons + h.shiny) as f64;
    }
    por_hora.sort_by_key(|x| x.0);
    let serie: Vec<Value> = por_hora
        .iter()
        .map(|(k, t, i, pk)| {
            json!({ "ts": k * 3_600_000, "total": js::json_num(*t),
                    "itens": js::json_num(*i), "pokemons": js::json_num(*pk) })
        })
        .collect();

    let mut ritmo: Option<f64> = None;
    let mut fonte: Option<&str> = None;
    if let (Some(p0), Some(pn)) = (por_hora.first(), por_hora.last()) {
        let span = (pn.0 - p0.0) as f64 * HORA_MS;
        if por_hora.len() >= 2 && span >= 3.0 * HORA_MS {
            ritmo = Some((pn.1 - p0.1) / (span / DIA_MS));
            fonte = Some("historico");
        }
    }
    if ritmo.is_none() {
        let est: f64 = itens
            .iter()
            .map(|(_, x)| {
                let f = |c: &str| {
                    if verdade(x.get(c)) {
                        js::mais(x.get(c))
                    } else {
                        0.0
                    }
                };
                f("dropsDia") * f("gemaMedia")
            })
            .sum();
        if est > 0.0 {
            ritmo = Some(est);
            fonte = Some("drops");
        }
    }
    let falta = js::round(meta - total).max(0.0);
    let dias_para_meta = match ritmo {
        Some(r) if r > 0.0 => js::json_num((falta / r).ceil()),
        _ if falta == 0.0 => json!(0),
        _ => Value::Null,
    };
    let (ano_mes_dias, dia) = dias_no_mes(p.agora);
    let (pokemons, shinies): (Vec<Value>, Vec<Value>) = pk.into_iter().partition(|x| !eh_shiny(x));
    let tem_itens = rr.is_some() && !itens_rr.is_empty();
    json!({
        "geradoEm": p.agora,
        "dadosEm": estado.get("em").filter(|v| verdade(Some(v))).cloned().unwrap_or(Value::Null),
        "temItens": tem_itens,
        "temPrecos": rr.is_some_and(|r| verdade(r.get("temPrecos"))),
        "notaMin": js::json_num(p.nota_min),
        "contas": contas,
        "conta": if nick.is_empty() { Value::Null } else { json!(nick) },
        "meta": js::json_num(meta), "metaConta": js::json_num(meta_conta),
        "metaRealista": rr.and_then(|r| r.get("metaRealista")).cloned().unwrap_or(Value::Null),
        "diasRestantes": ano_mes_dias - dia,
        "ritmoFonte": fonte,
        "cofre": { "itens": js::json_num(js::round(v_itens)), "pokemons": js::json_num(js::round(v_pk)),
                   "shiny": js::json_num(js::round(v_sh)), "total": js::json_num(js::round(total)) },
        "progresso": if meta != 0.0 { js::json_num(total / meta) } else { json!(0) },
        "falta": js::json_num(falta),
        "ritmoDia": ritmo.map_or(Value::Null, js::json_num),
        "diasParaMeta": dias_para_meta,
        "serie": serie, "porConta": por_conta,
        "itens": itens.into_iter().map(|i| i.1).collect::<Vec<_>>(),
        "pokemons": pokemons.into_iter().take(300).collect::<Vec<_>>(),
        "shinies": shinies.into_iter().take(100).collect::<Vec<_>>(),
        "cartasQualificadas": cartas, "quaseLa": quase,
        "altaPct": rr.and_then(|r| r.get("altaPct")).filter(|v| !v.is_null()).cloned().unwrap_or(json!(20)),
    })
}

/// Dias do mês e dia do mês de `agora`, em UTC (como o `diasRestantes` do legado).
fn dias_no_mes(agora: i64) -> (i64, i64) {
    let data = js::data_do_dia(agora.div_euclid(86_400_000));
    let mut partes = data.splitn(3, '-').map(|p| p.parse::<i64>().unwrap_or(1));
    let (ano, mes, dia) = (
        partes.next().unwrap_or(1970),
        partes.next().unwrap_or(1),
        partes.next().unwrap_or(1),
    );
    let bissexto = (ano % 4 == 0 && ano % 100 != 0) || ano % 400 == 0;
    let dias = match mes {
        2 if bissexto => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    (dias, dia)
}

#[cfg(test)]
mod testes {
    use super::*;

    fn lote(bolas: i64, shiny: i64) -> Value {
        json!({
            "raros": {"itens": [{"id": 1, "nome": "Token", "qtd": 3, "gemaMedia": 2,
                                 "contas": {"Ana": 2, "bia": 1}}],
                      "porConta": [{"nick": "Ana", "qtd": 2, "gemas": 4}, {"nick": "Bia", "qtd": 1, "gemas": 2}],
                      "meta": 100, "temPrecos": true},
            "historico": {"dia": {"Ana": {"2026-11-01": {"bolas": bolas, "shiny": shiny, "kills": 1}}},
                          "ritmo": {"Ana": {"kph": 100, "bolasKill": 2}}, "chanceShiny": 0.0001},
        })
    }

    #[test]
    fn ingerir_soma_deltas_e_trata_reset_do_contador() {
        let mut r = json!({});
        assert_eq!(ingerir(&mut r, &lote(100, 0), 7), Ok(1));
        ingerir(&mut r, &lote(130, 1), 8).unwrap();
        assert_eq!(r["acc"]["ana"]["dias"]["2026-11-01"]["bolas"], json!(130));
        assert_eq!(r["acc"]["ana"]["dias"]["2026-11-01"]["shiny"], json!(1));
        // Leitura menor que a anterior: o contador reiniciou, soma a leitura inteira.
        ingerir(&mut r, &lote(10, 1), 9).unwrap();
        assert_eq!(r["acc"]["ana"]["dias"]["2026-11-01"]["bolas"], json!(140));
        assert_eq!(r["acc"]["ana"]["dias"]["2026-11-01"]["shiny"], json!(1));
        assert_eq!((r["em"].clone(), r["histEm"].clone()), (json!(9), json!(9)));
        assert_eq!(
            ingerir(&mut r, &json!({"raros": {"itens": 1}}), 1),
            Err("raros inválido")
        );
    }

    #[test]
    fn ingerir_limita_a_400_itens() {
        let itens: Vec<Value> = (0..500).map(|i| json!({"id": i})).collect();
        let mut r = json!({});
        assert_eq!(
            ingerir(&mut r, &json!({"raros": {"itens": itens}}), 1),
            Ok(400)
        );
        assert!(r.get("hist").is_none());
    }

    #[test]
    fn registrar_shiny_valida_e_soma() {
        let mut r = json!({});
        assert_eq!(
            registrar_shiny(&mut r, &json!({}), "2026-11-02"),
            Err("nick obrigatório")
        );
        let v = registrar_shiny(
            &mut r,
            &json!({"nick": "Ana", "shiny": 2, "bolas": -5}),
            "2026-11-02",
        )
        .unwrap();
        assert_eq!(
            v,
            json!({"ok": true, "nick": "ana", "dia": "2026-11-02", "shiny": 2, "bolas": 0})
        );
        let v = registrar_shiny(
            &mut r,
            &json!({"nick": "ana", "shiny": 1, "bolas": 40}),
            "2026-11-02",
        )
        .unwrap();
        assert_eq!(
            (v["shiny"].clone(), v["bolas"].clone()),
            (json!(3), json!(40))
        );
        assert_eq!(r["acc"]["ana"]["nome"], json!("Ana"));
    }

    #[test]
    fn shiny_sem_dados_e_com_dados() {
        assert_eq!(shiny(&json!({}), "", None)["vazio"], json!(true));
        let mut r = json!({});
        ingerir(&mut r, &lote(1000, 0), 5).unwrap();
        let s = shiny(&r, "", None);
        assert_eq!(s["vazio"], json!(false));
        assert_eq!(s["contas"][0]["nick"], json!("Ana"));
        assert_eq!(s["contas"][0]["bolasDesde"], json!(1000));
        // bph = kph * bolasKill.
        assert_eq!(s["contas"][0]["bph"], json!(200));
        assert_eq!(
            s["modos"]["ver"]["curva"].as_array().map(Vec::len),
            Some(81)
        );
        assert_eq!(s["modos"]["ver"]["umEm"], json!(10000));
        assert_eq!(s["modos"]["capturar"]["umEmMax"], json!(4_517_516));
        assert_eq!(
            s["aviso"],
            json!("Amostra pequena: poucos arremessos registrados.")
        );
        assert_eq!(shiny(&r, "outra", None)["contas"], json!([]));
    }

    #[test]
    fn contas_do_cofre_une_relay_e_equipes() {
        let mut r = json!({});
        ingerir(&mut r, &lote(1, 0), 5).unwrap();
        let eq = vec![
            ("bia".to_owned(), json!({"pokemons": [{"id": 1}]})),
            ("vazia".to_owned(), json!({"pokemons": []})),
            ("conta da ia".to_owned(), json!({"pokemons": [{"id": 2}]})),
        ];
        assert_eq!(
            contas_do_cofre(&r, &eq),
            vec!["Ana".to_owned(), "Bia".to_owned()]
        );
    }

    #[test]
    fn raros2_soma_itens_e_cartas() {
        let mut r = json!({});
        ingerir(&mut r, &lote(1, 0), 5).unwrap();
        let contas = vec!["Ana".to_owned(), "Bia".to_owned()];
        let valiosos = |n: &str| Valiosos {
            lista: vec![
                json!({"nick": n, "id": 1, "shiny": false, "valorGemas": 10}),
                json!({"nick": n, "id": 2, "shiny": true, "valorGemas": 5}),
            ],
            total: 2,
            quase: 1,
        };
        let icone = |_: i64| Some("i.png".to_owned());
        let p = PedidoRaros2 {
            nick: "",
            nota_min: 3.5,
            meta: 0.0,
            agora: 1_800_000_000_000,
        };
        let v = raros2(&r, &contas, &valiosos, &[], &icone, &p);
        // Itens: 3 unidades * 2 gemas; cartas: 2 contas * (10 + 5).
        assert_eq!(
            v["cofre"],
            json!({"itens": 6, "pokemons": 20, "shiny": 10, "total": 36})
        );
        assert_eq!(v["meta"], json!(200));
        assert_eq!(v["falta"], json!(164));
        assert_eq!(v["itens"][0]["icone"], json!("i.png"));
        assert_eq!(v["cartasQualificadas"], json!(4));
        assert_eq!(v["quaseLa"], json!(2));
        assert_eq!(v["ritmoFonte"], Value::Null);
        assert_eq!(v["altaPct"], json!(20));
        let so_ana = PedidoRaros2 { nick: "ana", ..p };
        let v = raros2(&r, &contas, &valiosos, &[], &icone, &so_ana);
        assert_eq!(v["meta"], json!(100));
        assert_eq!(v["itens"][0]["qtd"], json!(2));
        assert_eq!(v["porConta"].as_array().map(Vec::len), Some(2));
    }

    #[test]
    fn dias_do_mes() {
        // 12/nov/2026, meio-dia em São Paulo.
        let agora = 1_794_495_600_000;
        assert_eq!(dias_no_mes(agora), (30, 12));
    }

    /// O merge do `ingerir` é idempotente: reaplicar o mesmo corpo (mesma leitura bruta) soma
    /// diferença zero. `registrar_shiny` soma de propósito e não entra aqui.
    mod propriedades {
        use proptest::prelude::*;

        use super::*;

        fn corpo() -> impl Strategy<Value = Value> {
            let dia = (0_u32..500, 0_u32..50).prop_map(|(b, s)| json!({"bolas": b, "shiny": s}));
            let dias = proptest::collection::btree_map("2026-01-0[1-3]", dia, 0..3);
            let contas = proptest::collection::btree_map("[A-C][a-c]{0,2}", dias, 0..3);
            let itens =
                proptest::collection::vec((0_u32..20).prop_map(|id| json!({"id": id})), 0..5);
            (itens, contas).prop_map(
                |(itens, contas)| json!({"raros": {"itens": itens}, "historico": {"dia": contas}}),
            )
        }

        proptest! {
            #[test]
            fn ingerir_repetido_e_idempotente(
                inicial in proptest::option::of(corpo()),
                c in corpo(),
                agora in 0_i64..4_000_000_000_000,
            ) {
                let mut base = json!({});
                if let Some(i) = inicial {
                    let _ = ingerir(&mut base, &i, agora);
                }
                let mut uma = base.clone();
                let _ = ingerir(&mut uma, &c, agora);
                let mut duas = uma.clone();
                let _ = ingerir(&mut duas, &c, agora);
                prop_assert_eq!(uma, duas);
            }
        }
    }
}
