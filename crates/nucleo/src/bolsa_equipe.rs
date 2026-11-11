//! Equipe de boss a partir das ofertas da bolsa (`equipeBolsa`), com os pokémon da conta
//! (`equipePropria`), alvos de coleta (`bolsa-alvos.js`), faltas, lucro (`bolsa-lucro.js`) e a
//! tabela de bosses da wiki (`bolsa-wiki-bosses.js`). Cálculo puro: ofertas, cotações e
//! preços chegam por parâmetro.

use std::collections::{HashMap, HashSet};

use serde_json::{json, Map, Value};

use crate::combate::combatente::{self, Opcoes as OpcoesMon, Perfil as PerfilTeorico};
use crate::combate::duelo::{Avaliacao, TEMPO_MAXIMO};
use crate::combate::especie::EspecieCombate;
use crate::combate::tipos::Tipo;
use crate::equipes::{norm, proximo_boss, BossLista, EstadoEquipes, MotorEquipes};
use crate::js;
use crate::otimizador::{self, Candidato, Opcoes, Perfil, ResultadoPerfil, Selecao};

// ---------- wiki ----------

/// Um boss da tabela da wiki do jogo.
#[derive(Debug, Clone, PartialEq)]
pub struct BossWiki {
    pub nome: String,
    pub nivel: f64,
    pub drops: Value,
    pub entrada: Value,
}

fn num_wiki(s: &str) -> f64 {
    js::mais(Some(&Value::from(s.replace('.', "").replacen(',', ".", 1))))
}

fn item_wiki(nome: &str) -> (Value, String) {
    let n = nome.to_lowercase();
    let (id, nome) = if n.contains("z-move") {
        (Value::from(59194), "TM Disk Piece".to_string())
    } else if n.contains("aoe") {
        (Value::from(40530), "AoE TM Disk Piece".to_string())
    } else if n.contains("mega shiny") {
        (Value::from(70015), "Mega Shiny Stone Fragment".to_string())
    } else if n.contains("mega stone") {
        (Value::from(70014), "Mega Stone Fragment".to_string())
    } else {
        (Value::Null, nome.trim().to_string())
    };
    (id, nome)
}

/// `([^·()]+?)\s*\(([\d.,]+)%\)`, todas as ocorrências.
fn drops_da_celula(texto: &str) -> Vec<(String, String)> {
    let cs: Vec<char> = texto.chars().collect();
    let especial = |c: char| matches!(c, '·' | '(' | ')');
    let mut out = Vec::new();
    let mut p = 0;
    let mut k = 0;
    while k < cs.len() {
        if cs[k] != '(' {
            k += 1;
            continue;
        }
        let mut f = k + 1;
        while f < cs.len() && (cs[f].is_ascii_digit() || cs[f] == '.' || cs[f] == ',') {
            f += 1;
        }
        let casa = f > k + 1 && cs.get(f) == Some(&'%') && cs.get(f + 1) == Some(&')');
        if casa {
            let inicio = (p..k).rev().find(|&i| especial(cs[i])).map_or(p, |i| i + 1);
            if inicio < k {
                let nome: String = cs[inicio..k].iter().collect();
                let pct: String = cs[k + 1..f].iter().collect();
                out.push((nome, pct));
                p = f + 2;
                k = p;
                continue;
            }
        }
        k += 1;
    }
    out
}

/// `parse` do `bolsa-wiki-bosses.js`: a tabela da seção "## Bosses".
pub fn parse_wiki(txt: &str) -> Vec<BossWiki> {
    let Some(i) = txt.find("## Bosses") else {
        return Vec::new();
    };
    let resto = txt.get(i + 5..).unwrap_or("");
    let sec = match resto.find("\n## ") {
        Some(j) => txt.get(i..i + 5 + j).unwrap_or(""),
        None => txt.get(i..).unwrap_or(""),
    };
    let mut out = Vec::new();
    for l in sec.split('\n') {
        if !l.chars().any(|c| c.is_ascii_digit()) || !l.contains('%') || !l.contains('|') {
            continue;
        }
        let c: Vec<&str> = l
            .split('|')
            .map(str::trim)
            .filter(|x| !x.is_empty())
            .collect();
        let [.., nome, nivel, dr] = c.as_slice() else {
            continue;
        };
        if nivel.is_empty() || !nivel.chars().all(|c| c.is_ascii_digit() || c == '.') {
            continue;
        }
        if nome.is_empty() || nome.chars().all(|c| c == '-') {
            continue;
        }
        let drops: Vec<Value> = drops_da_celula(dr)
            .into_iter()
            .map(|(n, pct)| {
                let (id, nome_item) = item_wiki(&n);
                json!({ "itemId": id, "nome": nome_item, "nomeWiki": n.trim(), "chance": num_wiki(&pct) / 100.0 })
            })
            .collect();
        if !drops.is_empty() {
            out.push(BossWiki {
                nome: (*nome).to_string(),
                nivel: num_wiki(nivel),
                drops: Value::Array(drops),
                entrada: json!({ "itemId": 70000, "nome": "Bronze Boss Token", "qtd": 1 }),
            });
        }
    }
    out
}

// ---------- boss da bolsa ----------

/// Boss escolhido para a bolsa, com a conta e os avisos.
pub struct BossBolsa<'a> {
    pub conta: Option<&'a Value>,
    pub boss: BossLista,
    pub avisos: Vec<String>,
}

/// `resolverBossBolsa`: a lista do app com nível, drops e entrada da wiki (que é a fonte
/// principal); bosses só da wiki entram no fim.
pub fn resolver_boss<'a>(
    motor: &MotorEquipes,
    estado: &'a EstadoEquipes,
    wiki: &[BossWiki],
    boss_key: Option<&str>,
    nick: Option<&str>,
) -> Result<BossBolsa<'a>, &'static str> {
    let conta = estado.conta(nick);
    let wk: HashMap<String, &BossWiki> = wiki.iter().map(|w| (norm(&w.nome), w)).collect();
    let mut lista: Vec<BossLista> = motor
        .lista_bosses(&estado.bosses)
        .into_iter()
        .map(|b| match wk.get(&b.key) {
            Some(w) => BossLista {
                team_level: w.nivel,
                min_tr: w.nivel,
                drops: w.drops.clone(),
                entrada: w.entrada.clone(),
                fonte: "wiki",
                ..b
            },
            None => b,
        })
        .collect();
    for w in wiki {
        let k = norm(&w.nome);
        if !lista.iter().any(|b| b.key == k) {
            lista.push(BossLista {
                key: k,
                nome: w.nome.clone(),
                team_level: w.nivel,
                min_tr: w.nivel,
                regiao: None,
                trinca: None,
                pos: None,
                tipos: motor
                    .dex
                    .por_nome(&w.nome)
                    .map(|sp| sp.tipos.iter().map(ToString::to_string).collect())
                    .unwrap_or_default(),
                entrada: w.entrada.clone(),
                drops: w.drops.clone(),
                fonte: "wiki",
            });
        }
    }
    lista.sort_by(|a, b| a.team_level.total_cmp(&b.team_level));
    let pedido = norm(boss_key.unwrap_or(""));
    let venc: HashSet<String> = conta
        .and_then(|c| c.get("vencidos"))
        .and_then(Value::as_array)
        .map(|v| v.iter().map(|x| norm(&js::texto(Some(x)))).collect())
        .unwrap_or_default();
    let tr = conta.map_or(f64::NAN, |c| js::mais(c.get("trLv")));
    let boss = lista
        .iter()
        .find(|b| b.key == pedido)
        .or_else(|| lista.iter().find(|b| norm(&b.nome) == pedido))
        .or_else(|| {
            let p = proximo_boss(&lista, tr, &venc)?;
            lista.iter().find(|b| b.key == p)
        })
        .cloned()
        .ok_or("boss não encontrado")?;
    let mut avisos = Vec::new();
    if !wiki.is_empty() && !wk.contains_key(&boss.key) {
        avisos.push(format!(
            "{} não consta na wiki do jogo: usei os dados do app (nível e drops).",
            boss.nome
        ));
    }
    let minimo = if boss.min_tr != 0.0 && !boss.min_tr.is_nan() {
        boss.min_tr
    } else {
        boss.team_level
    };
    if tr > 0.0 && tr < minimo {
        let tr_txt = js::texto(conta.and_then(|c| c.get("trLv")));
        avisos.push(format!(
            "O treinador (nível {tr_txt}) ainda não alcança o nível do boss ({}).",
            js::numero(minimo)
        ));
    }
    Ok(BossBolsa {
        conta,
        boss,
        avisos,
    })
}

// ---------- alvos de coleta ----------

