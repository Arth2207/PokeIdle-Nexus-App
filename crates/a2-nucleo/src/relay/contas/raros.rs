//! Itens raros das contas com preço e projeção do mês (`planoRaros` do motor do legado).

use std::collections::{BTreeMap, HashMap};

use serde_json::{json, Value};

use super::conta::Conta;
use crate::relay::tempo::{dia_de, dias_restantes_no_mes};

/// Configuração padrão do radar no legado.
pub const ALTA_PCT: f64 = 20.0;
pub const META_GEMAS: f64 = 1000.0;

/// Médias de preço do mercado (`market` aba `medias`): item → (ouro, gema).
#[derive(Debug, Default, Clone)]
pub struct Medias(pub HashMap<i64, (Option<f64>, Option<f64>)>);

impl Medias {
    pub fn da_resposta(m: &Value) -> Self {
        let mut r = HashMap::new();
        for (k, v) in m
            .get("medias")
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
        {
            let media = |moeda: &str| {
                v.pointer(&format!("/{moeda}/media"))
                    .and_then(Value::as_f64)
                    .filter(|x| *x != 0.0)
            };
            if let Ok(id) = k.parse() {
                r.insert(id, (media("gold"), media("orb")));
            }
        }
        Self(r)
    }

    fn ouro(&self, id: i64) -> Option<f64> {
        self.0.get(&id).and_then(|x| x.0)
    }

    fn gema(&self, id: i64) -> Option<f64> {
        self.0.get(&id).and_then(|x| x.1)
    }
}

fn eh_raro(id: i64, nome: &str, ouro: Option<f64>) -> bool {
    let n = nome.to_lowercase();
    let comum = ["potion", "revive", "ball", "ether", "elixir"]
        .iter()
        .any(|p| n.contains(p))
        || n.ends_with("stone");
    !comum && (id >= 60_000 || ouro.unwrap_or(0.0) >= 1e6)
}

fn nome_de(id: i64, nomes: &HashMap<i64, String>) -> String {
    match nomes.get(&id) {
        Some(n) => n.clone(),
        None if id == 70_000 => "Bronze Boss Token (provável)".into(),
        None => format!("#{id}"),
    }
}

fn r1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

/// Dados de entrada do cálculo.
pub struct Entrada<'a> {
    pub contas: Vec<&'a Conta>,
    pub nomes: &'a HashMap<i64, String>,
    pub m7: &'a Medias,
    pub m1: &'a Medias,
    pub drops_dia: &'a HashMap<String, f64>,
    pub agora: i64,
}

struct Linha {
    id: i64,
    nome: String,
    qtd: i64,
    contas: BTreeMap<String, i64>,
}

/// O objeto `raros` que o servidor recebe em `/api/raros/ingest`.
pub fn plano(e: &Entrada) -> Value {
    let mut linhas: BTreeMap<i64, Linha> = BTreeMap::new();
    for c in &e.contas {
        for (&id, &q) in &c.itens {
            if q <= 0 {
                continue;
            }
            let nome = nome_de(id, e.nomes);
            if !eh_raro(id, &nome, e.m7.ouro(id)) {
                continue;
            }
            let l = linhas.entry(id).or_insert_with(|| Linha {
                id,
                nome,
                qtd: 0,
                contas: BTreeMap::new(),
            });
            l.qtd += q;
            l.contas.insert(c.nick.clone(), q);
        }
    }
    let restantes = f64::from(dias_restantes_no_mes(e.agora));
    let mut itens: Vec<Value> = linhas.values().map(|l| item(l, e, restantes)).collect();
    let gemas_de = |i: &Value| i["valorGemas"].as_f64().unwrap_or(0.0);
    itens.sort_by(|a, b| gemas_de(b).total_cmp(&gemas_de(a)));

    let gemas: f64 = itens.iter().map(gemas_de).sum();
    let mut por_conta: BTreeMap<String, (i64, i64, f64)> = e
        .contas
        .iter()
        .map(|c| (c.nick.clone(), (0, 0, 0.0)))
        .collect();
    for i in &itens {
        let gema = i["gemaMedia"].as_f64().unwrap_or(0.0);
        for (n, q) in i["contas"].as_object().into_iter().flatten() {
            let q = q.as_i64().unwrap_or(0);
            let p = por_conta.entry(n.clone()).or_default();
            p.0 += q;
            p.1 += 1;
            p.2 += gema * q as f64;
        }
    }
    let mut por_conta: Vec<Value> = por_conta
        .into_iter()
        .map(
            |(nick, (qtd, n, g))| json!({"nick": nick, "qtd": qtd, "itens": n, "gemas": g.round()}),
        )
        .collect();
    let g = |v: &Value| v["gemas"].as_f64().unwrap_or(0.0);
    por_conta.sort_by(|a, b| g(b).total_cmp(&g(a)));
    let projetadas: f64 = itens
        .iter()
        .map(|i| {
            i["gemasProjetadas"]
                .as_f64()
                .filter(|x| *x != 0.0)
                .unwrap_or_else(|| gemas_de(i))
        })
        .sum();
    json!({
        "itens": itens,
        "porConta": por_conta,
        "gemasSeVendesse": gemas.round(),
        "gemasProjetadas": projetadas.round(),
        "metaRealista": (projetadas * 0.8).round(),
        "diasRestantes": restantes,
        "meta": META_GEMAS,
        "falta": (META_GEMAS - gemas).round().max(0.0),
        "altaPct": ALTA_PCT,
        "ativo": true,
        "temPrecos": !e.m7.0.is_empty(),
    })
}