pub const EXTRA_MAX_POR_PERFIL: usize = 20;
pub const EXTRA_PASSO: usize = 5;
const ALVOS_POR_PERFIL: usize = 5;
const MARGEM_MIN: f64 = 1.05;
const NIVEL_ECO: f64 = 1.5;
pub const PERFIS_LISTA: [&str; 3] = ["maxima", "equilibrada", "economica"];

/// Uma espécie de uma lista de perfil.
#[derive(Debug, Clone, PartialEq)]
pub struct AlvoEspecie {
    pub species_id: i64,
    pub nome: String,
    pub d: f64,
    pub preco: Option<f64>,
    pub extra: bool,
}

/// Espécies compráveis avaliadas contra o boss: (espécie, D no nível do boss, D a 1,5×) e a
/// referência (soma da equipe teórica realista).
#[derive(Debug, Clone)]
pub struct AvaliacaoEspecies {
    pub ava: Vec<(i64, String, f64, f64)>,
    pub referencia: f64,
}

/// `avaliarEspecies`.
pub fn avaliar_especies(motor: &MotorEquipes, boss: &BossLista) -> Option<AvaliacaoEspecies> {
    let k = motor.modelo.constantes();
    let (foe, _) = motor.modelo.boss(&boss.para_combate(), &motor.dex)?;
    let op = OpcoesMon {
        qualidade: 1.3,
        ivs: [31.0; 6],
        potencia: 3,
        ..OpcoesMon::default()
    };
    let mut ava: Vec<(i64, String, f64, f64)> = motor
        .dex
        .especies()
        .iter()
        .filter(|sp| {
            matches!(sp.categoria.as_deref(), None | Some("pokebola" | "mega")) && !sp.lendaria()
        })
        .map(|sp| {
            let me = combatente::Combatente::novo(sp, boss.team_level, &op, k);
            let m2 =
                combatente::Combatente::novo(sp, js::round(NIVEL_ECO * boss.team_level), &op, k);
            (
                sp.id,
                sp.nome.clone(),
                motor.modelo.avaliar(&me, &foe, 1.0).d,
                motor.modelo.avaliar(&m2, &foe, 1.0).d,
            )
        })
        .filter(|x| x.2 > 0.0)
        .collect();
    ava.sort_by(|a, b| b.2.total_cmp(&a.2));
    Some(AvaliacaoEspecies {
        ava,
        referencia: referencia(motor, boss, foe.hp),
    })
}

/// Referência de dano: a soma da equipe teórica realista (no mínimo 1).
pub fn referencia(motor: &MotorEquipes, boss: &BossLista, hp_boss: f64) -> f64 {
    let soma = motor
        .equipe_ideal(boss, PerfilTeorico::Realista, 5, None)
        .and_then(|e| e.get("soma").and_then(Value::as_f64))
        .unwrap_or(0.0);
    let r = if soma == 0.0 || soma.is_nan() {
        hp_boss
    } else {
        soma
    };
    r.max(1.0)
}

/// `listasDoBoss`: as 3 listas do boss, sem repetir espécie entre elas.
pub fn listas_do_boss(
    a: &AvaliacaoEspecies,
    precos: &HashMap<i64, f64>,
    extras: [usize; 3],
) -> [Vec<AlvoEspecie>; 3] {
    let preco = |id: i64| precos.get(&id).copied();
    let tem = |id: i64| preco(id).is_some_and(|p| p > 0.0);
    let minimo = MARGEM_MIN * a.referencia / 5.0;
    let maxima: Vec<&(i64, String, f64, f64)> = a.ava.iter().collect();
    let mut equilibrada: Vec<&(i64, String, f64, f64)> =
        a.ava.iter().filter(|x| tem(x.0)).collect();
    equilibrada.sort_by(|x, y| {
        let (px, py) = (
            preco(x.0).unwrap_or(f64::NAN),
            preco(y.0).unwrap_or(f64::NAN),
        );
        otimizador_ordem(y.2 / py - x.2 / px)
    });
    let mut economica: Vec<&(i64, String, f64, f64)> =
        a.ava.iter().filter(|x| tem(x.0) && x.3 >= minimo).collect();
    economica.sort_by(|x, y| {
        let (px, py) = (
            preco(x.0).unwrap_or(f64::NAN),
            preco(y.0).unwrap_or(f64::NAN),
        );
        let d = px - py;
        otimizador_ordem(if d == 0.0 || d.is_nan() { y.2 - x.2 } else { d })
    });
    let rank = [maxima, equilibrada, economica];
    let mut ptr = [0usize; 3];
    let mut usado = HashSet::new();
    let mut pegar = |k: usize, n: usize, extra: bool| {
        let mut out = Vec::new();
        while ptr[k] < rank[k].len() && out.len() < n {
            let x = rank[k][ptr[k]];
            ptr[k] += 1;
            if !usado.insert(x.0) {
                continue;
            }
            out.push(AlvoEspecie {
                species_id: x.0,
                nome: x.1.clone(),
                d: js::round(x.2),
                preco: preco(x.0),
                extra,
            });
        }
        out
    };
    let mut base = [
        pegar(0, ALVOS_POR_PERFIL, false),
        pegar(1, ALVOS_POR_PERFIL, false),
        pegar(2, ALVOS_POR_PERFIL, false),
    ];
    for (k, b) in base.iter_mut().enumerate() {
        let n = extras
            .get(k)
            .copied()
            .unwrap_or(0)
            .min(EXTRA_MAX_POR_PERFIL);
        b.extend(pegar(k, n, true));
    }
    base
}

fn otimizador_ordem(d: f64) -> std::cmp::Ordering {
    if d < 0.0 {
        std::cmp::Ordering::Less
    } else if d > 0.0 {
        std::cmp::Ordering::Greater
    } else {
        std::cmp::Ordering::Equal
    }
}

// ---------- faltas ----------

const FALTA_VALE_MS: i64 = 6 * 3_600_000;
const FALTA_PASSO_MS: i64 = 10 * 60_000;

/// Faltas de coleta: `boss|perfil` → registro (o `.bolsa-faltas.json` do legado).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Faltas(pub Map<String, Value>);

impl Faltas {
    fn num(f: &Value, c: &str) -> f64 {
        js::mais(f.get(c))
    }

    /// Quantas espécies extras o perfil pede (`extrasDe`).
    pub fn extras(&self, boss: &str, perfil: &str, agora: i64) -> usize {
        match self.0.get(&format!("{boss}|{perfil}")) {
            Some(f) if Self::num(f, "expira") > agora as f64 => {
                let n =
                    (EXTRA_PASSO as f64 * Self::num(f, "nivel")).min(EXTRA_MAX_POR_PERFIL as f64);
                if n.is_finite() && n > 0.0 {
                    n as usize
                } else {
                    0
                }
            }
            _ => 0,
        }
    }

    /// `registrarFalta`.
    pub fn registrar(&mut self, boss: &BossLista, perfil: &str, faltam: usize, agora: i64) {
        let k = format!("{}|{perfil}", boss.key);
        let f = self.0.entry(k).or_insert_with(|| {
            json!({ "boss": boss.key, "nome": boss.nome, "perfil": perfil, "nivel": 1, "proxEscala": agora + FALTA_PASSO_MS, "novo": true })
        });
        let Value::Object(o) = f else { return };
        let novo = o.remove("novo").is_some();
        if !novo {
            let ativa = o.get("ativa").is_some_and(|v| js::verdadeiro(Some(v)));
            if ativa && agora as f64 >= js::mais(o.get("proxEscala")) {
                let teto = (EXTRA_MAX_POR_PERFIL as f64 / EXTRA_PASSO as f64).ceil();
                let nivel = teto.min(js::mais(o.get("nivel")) + 1.0);
                o.insert("nivel".into(), js::json_num(nivel));
                o.insert("proxEscala".into(), Value::from(agora + FALTA_PASSO_MS));
            } else if !ativa {
                o.insert("proxEscala".into(), Value::from(agora + FALTA_PASSO_MS));
            }
        }
        o.insert("ativa".into(), Value::Bool(true));
        o.insert("faltam".into(), Value::from(faltam));
        o.insert("em".into(), Value::from(agora));
        o.insert("expira".into(), Value::from(agora + FALTA_VALE_MS));
    }

    /// `resolverFalta`.
    pub fn resolver(&mut self, boss: &BossLista, perfil: &str, agora: i64) {
        if let Some(Value::Object(o)) = self.0.get_mut(&format!("{}|{perfil}", boss.key)) {
            if o.get("ativa").is_some_and(|v| js::verdadeiro(Some(v))) {
                o.insert("ativa".into(), Value::Bool(false));
                o.insert("resolvidoEm".into(), Value::from(agora));
                o.insert("expira".into(), Value::from(agora + FALTA_VALE_MS));
            }
        }
    }

    /// Remove as vencidas (`limparFaltas`); devolve se mudou algo.
    pub fn limpar(&mut self, agora: i64) -> bool {
        let antes = self.0.len();
        self.0.retain(|_, f| {
            Self::num(f, "expira").partial_cmp(&(agora as f64)) != Some(std::cmp::Ordering::Less)
        });
        antes != self.0.len()
    }

    /// `faltasParaRota`: (lista com extras, alguma ativa, assinatura).
    pub fn para_rota(&self, agora: i64) -> (Vec<(String, String, usize)>, bool, String) {
        let vivas: Vec<&Value> = self
            .0
            .values()
            .filter(|f| Self::num(f, "expira") > agora as f64)
            .collect();
        let lista = vivas
            .iter()
            .map(|f| {
                let n =
                    (EXTRA_PASSO as f64 * Self::num(f, "nivel")).min(EXTRA_MAX_POR_PERFIL as f64);
                (
                    js::texto(f.get("boss")),
                    js::texto(f.get("perfil")),
                    if n > 0.0 { n as usize } else { 0 },
                )
            })
            .collect();
        let ativas = vivas.iter().any(|f| js::verdadeiro(f.get("ativa")));
        let mut sig: Vec<String> = vivas
            .iter()
            .map(|f| {
                format!(
                    "{}|{}|{}|{}",
                    js::texto(f.get("boss")),
                    js::texto(f.get("perfil")),
                    js::texto(f.get("nivel")),
                    if js::verdadeiro(f.get("ativa")) { 1 } else { 0 }
                )
            })
            .collect();
        sig.sort();
        (lista, ativas, sig.join(","))
    }
}

// ---------- lucro ----------

const P_K: f64 = 12.0;
const P_CENTRO: f64 = 0.91;
const TEMPO_FIXO: f64 = 10.0;
const HORAS_DIA: f64 = 24.0;
const LIMITE_HORAS_RETORNO: f64 = 24.0 * 30.0;
const DIAS_REVENDA: f64 = 7.0;

/// Preço de um item: (preço, fonte).
pub type PrecoDe<'a> = &'a dyn Fn(&Value, &Value) -> (Option<f64>, Option<&'static str>);

fn kills_para(q: f64, chance: f64) -> Value {
    if !positivo(chance) {
        Value::Null
    } else if chance >= 1.0 {
        Value::from(1)
    } else {
        js::json_num((js::ln(1.0 - q) / js::ln(1.0 - chance)).ceil())
    }
}

/// `lucroPerfil`: lucro esperado por boss (estimativa).
pub fn lucro_perfil(
    perfil: &Value,
    boss_hp: f64,
    drops: &Value,
    entrada: &Value,
    preco_de: PrecoDe<'_>,
) -> Value {
    let slots = perfil.get("slots").and_then(Value::as_array);
    if perfil
        .get("indisponivel")
        .is_some_and(|v| js::verdadeiro(Some(v)))
        || slots.is_none_or(Vec::is_empty)
    {
        return Value::Null;
    }
    let slots = slots.map_or(&[][..], Vec::as_slice);
    let drops: Vec<Value> = drops
        .as_array()
        .map_or(&[][..], Vec::as_slice)
        .iter()
        .map(|d| {
            let (p, fonte) = preco_de(d.get("itemId").unwrap_or(&Value::Null), d.get("nome").unwrap_or(&Value::Null));
            let preco = p.filter(|p| *p > 0.0).unwrap_or(0.0);
            let chance = ou(js::mais(d.get("chance")), 0.0);
            let qtd = ou(js::mais(d.get("qtd")), 1.0);
            json!({
                "itemId": d.get("itemId"), "nome": d.get("nome"), "chance": js::json_num(chance), "qtd": js::json_num(qtd),
                "preco": js::json_num(preco), "semPreco": !positivo(preco), "fonte": fonte,
                "kills50": kills_para(0.5, chance), "kills90": kills_para(0.9, chance), "kills99": kills_para(0.99, chance),
            })
        })
        .collect();
    let f = |v: &Value, c: &str| v.get(c).and_then(Value::as_f64).unwrap_or(0.0);
    let ev_kill = drops
        .iter()
        .fold(0.0, |s, d| s + f(d, "chance") * f(d, "qtd") * f(d, "preco"));
    let tem_entrada = js::verdadeiro(Some(entrada));
    let custo_entrada = if tem_entrada {
        let (p, _) = preco_de(
            entrada.get("itemId").unwrap_or(&Value::Null),
            entrada.get("nome").unwrap_or(&Value::Null),
        );
        p.filter(|p| *p > 0.0).unwrap_or(0.0) * ou(js::mais(entrada.get("qtd")), 1.0)
    } else {
        0.0
    };
    let ms = match perfil.get("margemEstresse") {
        None | Some(Value::Null) => js::mais(perfil.get("poder")) * 0.8,
        Some(v) => js::mais(Some(v)),
    };
    let p_vitoria = if ms > 0.0 {
        1.0 / (1.0 + js::exp(-(P_K * (js::ln(ms) - js::ln(P_CENTRO)))))
    } else {
        0.0
    };
    let dps = slots
        .iter()
        .fold(0.0, |s, x| s + ou(js::mais(x.get("dpsOut")), 0.0));
    let t_kill = if dps > 0.0 && boss_hp > 0.0 {
        TEMPO_MAXIMO.min((boss_hp / dps).max(1.0))
    } else {
        TEMPO_MAXIMO
    };
    let kills_hora = 3600.0 / (t_kill + TEMPO_FIXO);
    let kills_dia = kills_hora * HORAS_DIA;
    let ev = p_vitoria * ev_kill - custo_entrada;
    let lucro_dia = ev * kills_dia;
    let custo = ou(js::mais(perfil.get("total")), 0.0);
    let kills_pagar = (ev > 0.0).then(|| custo / ev);
    let horas_pagar = kills_pagar.map(|k| k / kills_hora);
    let revenda = slots.iter().fold(0.0, |s, x| {
        let j = js::mais(x.get("justo"));
        s + if j > 0.0 {
            j
        } else {
            ou(js::mais(x.get("preco")), 0.0)
        }
    });
    let compensa =
        ev > 0.0 && lucro_dia > 0.0 && horas_pagar.is_some_and(|h| h <= LIMITE_HORAS_RETORNO);
    let mut avisos = Vec::new();
    let sem: Vec<String> = drops
        .iter()
        .filter(|d| d.get("semPreco") == Some(&Value::Bool(true)))
        .map(|d| js::texto(d.get("nome")))
        .collect();
    if !sem.is_empty() {
        avisos.push(format!(
            "Sem preço de mercado (contam 0): {}.",
            sem.join(", ")
        ));
    }
    if !tem_entrada {
        avisos.push(
            "Sem dados de entrada para este boss: custo de entrada contado como 0.".to_string(),
        );
    }
    if tem_entrada && !positivo(custo_entrada) {
        avisos.push(format!(
            "Sem preço da entrada ({}): custo de entrada contado como 0.",
            js::texto(entrada.get("nome"))
        ));
    }
    json!({
        "ev": js::json_num(ev), "evKill": js::json_num(ev_kill), "custoEntrada": js::json_num(custo_entrada),
        "pVitoria": js::json_num(p_vitoria), "tKill": js::json_num(t_kill), "killsHora": js::json_num(kills_hora),
        "killsDia": js::json_num(kills_dia), "killsParaPagar": kills_pagar.map(js::json_num),
        "horasParaPagar": horas_pagar.map(js::json_num), "lucroDia": js::json_num(lucro_dia), "revenda": js::json_num(revenda),
        "lucroSeRevender": js::json_num(lucro_dia * DIAS_REVENDA - (custo - revenda)), "diasRevenda": js::json_num(DIAS_REVENDA),
        "compensa": compensa,
        "motivoNaoCompensa": if compensa { Value::Null } else if ev <= 0.0 { "o valor esperado por luta não cobre a entrada".into() } else { "prazo de retorno acima do limite".into() },
        "limiteHoras": js::json_num(LIMITE_HORAS_RETORNO), "drops": drops, "avisos": avisos, "estimativa": true,
    })
}

fn ou(a: f64, b: f64) -> f64 {
    if a == 0.0 || a.is_nan() {
        b
    } else {
        a
    }
}

// ---------- equipe da bolsa ----------

/// Uma oferta de pokémon como chega do banco.
#[derive(Debug, Clone, PartialEq)]
pub struct OfertaEquipe {
    pub id: i64,
    pub preco: f64,
    pub quantidade: f64,
    pub ficha: Value,
}

/// Uma carta avaliada contra o boss.
struct Carta<'a> {
    id: String,
    ficha: &'a Value,
    sp: &'a EspecieCombate,
    lvl: f64,
    unit: f64,
    hp: f64,
    d: f64,
    r: Avaliacao,
    justo: f64,
    cotacao: Option<&'a Value>,
}