fn item(l: &Linha, e: &Entrada, restantes: f64) -> Value {
    let (g7, g1, o7, o1) = (
        e.m7.ouro(l.id),
        e.m1.ouro(l.id),
        e.m7.gema(l.id),
        e.m1.gema(l.id),
    );
    let var = |a: Option<f64>, b: Option<f64>| Some((b? / a? - 1.0) * 100.0);
    let v = [var(g7, g1), var(o7, o1)]
        .into_iter()
        .flatten()
        .reduce(f64::max);
    let sinal = match v {
        Some(v) if v >= ALTA_PCT => "vender",
        Some(v) if v <= -ALTA_PCT => "barato",
        _ => "normal",
    };
    let base = l
        .nome
        .trim_end_matches("(provável)")
        .trim_end_matches("(provavel)")
        .trim_end();
    let drops = e
        .drops_dia
        .get(&l.nome)
        .or_else(|| e.drops_dia.get(base))
        .copied()
        .filter(|d| *d != 0.0);
    let qtd = l.qtd as f64;
    let fim_mes = drops.map(|d| (qtd + d * restantes).round());
    let projetadas = o7.zip(fim_mes).map(|(o, q)| (o * q).round());
    json!({
        "id": l.id, "nome": l.nome, "qtd": l.qtd, "contas": l.contas,
        "ouroMedio": g7, "ouroHoje": g1, "gemaMedia": o7, "gemaHoje": o1,
        "valorOuro": g7.map(|x| x * qtd), "valorGemas": o7.map(|x| x * qtd),
        "varPct": v.map(r1), "sinal": sinal,
        "dropsDia": drops.map(r1), "qtdFimMes": fim_mes, "gemasProjetadas": projetadas,
    })
}

/// Histórico que acompanha os raros: dias (bolas, shiny, abates), ritmo e chance de shiny.
pub fn historico(
    diario: &super::diario::Diario,
    nicks: &[String],
    chance_shiny: Option<f64>,
    agora: i64,
) -> Value {
    let mut dia = serde_json::Map::new();
    for n in diario.nicks() {
        let Some(por) = diario.dias(n) else { continue };
        let m: serde_json::Map<String, Value> = por
            .iter()
            .rev()
            .take(45)
            .map(|(k, d)| {
                (
                    k.clone(),
                    json!({"bolas": d.bolas, "shiny": d.shiny, "kills": d.kills}),
                )
            })
            .collect();
        dia.insert(n.clone(), Value::Object(m));
    }
    let ritmo: serde_json::Map<String, Value> = nicks
        .iter()
        .map(|n| {
            let (kph, bk) = diario.ritmo(n, agora);
            (n.clone(), json!({"kph": kph, "bolasKill": bk}))
        })
        .collect();
    json!({ "dia": dia, "ritmo": ritmo, "chanceShiny": chance_shiny, "hoje": dia_de(agora) })
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn raros_com_precos_e_projecao() {
        let mut a = Conta {
            nick: "Sikien".into(),
            ..Conta::default()
        };
        a.itens.insert(70_000, 10);
        a.itens.insert(23, 190); // poção comum: fora
        a.itens.insert(70_070, 3);
        let mut b = Conta {
            nick: "Alllphha".into(),
            ..Conta::default()
        };
        b.itens.insert(70_000, 5);
        let mut nomes = HashMap::new();
        nomes.insert(70_070, "Super Potion".to_owned());
        nomes.insert(23, "Potion".to_owned());
        let m7 = Medias::da_resposta(
            &json!({"medias": {"70000": {"gold": {"media": 100}, "orb": {"media": 10}}}}),
        );
        let m1 = Medias::da_resposta(
            &json!({"medias": {"70000": {"gold": {"media": 130}, "orb": {"media": 9}}}}),
        );
        let mut drops = HashMap::new();
        drops.insert("Bronze Boss Token".to_owned(), 2.0);
        // 14/11/2026: faltam 16 dias.
        let p = plano(&Entrada {
            contas: vec![&a, &b],
            nomes: &nomes,
            m7: &m7,
            m1: &m1,
            drops_dia: &drops,
            agora: 1_794_632_400_000,
        });
        let i = &p["itens"][0];
        assert_eq!(p["itens"].as_array().map(Vec::len), Some(1));
        assert_eq!(i["nome"], "Bronze Boss Token (provável)");
        assert_eq!(i["qtd"], 15);
        assert_eq!(i["contas"]["Alllphha"], 5);
        assert_eq!(i["varPct"], 30.0);
        assert_eq!(i["sinal"], "vender");
        assert_eq!(i["qtdFimMes"], 47.0);
        assert_eq!(i["gemasProjetadas"], 470.0);
        assert_eq!(p["gemasSeVendesse"], 150.0);
        assert_eq!(p["falta"], 850.0);
        assert_eq!(p["porConta"][0]["nick"], "Sikien");
        assert_eq!(p["temPrecos"], true);
    }
}