fn resumo_tipos(motor: &MotorEquipes, tipos: &[Tipo]) -> Value {
    let k = motor.modelo.constantes();
    Value::Array(
        Tipo::TODOS
            .iter()
            .map(|t| (t, k.efetividade_hunt(*t, tipos)))
            .filter(|(_, m)| *m != 1.0)
            .map(|(t, m)| json!({ "t": t.to_string(), "mult": js::json_num(m) }))
            .collect(),
    )
}

fn eficacia(e: crate::combate::duelo::Eficacia) -> Value {
    json!({ "mult": js::json_num(e.mult), "tipo": e.tipo.map(|t| t.to_string()) })
}

fn ou_nulo(f: &Value, c: &str) -> Value {
    js::nulo_ou(f.get(c), None).cloned().unwrap_or(Value::Null)
}

/// Perfil composto (o `resolverPorPerfil`): resultado com índices nas cartas originais.
#[derive(Debug, Clone)]
pub struct PerfilComposto {
    pub r: ResultadoPerfil,
    pub lista: Option<Vec<String>>,
    pub completado: bool,
    pub falta_coleta: bool,
}

/// Saída do `resolverPorPerfil`.
pub struct Resolvido {
    pub perfis: [PerfilComposto; 3],
    pub avisos: Vec<String>,
    pub elegiveis: usize,
    pub excluidas_preco: usize,
    pub sem_avaliacao: usize,
}

fn remapear(r: &ResultadoPerfil, mapa: &[usize]) -> ResultadoPerfil {
    let m = |i: usize| mapa.get(i).copied().unwrap_or(i);
    let mut r = r.clone();
    if let Some(e) = r.equipe.as_mut() {
        e.pick = e.pick.iter().map(|&i| m(i)).collect();
    }
    for a in &mut r.alternativas {
        if let Some(e) = a.equipe.as_mut() {
            e.pick = e.pick.iter().map(|&i| m(i)).collect();
        }
    }
    for l in &mut r.reservas {
        for x in l.iter_mut() {
            x.x = m(x.x);
        }
    }
    r
}

fn ids_de(c: &[Candidato], r: &ResultadoPerfil) -> String {
    let mut ids: Vec<&str> = r
        .equipe
        .as_ref()
        .map(|e| e.pick.iter().map(|&i| c[i].id.as_str()).collect())
        .unwrap_or_default();
    ids.sort_unstable();
    ids.join(",")
}

fn perfil_de(s: &Selecao, p: Perfil) -> &ResultadoPerfil {
    match p {
        Perfil::Eco => &s.eco,
        Perfil::Eq => &s.eq,
        Perfil::Max => &s.max,
    }
}

/// `resolverPorPerfil`: cada perfil escolhe primeiro entre as espécies da lista dele; se não
/// fechar equipe viável, completa com as demais. Registra ou resolve as faltas.
pub fn resolver_por_perfil(
    c: &[Candidato],
    op: &Opcoes,
    boss: &BossLista,
    listas: Option<&[Vec<AlvoEspecie>; 3]>,
    faltas: &mut Faltas,
    agora: i64,
) -> Resolvido {
    let ordem = [(Perfil::Max, 0usize), (Perfil::Eq, 1), (Perfil::Eco, 2)];
    let Some(listas) = listas.filter(|l| !l[0].is_empty()) else {
        let s = otimizador::selecionar(c, op);
        let pc = |r: &ResultadoPerfil| PerfilComposto {
            r: r.clone(),
            lista: None,
            completado: false,
            falta_coleta: false,
        };
        return Resolvido {
            perfis: [pc(&s.eco), pc(&s.eq), pc(&s.max)],
            avisos: s.avisos,
            elegiveis: s.elegiveis,
            excluidas_preco: s.excluidas_preco,
            sem_avaliacao: s.sem_avaliacao,
        };
    };
    let mut cheio: Option<Selecao> = None;
    let mut runs: Vec<(usize, usize, usize)> = Vec::new();
    let mut avisos: Vec<String> = Vec::new();
    let mut saida: HashMap<usize, PerfilComposto> = HashMap::new();
    for (p, li) in ordem {
        let lista = &listas[li];
        let ids: HashSet<i64> = lista.iter().map(|e| e.species_id).collect();
        let mapa: Vec<usize> = (0..c.len()).filter(|&i| ids.contains(&c[i].sp)).collect();
        let sub: Vec<Candidato> = mapa.iter().map(|&i| c[i].clone()).collect();
        let nm = p.nome();
        let outros: Vec<&str> = [Perfil::Eco, Perfil::Eq, Perfil::Max]
            .iter()
            .map(|x| x.nome())
            .filter(|x| *x != nm)
            .collect();
        let mut r: Option<Selecao> = None;
        let mut ok = false;
        if sub.len() >= op.n {
            let o = Opcoes {
                so_max: op.so_max || p == Perfil::Max,
                so_eco: op.so_eco || p == Perfil::Eco,
                ..op.clone()
            };
            let s = otimizador::selecionar(&sub, &o);
            let pk = perfil_de(&s, p);
            let prefixo = format!("{nm}: nenhuma equipe cumpriu");
            ok = !pk.indisponivel
                && !pk.inviavel
                && !s.avisos.iter().any(|a| a.starts_with(&prefixo));
            r = Some(s);
        }
        let (fonte_res, fonte_avisos, fonte_cont) = match r.as_ref().filter(|_| ok) {
            Some(s) => (
                remapear(perfil_de(s, p), &mapa),
                s.avisos.clone(),
                (s.elegiveis, s.excluidas_preco, s.sem_avaliacao),
            ),
            None => {
                let s = cheio.get_or_insert_with(|| otimizador::selecionar(c, op));
                (
                    perfil_de(s, p).clone(),
                    s.avisos.clone(),
                    (s.elegiveis, s.excluidas_preco, s.sem_avaliacao),
                )
            }
        };
        runs.push(fonte_cont);
        let mut pf = PerfilComposto {
            r: fonte_res,
            lista: Some(lista.iter().map(|e| e.nome.clone()).collect()),
            completado: !ok,
            falta_coleta: false,
        };
        if p != Perfil::Max {
            let imp = |a: &Vec<String>| a.iter().any(|x| x.contains("Boss impossível"));
            let impossivel = imp(&fonte_avisos) || cheio.as_ref().is_some_and(|s| imp(&s.avisos));
            let nome_lista = PERFIS_LISTA[li];
            if ok {
                faltas.resolver(boss, nome_lista, agora);
            } else if !impossivel {
                faltas.registrar(
                    boss,
                    nome_lista,
                    op.n.saturating_sub(sub.len()).max(1),
                    agora,
                );
                pf.falta_coleta = true;
            }
        }
        for a in &fonte_avisos {
            if (a.starts_with(nm) || !outros.iter().any(|o| a.starts_with(o)))
                && !avisos.contains(a)
            {
                avisos.push(a.clone());
            }
        }
        if !ok {
            avisos.push(format!(
                "{nm}: completado com outras espécies (as {} da lista do perfil não formaram equipe viável).",
                lista.len()
            ));
        }
        saida.insert(li, pf);
    }
    let mut take = |i: usize| {
        saida.remove(&i).unwrap_or_else(|| PerfilComposto {
            r: vazio(),
            lista: None,
            completado: false,
            falta_coleta: false,
        })
    };
    let (maxima, eq, mut eco) = (take(0), take(1), take(2));
    let tot = |p: &PerfilComposto| p.r.equipe.as_ref().map_or(f64::NAN, |e| e.total);
    if !eco.r.indisponivel && !eq.r.indisponivel && tot(&eq) < tot(&eco) - 1e-9 {
        let alvo = eco.r.alvo;
        let lista = eco.lista.clone();
        eco = PerfilComposto {
            r: ResultadoPerfil {
                alvo,
                ..eq.r.clone()
            },
            lista,
            completado: eq.completado,
            falta_coleta: eq.falta_coleta,
        };
    }
    if !eq.r.indisponivel && !eco.r.indisponivel && ids_de(c, &eq.r) == ids_de(c, &eco.r) {
        avisos.push("Boss fraco para as ofertas disponíveis: a equipe econômica já cumpre a margem da equilibrada, por isso as duas coincidem.".into());
    }
    let base = cheio
        .as_ref()
        .map(|s| (s.elegiveis, s.excluidas_preco, s.sem_avaliacao))
        .or_else(|| runs.get(1).copied())
        .or_else(|| runs.first().copied())
        .unwrap_or((0, 0, 0));
    Resolvido {
        perfis: [eco, eq, maxima],
        avisos,
        elegiveis: base.0,
        excluidas_preco: base.1,
        sem_avaliacao: base.2,
    }
}

fn vazio() -> ResultadoPerfil {
    ResultadoPerfil {
        alvo: None,
        indisponivel: true,
        inviavel: true,
        equipe: None,
        poder: 0.0,
        margem_estresse: 0.0,
        abaixo: 0,
        pior_faltam: 0.0,
        t_sobrevive: None,
        restr: None,
        teto_mult: None,
        teto_usado: None,
        alternativas: Vec::new(),
        reservas: Vec::new(),
    }
}

/// Entrada do `equipeBolsa`.
pub struct EntradaBolsa<'a> {
    pub motor: &'a MotorEquipes,
    pub boss: &'a BossBolsa<'a>,
    pub moeda: &'a str,
    pub idade_min: f64,
    pub n: usize,
    pub ofertas: &'a [OfertaEquipe],
    /// Cotação de cada oferta (o JSON da oferta avaliada), só em orb.
    pub cotacoes: &'a HashMap<i64, Value>,
    pub listas: Option<[Vec<AlvoEspecie>; 3]>,
    pub preco_de: PrecoDe<'a>,
    pub agora: i64,
}

fn limites(teto: Option<f64>) -> Value {
    json!({
        "piso": otimizador::LIMITE_PISO, "tetoNivel": otimizador::LIMITE_TETO_NIVEL, "tetoMult": teto.map(js::json_num),
        "heroi": otimizador::LIMITE_HEROI,
        "estresse": { "causado": otimizador::LIMITE_ESTRESSE.0, "recebido": otimizador::LIMITE_ESTRESSE.1 },
    })
}

fn restr_json(r: &ResultadoPerfil, mult: f64, n_slots: usize, n: usize) -> Value {
    let mut o = Map::new();
    o.insert("nivelOk".into(), Value::Bool(mult <= 1.000001));
    o.insert("equilibrada".into(), Value::Bool(n_slots == n));
    if let Some(x) = &r.restr {
        o.insert("piso".into(), Value::Bool(x.piso));
        o.insert("tetoNivel".into(), Value::Bool(x.teto_nivel));
        o.insert("mult".into(), Value::Bool(x.mult));
        o.insert("heroi".into(), Value::Bool(x.heroi));
        o.insert("estresse".into(), Value::Bool(x.estresse));
        o.insert("relaxadas".into(), json!(x.relaxadas));
    }
    Value::Object(o)
}

/// `equipeBolsa`: a melhor equipe a comprar contra o boss. Devolve a resposta e as faltas
/// atualizadas (que só são gravadas com a escrita ligada).
#[allow(clippy::too_many_lines)]
pub fn equipe_bolsa(e: &EntradaBolsa<'_>, faltas: &mut Faltas) -> Result<Value, &'static str> {
    let motor = e.motor;
    let boss = &e.boss.boss;
    let k = motor.modelo.constantes();
    let (foe, sp_boss) = motor
        .modelo
        .boss(&boss.para_combate(), &motor.dex)
        .ok_or("espécie do boss não está na Pokédex")?;
    let ref_eq = motor.equipe_ideal(boss, PerfilTeorico::Realista, 5, None);
    let soma = ref_eq
        .as_ref()
        .and_then(|r| r.get("soma"))
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    let ref_d = if soma == 0.0 || soma.is_nan() {
        foe.hp
    } else {
        soma
    }
    .max(1.0);
    let mut cartas: Vec<Carta<'_>> = Vec::new();
    for o in e.ofertas {
        let lvl = js::mais(o.ficha.get("level"));
        let unit = o.preco / ou(o.quantidade, 1.0).max(1.0);
        let Some((mon, sp)) = combatente::de_ficha(&motor.dex, &o.ficha, k) else {
            continue;
        };
        let r = motor.modelo.avaliar(&mon, &foe, 1.0);
        if !positivo(r.d) || !positivo(unit) {
            continue;
        }
        let ind = r.dps_causado
            * if r.dps_recebido > 0.0 {
                TEMPO_MAXIMO.min(
                    mon.hp
                        / (r.dps_recebido
                            * motor.modelo.penalidade_individual(lvl, boss.team_level)),
                )
            } else {
                TEMPO_MAXIMO
            };
        let cot = e.cotacoes.get(&o.id);
        let justo = cot
            .and_then(|c| c.get("justo"))
            .and_then(Value::as_f64)
            .unwrap_or(f64::NAN);
        cartas.push(Carta {
            id: o.id.to_string(),
            ficha: &o.ficha,
            sp,
            lvl,
            unit,
            hp: mon.hp,
            d: ind,
            r,
            justo,
            cotacao: cot,
        });
    }
    let boss_json = {
        let mut b = json!({
            "key": boss.key, "nome": boss.nome, "teamLevel": js::json_num(boss.team_level),
            "tipos": foe.tipos.iter().map(ToString::to_string).collect::<Vec<_>>(), "hp": js::json_num(js::round(foe.hp)),
            "sprite": motor.sprite(Some(sp_boss), false), "fonte": boss.fonte, "regiao": boss.regiao, "trinca": boss.trinca,
            "resumoTipos": resumo_tipos(motor, &foe.tipos),
        });
        if !cartas.is_empty() {
            if let Value::Object(o) = &mut b {
                o.insert(
                    "drops".into(),
                    if js::verdadeiro(Some(&boss.drops)) {
                        boss.drops.clone()
                    } else {
                        Value::Null
                    },
                );
                o.insert(
                    "entrada".into(),
                    if js::verdadeiro(Some(&boss.entrada)) {
                        boss.entrada.clone()
                    } else {
                        Value::Null
                    },
                );
            }
        }
        b
    };
    if cartas.is_empty() {
        return Ok(json!({
            "boss": boss_json, "moeda": e.moeda, "ofertasAnalisadas": e.ofertas.len(), "candidatas": 0, "vazio": true,
            "suspeitas": [], "perfis": {},
        }));
    }
    let cands: Vec<Candidato> = cartas
        .iter()
        .map(|x| Candidato {
            id: x.id.clone(),
            sp: x.sp.id,
            lvl: x.lvl,
            unit: x.unit,
            justo: x.justo,
            hp: x.hp,
            dps_out: x.r.dps_causado,
            dps_in: x.r.dps_recebido,
        })
        .collect();
    let op = Opcoes {
        n: e.n,
        nivel_boss: boss.team_level,
        referencia: ref_d,
        teto_justo: true,
        moeda_orb: e.moeda == "orb",
        so_max: false,
        so_eco: false,
    };
    let res = resolver_por_perfil(&cands, &op, boss, e.listas.as_ref(), faltas, e.agora);

    let slot = |i: usize, d: f64, dt: f64| -> Map<String, Value> {
        let x = &cartas[i];
        let f = x.ficha;
        let shiny = js::verdadeiro(f.get("shiny"));
        let nome = f
            .get("nome")
            .filter(|v| js::verdadeiro(Some(v)))
            .cloned()
            .unwrap_or_else(|| Value::from(x.sp.nome.clone()));
        let cot = |c: &str| {
            x.cotacao
                .and_then(|v| v.get(c))
                .cloned()
                .unwrap_or(Value::Null)
        };
        let v = json!({
            "ofertaId": x.id, "pokemonId": ou_nulo(f, "id"), "nome": nome, "speciesId": x.sp.id,
            "tipos": x.sp.tipos.iter().map(ToString::to_string).collect::<Vec<_>>(), "sprite": motor.sprite(Some(x.sp), shiny),
            "shiny": shiny, "preco": js::json_num(x.unit), "justo": js::json_num(x.justo), "desconto": cot("desconto"),
            "faixaJusta": cot("faixa"), "confianca": cot("confianca"), "nivel": js::json_num(x.lvl), "nota": ou_nulo(f, "nota"),
            "quality": ou_nulo(f, "quality"), "potencia": ou_nulo(f, "potencia"), "ivTotal": ou_nulo(f, "ivTotal"),
            "poder": ou_nulo(f, "poder"), "D": js::json_num(d), "dpsOut": js::json_num(x.r.dps_causado),
            "tDie": js::json_num(if x.r.dps_causado > 0.0 { d / x.r.dps_causado } else { 0.0 }),
            "ratio": js::json_num(d / x.unit), "share": js::json_num(if dt > 0.0 { d / dt } else { 0.0 }),
            "bate": eficacia(x.r.bate), "apanha": eficacia(x.r.apanha), "imune": x.r.imune,
            "faltam": js::json_num((boss.team_level - x.lvl).max(0.0)), "alternativas": [],
        });
        match v {
            Value::Object(o) => o,
            _ => Map::new(),
        }
    };
    let nomes = [
        ("eco", "Econômica"),
        ("eq", "Equilibrada"),
        ("max", "Máxima"),
    ];
    let mut perfis = Map::new();
    for (pi, pc) in res.perfis.iter().enumerate() {
        let (chave, nome) = nomes[pi];
        let p = &pc.r;
        let (slots, total, forca, mult) = match &p.equipe {
            Some(eq) => {
                let slots: Vec<Value> = eq
                    .pick
                    .iter()
                    .enumerate()
                    .map(|(i, &ci)| {
                        let mut s = slot(ci, eq.danos.get(i).copied().unwrap_or(0.0), eq.d);
                        let reservas: Vec<Value> = p
                            .reservas
                            .get(i)
                            .map_or(&[][..], Vec::as_slice)
                            .iter()
                            .map(|c| {
                                let x = &cartas[c.x];
                                let nome = x.ficha.get("nome").filter(|v| js::verdadeiro(Some(v))).cloned().unwrap_or_else(|| Value::from(x.sp.nome.clone()));
                                json!({
                                    "ofertaId": x.id, "nome": nome, "speciesId": x.sp.id,
                                    "sprite": motor.sprite(Some(x.sp), js::verdadeiro(x.ficha.get("shiny"))),
                                    "nivel": js::json_num(x.lvl), "preco": js::json_num(x.unit), "justo": js::json_num(x.justo),
                                    "margem": js::json_num(c.margem), "total": js::json_num(c.total),
                                })
                            })
                            .collect();
                        s.insert("reservas".into(), Value::Array(reservas));
                        Value::Object(s)
                    })
                    .collect();
                (slots, js::json_num(eq.total), eq.forca, eq.mult)
            }
            None => (Vec::new(), Value::Null, 0.0, 1.0),
        };
        let planos: Vec<Value> = p
            .alternativas
            .iter()
            .enumerate()
            .filter_map(|(i, a)| {
                let ae = a.equipe.as_ref()?;
                Some(json!({
                    "plano": if i == 0 { "B" } else { "C" },
                    "slots": ae.pick.iter().enumerate().map(|(j, &ci)| Value::Object(slot(ci, ae.danos.get(j).copied().unwrap_or(0.0), ae.d))).collect::<Vec<_>>(),
                    "total": js::json_num(ae.total), "margem": js::json_num(a.poder), "mult": js::json_num(ae.mult),
                    "margemEstresse": js::json_num(a.margem_estresse),
                }))
            })
            .collect();
        let n_slots = slots.len();
        let disponivel = p.equipe.is_some();
        let mut perfil = json!({
            "nome": nome, "listaPerfil": pc.lista, "completado": pc.completado, "alvo": p.alvo.map(js::json_num),
            "slots": slots, "total": total, "poder": js::json_num(p.poder), "forca": js::json_num(forca), "mult": js::json_num(mult),
            "inviavel": p.inviavel, "indisponivel": p.indisponivel, "abaixo": p.abaixo, "piorFaltam": js::json_num(p.pior_faltam),
            "tSobrevive": p.t_sobrevive.map(js::json_num), "danoRecebido": js::json_num(mult), "estimativa": true,
            "tetoUsado": p.teto_usado.map(js::json_num), "planos": planos,
            "margemEstresse": js::json_num(p.margem_estresse),
            "heroiShare": js::json_num(p.equipe.as_ref().map_or(0.0, |e| e.hero)),
            "limites": if disponivel { limites(p.teto_mult) } else { Value::Null },
            "restr": restr_json(p, mult, n_slots, e.n),
            "custoPorPoder": if p.poder > 0.0 { p.equipe.as_ref().map(|eq| js::json_num(eq.total / p.poder)) } else { None },
        });
        let lucro = lucro_perfil(
            &perfil,
            foe_hp_round(foe.hp),
            &boss.drops,
            &boss.entrada,
            e.preco_de,
        );
        if let Value::Object(o) = &mut perfil {
            o.insert("lucro".into(), lucro);
        }
        perfis.insert(chave.into(), perfil);
    }
    // Nuvem custo × dano e a fronteira de Pareto das cartas soltas.
    let mut pareto: Vec<usize> = (0..cartas.len()).collect();
    pareto.sort_by(|&a, &b| otimizador_ordem(cartas[a].unit - cartas[b].unit));
    let mut melhor = -1.0;
    let mut fronteira = Vec::new();
    for i in pareto {
        let x = &cartas[i];
        if x.d > melhor {
            melhor = x.d;
            fronteira.push(json!([
                js::json_num(x.unit),
                js::json_num(x.d),
                js::mais(Some(&Value::from(x.id.clone()))) as i64
            ]));
        }
    }
    let passo = if cartas.len() > 320 {
        cartas.len().div_ceil(320)
    } else {
        1
    };
    let nuvem: Vec<Value> = cartas
        .iter()
        .enumerate()
        .filter(|(i, _)| i % passo == 0)
        .map(|(_, x)| {
            json!([
                js::json_num(x.unit),
                js::json_num(x.d),
                js::mais(Some(&Value::from(x.id.clone()))) as i64
            ])
        })
        .collect();
    let aviso: Vec<String> = e
        .boss
        .avisos
        .iter()
        .cloned()
        .chain(res.avisos.iter().cloned())
        .collect();
    let ref_nomes: Vec<Value> = ref_eq
        .as_ref()
        .and_then(|r| r.get("equipe"))
        .and_then(Value::as_array)
        .map(|eq| {
            eq.iter()
                .take(5)
                .map(|m| m.get("nome").cloned().unwrap_or(Value::Null))
                .collect()
        })
        .unwrap_or_default();
    Ok(json!({
        "boss": boss_json, "moeda": e.moeda, "ref": { "soma": js::json_num(js::round(ref_d)), "equipe": ref_nomes },
        "ofertasAnalisadas": e.ofertas.len(), "candidatas": cartas.len(), "suspeitasN": 0, "suspeitas": [],
        "idadeMin": js::json_num(e.idade_min), "geradoEm": e.agora, "perfis": perfis,
        "filtroPreco": {
            "teto": if e.moeda == "orb" { Value::from("valor justo estimado") } else { Value::Null },
            "elegiveis": res.elegiveis, "acimaDoJusto": res.excluidas_preco, "semAvaliacao": res.sem_avaliacao,
        },
        "metodo": otimizador::METODO,
        "aviso": if aviso.is_empty() { Value::Null } else { Value::from(aviso.join(" ")) },
        "nuvem": nuvem, "fronteira": fronteira,
    }))
}

fn foe_hp_round(hp: f64) -> f64 {
    js::round(hp)
}

/// Entrada do `equipePropria`.
pub struct EntradaPropria<'a> {
    pub motor: &'a MotorEquipes,
    pub estado: &'a EstadoEquipes,
    pub boss: &'a BossBolsa<'a>,
    pub nick: Option<&'a str>,
    /// Valor justo de uma ficha (o precificador).
    pub valor_ficha: &'a dyn Fn(&Value) -> Option<f64>,
    pub preco_de: PrecoDe<'a>,
    /// Equipe econômica de compra já calculada para o mesmo boss: (total, vagas).
    pub eco_compra: Option<(f64, usize)>,
    pub agora: i64,
}

/// `equipePropria`: a melhor equipe só com os pokémon da conta.
#[allow(clippy::too_many_lines)]
pub fn equipe_propria(e: &EntradaPropria<'_>) -> Result<Value, &'static str> {
    let motor = e.motor;
    let boss = &e.boss.boss;
    let conta = e.boss.conta.ok_or("conta não encontrada")?;
    let k = motor.modelo.constantes();
    let (foe, sp_boss) = motor
        .modelo
        .boss(&boss.para_combate(), &motor.dex)
        .ok_or("espécie do boss não está na Pokédex")?;
    let l = boss.team_level;
    let pk: Vec<&Value> = conta
        .get("pokemons")
        .and_then(Value::as_array)
        .map(|v| {
            v.iter()
                .filter(|f| js::verdadeiro(Some(f)) && js::mais(f.get("level")) > 0.0)
                .collect()
        })
        .unwrap_or_default();
    let ref_eq = motor.equipe_ideal(boss, PerfilTeorico::Realista, 5, None);
    let soma = ref_eq
        .as_ref()
        .and_then(|r| r.get("soma"))
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    let ref_d = if soma == 0.0 || soma.is_nan() {
        foe.hp
    } else {
        soma
    }
    .max(1.0);
    let mut todos: Vec<Candidato> = Vec::new();
    let mut origem: Vec<(&Value, &EspecieCombate, Avaliacao)> = Vec::new();
    for f in &pk {
        let sid = js::mais(f.get("speciesId"));
        let c = combatente::de_ficha(&motor.dex, f, k).or_else(|| {
            (13000.0..14000.0).contains(&sid).then(|| {
                let mut g = (*f).clone();
                if let Value::Object(o) = &mut g {
                    o.insert("speciesId".into(), js::json_num(sid - 13000.0));
                }
                combatente::de_ficha(&motor.dex, &g, k)
            })?
        });
        let Some((mon, sp)) = c else { continue };
        let r = motor.modelo.avaliar(&mon, &foe, 1.0);
        if !positivo(r.d) {
            continue;
        }
        let lvl = js::mais(f.get("level"));
        let unit = lvl.max(1.0)
            * if js::verdadeiro(f.get("shiny")) {
                3.0
            } else {
                1.0
            };
        todos.push(Candidato {
            id: js::texto(f.get("id")),
            sp: sp.id,
            lvl,
            unit,
            justo: f64::NAN,
            hp: mon.hp,
            dps_out: r.dps_causado,
            dps_in: r.dps_recebido,
        });
        origem.push((f, sp, r));
    }
    let mut avisos = e.boss.avisos.clone();
    let teto = otimizador::LIMITE_TETO_NIVEL * l;
    let mut pool: Vec<usize> = (0..todos.len()).filter(|&i| todos[i].lvl <= teto).collect();
    let teto_txt = js::numero(js::round(teto));
    if pool.len() < 5 {
        if todos.len() >= 5 {
            avisos.push(format!(
                "Só {} Pokémon desta conta têm nível até {}× o do boss ({teto_txt}): relaxei o teto para completar os 5.",
                pool.len(),
                js::numero(otimizador::LIMITE_TETO_NIVEL)
            ));
        }
        pool = (0..todos.len()).collect();
    } else if todos.len() > pool.len() {
        avisos.push(format!(
            "{} Pokémon acima de {teto_txt} ({}× o nível do boss) ficaram de fora, para não gastar os melhores num boss fácil.",
            todos.len() - pool.len(),
            js::numero(otimizador::LIMITE_TETO_NIVEL)
        ));
    }
    let passam = todos.iter().filter(|x| x.lvl >= l).count();
    let contas: Vec<&str> = e
        .estado
        .contas
        .iter()
        .filter(|(_, c)| {
            c.get("pokemons")
                .and_then(Value::as_array)
                .is_some_and(|p| !p.is_empty())
        })
        .map(|(n, _)| n.as_str())
        .collect();
    let mut saida = json!({
        "contas": contas,
        "boss": {
            "key": boss.key, "nome": boss.nome, "teamLevel": js::json_num(l),
            "tipos": foe.tipos.iter().map(ToString::to_string).collect::<Vec<_>>(), "hp": js::json_num(js::round(foe.hp)),
            "sprite": motor.sprite(Some(sp_boss), false), "fonte": boss.fonte, "regiao": boss.regiao, "trinca": boss.trinca,
            "drops": if js::verdadeiro(Some(&boss.drops)) { boss.drops.clone() } else { Value::Null },
            "entrada": if js::verdadeiro(Some(&boss.entrada)) { boss.entrada.clone() } else { Value::Null },
        },
        "conta": {
            "nick": e.nick, "trLv": js::nulo_ou(conta.get("trLv"), None), "pokemons": pk.len(), "avaliados": todos.len(),
            "noTeto": pool.len(), "passam": passam, "tetoNivel": otimizador::LIMITE_TETO_NIVEL,
        },
        "ref": { "soma": js::json_num(js::round(ref_d)) }, "geradoEm": e.agora, "estimativa": true, "perfis": {},
    });
    let Value::Object(so) = &mut saida else {
        return Err("falha interna");
    };
    if pool.len() < 5 {
        avisos.push(format!(
            "A conta tem só {} Pokémon avaliáveis (precisa de 5).",
            todos.len()
        ));
        so.insert("vazio".into(), Value::Bool(true));
        so.insert("aviso".into(), Value::from(avisos.join(" ")));
        return Ok(saida);
    }
    let sub: Vec<Candidato> = pool.iter().map(|&i| todos[i].clone()).collect();
    let op = Opcoes {
        n: 5,
        nivel_boss: l,
        referencia: ref_d,
        teto_justo: false,
        moeda_orb: true,
        so_max: false,
        so_eco: true,
    };
    let r = otimizador::selecionar(&sub, &op);
    for a in &r.avisos {
        if !a.contains("Econômica: a opção mais barata") {
            avisos.push(match a.strip_prefix("Econômica:") {
                Some(resto) => format!("Equipe suficiente:{resto}"),
                None => a.clone(),
            });
        }
    }
    let mut perfis = Map::new();
    for (chave, nome, p) in [
        ("eco", "Suficiente (o mais fraco que cumpre)", &r.eco),
        ("max", "Mais forte possível", &r.max),
    ] {
        let Some(eq) = p.equipe.as_ref().filter(|_| !p.indisponivel) else {
            perfis.insert(chave.into(), json!({ "nome": nome, "indisponivel": true }));
            continue;
        };
        let slots: Vec<Value> = eq
            .pick
            .iter()
            .enumerate()
            .map(|(i, &si)| {
                let (f, sp, rr) = &origem[pool[si]];
                let x = &sub[si];
                let d = eq.danos.get(i).copied().unwrap_or(0.0);
                let mut g = (*f).clone();
                if let Value::Object(o) = &mut g {
                    o.insert("nome".into(), Value::from(sp.nome.clone()));
                }
                let justo = (e.valor_ficha)(&g);
                let shiny = js::verdadeiro(f.get("shiny"));
                json!({
                    "pokemonId": ou_nulo(f, "id"), "nome": sp.nome, "speciesId": sp.id,
                    "tipos": sp.tipos.iter().map(ToString::to_string).collect::<Vec<_>>(), "sprite": motor.sprite(Some(sp), shiny),
                    "shiny": shiny, "nivel": js::json_num(x.lvl), "quality": ou_nulo(f, "quality"), "potencia": ou_nulo(f, "potencia"),
                    "D": js::json_num(d), "dpsOut": js::json_num(rr.dps_causado),
                    "tDie": js::json_num(if rr.dps_causado > 0.0 { d / rr.dps_causado } else { 0.0 }),
                    "share": js::json_num(if eq.d > 0.0 { d / eq.d } else { 0.0 }), "bate": eficacia(rr.bate),
                    "apanha": eficacia(rr.apanha), "imune": rr.imune, "faltam": js::json_num((l - x.lvl).max(0.0)),
                    "justo": justo.map(js::json_num), "preco": 0,
                })
            })
            .collect();
        let custo = slots.iter().fold(0.0, |s, x| {
            let j = js::mais(x.get("justo"));
            s + if j > 0.0 { j } else { 0.0 }
        });
        let sem = slots
            .iter()
            .filter(|x| !positivo(js::mais(x.get("justo"))))
            .count();
        let restr = p.restr.as_ref().map(|x| {
            json!({ "piso": x.piso, "tetoNivel": x.teto_nivel, "mult": x.mult, "heroi": x.heroi, "estresse": x.estresse, "relaxadas": x.relaxadas })
        });
        let mut perfil = json!({
            "nome": nome, "slots": slots, "total": 0, "poder": js::json_num(p.poder), "forca": js::json_num(eq.forca),
            "mult": js::json_num(eq.mult), "abaixo": p.abaixo, "piorFaltam": js::json_num(p.pior_faltam),
            "tSobrevive": p.t_sobrevive.map(js::json_num), "danoRecebido": js::json_num(eq.mult),
            "margemEstresse": js::json_num(p.margem_estresse), "heroiShare": js::json_num(eq.hero), "estimativa": true,
            "restr": restr, "custoOportunidade": js::json_num(custo), "semCotacao": sem,
        });
        let lucro = lucro_perfil(
            &perfil,
            js::round(foe.hp),
            &boss.drops,
            &boss.entrada,
            e.preco_de,
        );
        if let Value::Object(o) = &mut perfil {
            o.insert("lucro".into(), lucro);
        }
        perfis.insert(chave.into(), perfil);
    }
    so.insert("perfis".into(), Value::Object(perfis));
    if passam < 5 {
        let sugestao = match e.eco_compra {
            Some((total, vagas)) if vagas > 0 => {
                let por = total / vagas as f64;
                json!({
                    "custoMedioPorSlot": js::json_num(js::round(por)), "custoAproximado": js::json_num(js::round(por * (5 - passam) as f64)),
                    "moeda": "orb", "nota": "média por vaga da equipe Econômica de compra × vagas que faltam (aproximado)",
                })
            }
            _ => json!({ "nota": "Veja a aba Econômica para o custo de completar a equipe." }),
        };
        so.insert(
            "falta".into(),
            json!({ "passam": passam, "faltam": 5 - passam, "sugestao": sugestao }),
        );
        avisos.push(format!(
            "Só {passam} Pokémon da conta têm nível ≥ {}: faltam {} para a equipe completa sem penalidade.",
            js::numero(l),
            5 - passam
        ));
    }
    so.insert(
        "aviso".into(),
        if avisos.is_empty() {
            Value::Null
        } else {
            Value::from(avisos.join(" "))
        },
    );
    Ok(saida)
}

// ---------- alvos de coleta (rota) ----------

const MAX_PADRAO: f64 = 40.0;
const MAX_TOTAL: f64 = 60.0;

/// `contexto(nick)`: nível de treinador, bosses alcançáveis e a faixa de nível dos preços.
pub fn contexto_alvos(
    motor: &MotorEquipes,
    estado: &EstadoEquipes,
    nick: Option<&str>,
) -> (f64, Vec<BossLista>, f64, f64) {
    let mut tr = 0.0;
    if let Some(n) = nick.filter(|n| !n.is_empty()) {
        tr = ou(
            js::mais(estado.conta(Some(n)).and_then(|c| c.get("trLv"))),
            0.0,
        );
    }
    if tr == 0.0 {
        for (_, c) in &estado.contas {
            tr = otimizador_max(tr, ou(js::mais(c.get("trLv")), 0.0));
        }
    }
    let mut lista: Vec<BossLista> = motor
        .lista_bosses(&estado.bosses)
        .into_iter()
        .filter(|b| b.team_level != 0.0 && b.team_level <= tr)
        .collect();
    lista.sort_by(|a, b| a.team_level.total_cmp(&b.team_level));
    let lo = lista
        .first()
        .map_or(1.0, |b| js::round(0.7 * b.team_level).max(1.0));
    let hi = lista.last().map_or(1.0, |b| js::round(2.5 * b.team_level));
    (tr, lista, lo, hi)
}

fn otimizador_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else {
        a.max(b)
    }
}

/// `alvosColeta`: as espécies úteis nos bosses que a conta alcança, por perfil.
pub fn alvos_coleta(
    motor: &MotorEquipes,
    estado: &EstadoEquipes,
    nick: Option<&str>,
    max: Option<&str>,
    precos: &HashMap<i64, f64>,
    faltas: &[(String, String, usize)],
    agora: i64,
) -> Value {
    let pedido = js::mais(max.map(Value::from).as_ref());
    let max = MAX_TOTAL.min(ou(pedido, MAX_PADRAO)).max(1.0);
    let (tr, lista, _, _) = contexto_alvos(motor, estado, nick);
    let ex: HashMap<String, usize> = faltas
        .iter()
        .map(|(b, p, n)| (format!("{b}|{p}"), *n))
        .collect();
    let rankings: Vec<(&BossLista, [Vec<AlvoEspecie>; 3])> = lista
        .iter()
        .map(|b| {
            let extras =
                PERFIS_LISTA.map(|p| ex.get(&format!("{}|{p}", b.key)).copied().unwrap_or(0));
            let l = avaliar_especies(motor, b).map_or_else(
                || [Vec::new(), Vec::new(), Vec::new()],
                |a| listas_do_boss(&a, precos, extras),
            );
            (b, l)
        })
        .collect();
    // (speciesId, alvo) na ordem de entrada.
    let mut alvos: Vec<(i64, Map<String, Value>)> = Vec::new();
    let mut add = |boss: &BossLista, e: &AlvoEspecie, perfil: &str, extra: bool| {
        let lo = js::round(0.7 * boss.team_level).max(1.0);
        let hi = js::round(2.5 * boss.team_level);
        if let Some((_, a)) = alvos.iter_mut().find(|(id, _)| *id == e.species_id) {
            let nmin = js::mais(a.get("nivelMin")).min(lo);
            let nmax = js::mais(a.get("nivelMax")).max(hi);
            a.insert("nivelMin".into(), js::json_num(nmin));
            a.insert("nivelMax".into(), js::json_num(nmax));
            for (campo, valor) in [("bosses", boss.nome.as_str()), ("perfis", perfil)] {
                if let Some(Value::Array(v)) = a.get_mut(campo) {
                    if !v.iter().any(|x| x.as_str() == Some(valor)) {
                        v.push(Value::from(valor));
                    }
                }
            }
            let ja = a.get("extra").and_then(Value::as_bool).unwrap_or(false);
            a.insert("extra".into(), Value::Bool(ja && extra));
        } else if (alvos.len() as f64) < max {
            let mut a = Map::new();
            a.insert("speciesId".into(), Value::from(e.species_id));
            a.insert("nome".into(), Value::from(e.nome.clone()));
            a.insert("nivelMin".into(), js::json_num(lo));
            a.insert("nivelMax".into(), js::json_num(hi));
            a.insert("bosses".into(), json!([boss.nome]));
            a.insert("perfis".into(), json!([perfil]));
            a.insert("extra".into(), Value::Bool(extra));
            alvos.push((e.species_id, a));
        }
    };
    let nivel_baixo = rankings.first().map_or(0.0, |(b, _)| b.team_level);
    let base = |grupo: &[&(&BossLista, [Vec<AlvoEspecie>; 3])],
                add: &mut dyn FnMut(&BossLista, &AlvoEspecie, &str, bool)| {
        for i in 0..ALVOS_POR_PERFIL {
            for (boss, listas) in grupo {
                for (pi, perfil) in PERFIS_LISTA.iter().enumerate() {
                    if let Some(e) = listas[pi].iter().filter(|x| !x.extra).nth(i) {
                        add(boss, e, perfil, false);
                    }
                }
            }
        }
    };
    let baixos: Vec<&(&BossLista, [Vec<AlvoEspecie>; 3])> = rankings
        .iter()
        .filter(|(b, _)| b.team_level == nivel_baixo)
        .collect();
    let altos: Vec<&(&BossLista, [Vec<AlvoEspecie>; 3])> = rankings
        .iter()
        .filter(|(b, _)| b.team_level != nivel_baixo)
        .collect();
    base(&baixos, &mut add);
    for i in 0..EXTRA_MAX_POR_PERFIL {
        for (boss, listas) in &rankings {
            for (pi, perfil) in PERFIS_LISTA.iter().enumerate() {
                if let Some(e) = listas[pi].iter().filter(|x| x.extra).nth(i) {
                    add(boss, e, perfil, true);
                }
            }
        }
    }
    base(&altos, &mut add);
    let nomes = |l: &[AlvoEspecie], extra: bool| {
        l.iter()
            .filter(|x| x.extra == extra)
            .map(|x| x.nome.clone())
            .collect::<Vec<_>>()
    };
    let bosses: Vec<Value> = rankings
        .iter()
        .map(|(b, l)| {
            let mut extras = Map::new();
            for (pi, p) in PERFIS_LISTA.iter().enumerate() {
                let n = nomes(&l[pi], true);
                if !n.is_empty() {
                    extras.insert((*p).into(), json!(n));
                }
            }
            json!({
                "nome": b.nome, "nivel": js::json_num(b.team_level), "tipos": b.tipos,
                "maxima": nomes(&l[0], false), "equilibrada": nomes(&l[1], false), "economica": nomes(&l[2], false),
                "extras": extras,
            })
        })
        .collect();
    json!({
        "geradoEm": agora, "trLv": js::json_num(tr), "piso": otimizador::LIMITE_PISO, "teto": otimizador::LIMITE_TETO_NIVEL,
        "max": js::json_num(max), "porPerfil": { "maxima": 5, "equilibrada": 5, "economica": 5 },
        "extrasMax": EXTRA_MAX_POR_PERFIL, "bosses": bosses,
        "alvos": alvos.into_iter().map(|(_, a)| Value::Object(a)).collect::<Vec<_>>(),
    })
}

/// `x > 0` do JS, com `NaN` falso: `!positivo(x)` é o `!(x > 0)` do legado.
fn positivo(x: f64) -> bool {
    x > 0.0
}
