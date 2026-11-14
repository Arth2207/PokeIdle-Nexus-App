//! Equipes por boss: lista de bosses, próximo boss, equipe teórica (`equipeIdeal`), equipe
//! com a coleção da conta (`equipeColecao`) e as respostas de `/api/equipes/*`.
//!
//! Cópia do `equipes-core.js`, `equipes.js` e `equipes-estado.js` do legado. As respostas
//! saem como `serde_json::Value` no mesmo formato do Node, porque boa parte dos campos são
//! valores das fichas repassados sem tratamento.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use serde_json::{json, Map, Value};
use unicode_normalization::UnicodeNormalization;

use crate::combate::combatente::{self, Combatente, Perfil};
use crate::combate::duelo::{Avaliacao, TAMANHO_EQUIPE};
use crate::combate::especie::{EspecieCombate, Pokedex};
use crate::combate::modelo::{Boss, Legado, ModeloCombate};
use crate::combate::tipos::Tipo;
use crate::js;

const POKEAPI: &str = "https://raw.githubusercontent.com/PokeAPI/sprites/master/sprites/pokemon/";
/// Aviso que acompanha o plano: o boss do modelo legado é uma estimativa.
pub const NOTA_PLANO: &str =
    "Estimativa: HP e golpes do boss são aproximados pela espécie (a ficha real do boss não é pública).";

/// `norm` do legado: minúsculas, sem acentos, tudo que não é letra ou número vira `-`.
pub fn norm(texto: &str) -> String {
    let base: String = texto
        .to_lowercase()
        .nfd()
        .filter(|c| !('\u{0300}'..='\u{036f}').contains(c))
        .collect();
    let mut saida = String::with_capacity(base.len());
    let mut traco = false;
    for c in base.chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            saida.push(c);
            traco = false;
        } else if !traco {
            saida.push('-');
            traco = true;
        }
    }
    let saida = saida.strip_prefix('-').unwrap_or(&saida);
    saida.strip_suffix('-').unwrap_or(saida).to_string()
}

/// Tabela de reserva: 27 bosses em 9 trincas por região.
const REGIOES: [(&str, &str, f64, [&str; 3]); 9] = [
    (
        "Kanto",
        "Trio Aves Lendárias",
        300.0,
        ["Articuno", "Moltres", "Zapdos"],
    ),
    (
        "Johto",
        "Trio Bestas Lendárias",
        650.0,
        ["Suicune", "Entei", "Raikou"],
    ),
    (
        "Hoenn",
        "Trio Climático",
        3100.0,
        ["Groudon", "Kyogre", "Rayquaza"],
    ),
    (
        "Sinnoh",
        "Trio da Criação",
        9750.0,
        ["Dialga", "Palkia", "Giratina"],
    ),
    (
        "Unova",
        "Trio \"Tao\"",
        22700.0,
        ["Reshiram", "Zekrom", "Kyurem"],
    ),
    (
        "Kalos",
        "Trio da Aura",
        44050.0,
        ["Xerneas", "Yveltal", "Zygarde"],
    ),
    (
        "Alola",
        "Trio da Luz",
        75900.0,
        ["Solgaleo", "Lunala", "Necrozma"],
    ),
    (
        "Galar",
        "Trio do Combate",
        120350.0,
        ["Zacian Crowned", "Zamazenta Crowned", "Eternatus"],
    ),
    (
        "Paldea",
        "Trio da Lealdade",
        179500.0,
        ["Okidogi", "Munkidori", "Fezandipiti"],
    ),
];

/// Um boss da lista (jogo ou wiki).
#[derive(Debug, Clone, PartialEq)]
pub struct BossLista {
    pub key: String,
    pub nome: String,
    pub team_level: f64,
    pub min_tr: f64,
    pub regiao: Option<String>,
    pub trinca: Option<String>,
    pub pos: Option<u8>,
    /// Como vieram (maiúsculas); os desconhecidos são ignorados no combate.
    pub tipos: Vec<String>,
    pub entrada: Value,
    pub drops: Value,
    /// `"jogo"` ou `"wiki"`.
    pub fonte: &'static str,
}

impl BossLista {
    /// O boss para o modelo de combate.
    pub fn para_combate(&self) -> Boss {
        Boss {
            key: self.key.clone(),
            nome: self.nome.clone(),
            nivel_equipe: self.team_level,
            tipos: self.tipos.iter().filter_map(|t| t.parse().ok()).collect(),
        }
    }
}

fn nomes_tipos(tipos: &[Tipo]) -> Vec<String> {
    tipos.iter().map(ToString::to_string).collect()
}

/// O motor de equipes: a pokédex, o modelo de combate e os índices de sprite.
#[derive(Debug, Clone)]
pub struct MotorEquipes {
    pub dex: Pokedex,
    /// O modelo de combate. `dyn` porque a escolha é feita em tempo de execução (o legado
    /// responde; o modelo do jogo roda ao lado, só nas métricas).
    pub modelo: Arc<dyn ModeloCombate>,
    /// Nome (minúsculo) → id da primeira espécie com id de 1 a 1025, para achar o sprite de
    /// formas (megas, regionais).
    nome_base: HashMap<String, i64>,
    wiki: Vec<BossLista>,
}

impl MotorEquipes {
    /// Motor com o modelo legado.
    pub fn novo(dex: Pokedex) -> Self {
        Self::com_modelo(dex, Arc::new(Legado::novo()))
    }

    pub fn com_modelo(dex: Pokedex, modelo: Arc<dyn ModeloCombate>) -> Self {
        let mut nome_base = HashMap::new();
        for sp in dex.especies() {
            if sp.id > 0 && sp.id <= 1025 {
                nome_base.entry(sp.nome.to_lowercase()).or_insert(sp.id);
            }
        }
        let wiki = REGIOES
            .iter()
            .flat_map(|(regiao, trinca, nivel, nomes)| {
                nomes.iter().enumerate().map(|(i, nome)| BossLista {
                    key: norm(nome),
                    nome: (*nome).to_string(),
                    team_level: *nivel,
                    min_tr: *nivel,
                    regiao: Some((*regiao).to_string()),
                    trinca: Some((*trinca).to_string()),
                    pos: u8::try_from(i + 1).ok(),
                    tipos: dex
                        .por_nome(nome)
                        .map(|sp| nomes_tipos(&sp.tipos))
                        .unwrap_or_default(),
                    entrada: Value::Null,
                    drops: Value::Null,
                    fonte: "wiki",
                })
            })
            .collect();
        MotorEquipes {
            dex,
            modelo,
            nome_base,
            wiki,
        }
    }

    /// `dexBase`: id da espécie "base" para o sprite.
    fn dex_base(&self, sp: &EspecieCombate) -> Option<i64> {
        if sp.id > 0 && sp.id <= 1025 {
            return Some(sp.id);
        }
        let nome = sp.nome.to_lowercase().replace('-', " ");
        let palavras: Vec<&str> = nome.split_whitespace().collect();
        for i in 0..palavras.len() {
            for j in (i + 1..=palavras.len()).rev() {
                if let Some(b) = palavras
                    .get(i..j)
                    .and_then(|p| self.nome_base.get(&p.join(" ")))
                {
                    return Some(*b);
                }
            }
        }
        let m = sp.id % 1000;
        (m > 0 && m <= 1025).then_some(m)
    }

    /// Id da espécie base (Pokédex nacional) de uma espécie do jogo, para a imagem: as formas
    /// especiais ("Furious Scizor", 2050) caem na base pelo nome ("Scizor", 212).
    pub fn dex_de(&self, id: i64) -> Option<i64> {
        self.dex.por_id(id).and_then(|sp| self.dex_base(sp))
    }

    /// Bioma (fundo do painel) de uma hunt pelo texto da tela ("Furious Scizor · Outland 2 ·
    /// nível 150"): a Outland tem o seu; senão, pelo tipo principal da espécie da hunt.
    pub fn bioma_da_hunt(&self, hunt: &str) -> &'static str {
        if hunt.contains("Outland") {
            return "outland";
        }
        let nome = hunt.split(" · ").next().unwrap_or("").trim();
        self.dex
            .especies()
            .iter()
            .find(|e| e.nome.eq_ignore_ascii_case(nome))
            .and_then(|e| e.tipos.first())
            .map_or("floresta", |t| bioma_do_tipo(*t))
    }

    /// `spriteDe`.
    pub fn sprite(&self, sp: Option<&EspecieCombate>, shiny: bool) -> Value {
        sp.and_then(|sp| self.dex_base(sp))
            .map_or(Value::Null, |d| {
                Value::from(format!(
                    "{POKEAPI}{}{d}.png",
                    if shiny { "shiny/" } else { "" }
                ))
            })
    }

    /// `listaBosses(vivos)`: a lista viva do jogo, completada pela wiki; sem lista, a wiki.
    pub fn lista_bosses(&self, vivos: &[Value]) -> Vec<BossLista> {
        if vivos.is_empty() {
            return self.wiki.clone();
        }
        let wiki_por = |k: &str| self.wiki.iter().find(|b| b.key == k);
        let mut lista: Vec<BossLista> = vivos
            .iter()
            .filter_map(|v| {
                let nome_v = ["nome", "name", "key"]
                    .iter()
                    .find_map(|c| v.get(*c).filter(|x| js::verdadeiro(Some(x))));
                let nome = js::texto(nome_v);
                let chave = v.get("key").filter(|x| js::verdadeiro(Some(x))).or(nome_v);
                let key = norm(&js::texto(chave));
                let w = wiki_por(&key).or_else(|| wiki_por(&norm(&nome)));
                let mut tipos: Vec<String> = match v.get("tipos").and_then(Value::as_array) {
                    Some(t) if !t.is_empty() => t
                        .iter()
                        .map(|x| js::texto(Some(x)).to_uppercase())
                        .collect(),
                    _ => ["type1", "type2"]
                        .iter()
                        .filter_map(|c| v.get(*c).filter(|x| js::verdadeiro(Some(x))))
                        .map(|x| js::texto(Some(x)).to_uppercase())
                        .collect(),
                };
                if tipos.is_empty() {
                    tipos = match w {
                        Some(w) if !w.tipos.is_empty() => w.tipos.clone(),
                        _ => self
                            .dex
                            .por_nome(&nome)
                            .map(|sp| nomes_tipos(&sp.tipos))
                            .unwrap_or_default(),
                    };
                }
                let wiki_tl = w.map(|w| Value::from(w.team_level));
                let tl = js::mais(js::nulo_ou(
                    v.get("teamLevel"),
                    js::nulo_ou(v.get("level"), wiki_tl.as_ref()),
                ));
                if tl == 0.0 || tl.is_nan() {
                    return None;
                }
                let tl_v = Value::from(tl);
                let mt = js::mais(js::nulo_ou(
                    v.get("minTr"),
                    js::nulo_ou(v.get("minNivelTreinador"), Some(&tl_v)),
                ));
                let campo = |c: &str| {
                    v.get(c)
                        .filter(|x| js::verdadeiro(Some(x)))
                        .cloned()
                        .unwrap_or(Value::Null)
                };
                Some(BossLista {
                    key,
                    nome,
                    team_level: tl,
                    min_tr: if mt == 0.0 || mt.is_nan() { tl } else { mt },
                    regiao: w.and_then(|w| w.regiao.clone()),
                    trinca: w.and_then(|w| w.trinca.clone()),
                    pos: w.and_then(|w| w.pos),
                    tipos,
                    entrada: campo("entrada"),
                    drops: campo("drops"),
                    fonte: "jogo",
                })
            })
            .collect();
        lista.sort_by(|a, b| a.team_level.total_cmp(&b.team_level));
        lista
    }

    /// `equipeIdeal`: a melhor equipe teórica (por tipo + simulação).
    pub fn equipe_ideal(
        &self,
        boss: &BossLista,
        perfil: Perfil,
        n: usize,
        tr_lv: Option<f64>,
    ) -> Option<Value> {
        let k = self.modelo.constantes();
        let (foe, sp_boss) = self.modelo.boss(&boss.para_combate(), &self.dex)?;
        let tipos = foe.tipos.clone();
        let alcanca = |sp: &EspecieCombate| {
            tr_lv.is_none()
                || perfil == Perfil::Ideal
                || sp
                    .nivel_captura
                    .is_some_and(|h| tr_lv.is_some_and(|t| h <= t))
        };
        let mut candidatos: Vec<(&EspecieCombate, usize, f64)> = self
            .dex
            .especies()
            .iter()
            .filter(|sp| {
                sp.de_pokebola()
                    && !sp.lendaria()
                    && !(sp.id >= 2000 && sp.id < 3000)
                    && alcanca(sp)
            })
            .map(|sp| {
                let se = sp
                    .golpes
                    .iter()
                    .filter(|g| g.poder > 0.0 && k.efetividade_bruta(g.tipo, &tipos) > 1.0)
                    .count();
                (sp, se, sp.base.iter().sum::<f64>())
            })
            .collect();
        candidatos.sort_by(|a, b| b.1.cmp(&a.1).then(b.2.total_cmp(&a.2)));
        candidatos.truncate(140);
        let mut avaliados: Vec<(&EspecieCombate, Avaliacao)> = candidatos
            .into_iter()
            .map(|(sp, _, _)| {
                let me = combatente::teorico(sp, boss.team_level, perfil, k);
                (sp, self.modelo.avaliar(&me, &foe, 1.0))
            })
            .filter(|(_, r)| r.d > 0.0)
            .collect();
        avaliados.sort_by(|a, b| b.1.d.total_cmp(&a.1.d));

        let mut escolhidos = Vec::new();
        let mut por_tipo: HashMap<Option<Tipo>, usize> = HashMap::new();
        let mut familias = HashSet::new();
        for (sp, r) in avaliados {
            let t1 = sp.tipos.first().copied();
            let nome = sp.nome.to_lowercase();
            let familia = nome.split(' ').next_back().unwrap_or_default().to_string();
            if por_tipo.get(&t1).copied().unwrap_or(0) >= 2 || familias.contains(&familia) {
                continue;
            }
            *por_tipo.entry(t1).or_insert(0) += 1;
            familias.insert(familia);
            escolhidos.push((sp, r));
            if escolhidos.len() >= n {
                break;
            }
        }
        let soma = escolhidos
            .iter()
            .take(TAMANHO_EQUIPE)
            .fold(0.0, |s, (_, r)| s + r.d);
        let equipe: Vec<Value> = escolhidos
            .iter()
            .map(|(sp, r)| {
                json!({
                    "speciesId": sp.id, "nome": sp.nome, "tipos": nomes_tipos(&sp.tipos),
                    "sprite": self.sprite(Some(sp), false), "nivel": js::json_num(boss.team_level),
                    "D": js::json_num(js::round(r.d)), "dpsOut": js::json_num(js::round(r.dps_causado)),
                    "tDie": js::json_num(js::round(r.tempo_ate_cair)),
                    "bate": eficacia(r.bate), "apanha": eficacia(r.apanha), "imune": r.imune,
                    "share": js::json_num(r.d / soma),
                })
            })
            .collect();
        Some(json!({
            "boss": {
                "key": boss.key, "nome": boss.nome, "teamLevel": js::json_num(boss.team_level),
                "tipos": nomes_tipos(&tipos), "hp": js::json_num(foe.hp),
                "sprite": self.sprite(Some(sp_boss), false), "fonte": boss.fonte,
            },
            "perfil": nome_perfil(perfil), "soma": js::json_num(soma), "cobertura": js::json_num(soma / foe.hp),
            "equipe": equipe,
        }))
    }

    /// `equipeColecao`: a melhor equipe com as fichas da conta.
    pub fn equipe_colecao(&self, boss: &BossLista, pokemons: &[Value], n: usize) -> Option<Value> {
        let k = self.modelo.constantes();
        let (foe, sp_boss) = self.modelo.boss(&boss.para_combate(), &self.dex)?;
        let mut itens: Vec<(&Value, Combatente, &EspecieCombate, Avaliacao)> = pokemons
            .iter()
            .filter_map(|f| {
                let (mon, sp) = combatente::de_ficha(&self.dex, f, k)?;
                let r = self.modelo.avaliar(
                    &mon,
                    &foe,
                    self.modelo
                        .penalidade_individual(js::mais(f.get("level")), boss.team_level),
                );
                Some((f, mon, sp, r))
            })
            .collect();
        let avaliados = itens.len();
        itens.sort_by(|a, b| b.3.d.total_cmp(&a.3.d));
        let mut vistos = HashSet::new();
        let escolhidos: Vec<_> = itens
            .into_iter()
            .filter(|(_, _, sp, _)| vistos.insert(sp.id))
            .take(n)
            .collect();
        let niveis: Vec<f64> = escolhidos
            .iter()
            .map(|(f, ..)| js::mais(f.get("level")))
            .collect();
        let (forca, mult) = self.modelo.penalidade(&niveis, boss.team_level);
        let escolhidos: Vec<_> = escolhidos
            .into_iter()
            .map(|(f, mon, sp, _)| {
                let r = self.modelo.avaliar(&mon, &foe, mult);
                (f, sp, r)
            })
            .collect();
        let soma = escolhidos
            .iter()
            .take(TAMANHO_EQUIPE)
            .fold(0.0, |s, (_, _, r)| s + r.d);
        let ou_nulo =
            |f: &Value, c: &str| js::nulo_ou(f.get(c), None).cloned().unwrap_or(Value::Null);
        let equipe: Vec<Value> = escolhidos
            .iter()
            .map(|(f, sp, r)| {
                let nivel = js::mais(f.get("level"));
                let nome = f.get("nome").filter(|x| js::verdadeiro(Some(x))).cloned().unwrap_or_else(|| Value::from(sp.nome.clone()));
                json!({
                    "id": ou_nulo(f, "id"), "speciesId": sp.id, "nome": nome, "tipos": nomes_tipos(&sp.tipos),
                    "sprite": self.sprite(Some(sp), js::verdadeiro(f.get("shiny"))), "nivel": js::json_num(nivel),
                    "nota": ou_nulo(f, "nota"), "quality": ou_nulo(f, "quality"), "potencia": ou_nulo(f, "potencia"),
                    "ivTotal": ou_nulo(f, "ivTotal"), "D": js::json_num(js::round(r.d)),
                    "tDie": js::json_num(js::round(r.tempo_ate_cair)), "bate": eficacia(r.bate),
                    "apanha": eficacia(r.apanha), "imune": r.imune,
                    "faltam": js::json_num((boss.team_level - nivel).max(0.0)),
                })
            })
            .collect();
        Some(json!({
            "boss": {
                "key": boss.key, "nome": boss.nome, "teamLevel": js::json_num(boss.team_level),
                "tipos": nomes_tipos(&foe.tipos), "hp": js::json_num(foe.hp),
                "sprite": self.sprite(Some(sp_boss), false), "fonte": boss.fonte,
            },
            "forca": js::json_num(forca), "mult": js::json_num(mult), "soma": js::json_num(soma),
            "cobertura": js::json_num(soma / foe.hp), "equipe": equipe, "avaliados": avaliados,
        }))
    }
}

fn nome_perfil(p: Perfil) -> &'static str {
    match p {
        Perfil::Ideal => "ideal",
        Perfil::Realista => "realista",
    }
}

fn eficacia(e: crate::combate::duelo::Eficacia) -> Value {
    json!({ "mult": js::json_num(e.mult), "tipo": e.tipo.map(|t| t.to_string()) })
}

/// `proximoBoss`: sem vitórias, o boss mais alto que o nível de treinador alcança; senão o
/// primeiro não vencido que ele alcança.
pub fn proximo_boss(lista: &[BossLista], tr_lv: f64, vencidos: &HashSet<String>) -> Option<String> {
    let tem_tr = tr_lv != 0.0 && !tr_lv.is_nan();
    let pendentes: Vec<&BossLista> = lista
        .iter()
        .filter(|b| !vencidos.contains(&b.key))
        .collect();
    let mut alvo = None;
    if vencidos.is_empty() && tem_tr {
        let ok: Vec<&BossLista> = lista.iter().filter(|b| b.min_tr <= tr_lv).collect();
        if let Some(topo) = ok.last().map(|b| b.min_tr) {
            alvo = ok.into_iter().find(|b| b.min_tr == topo);
        }
    }
    alvo.or_else(|| {
        pendentes
            .iter()
            .copied()
            .find(|b| !tem_tr || b.min_tr <= tr_lv)
    })
    .or_else(|| pendentes.first().copied())
    .or_else(|| lista.first())
    .map(|b| b.key.clone())
}

/// Estado de equipes: lista viva de bosses e, por conta, nível de treinador, vencidos e
/// coleção (o `equipes-estado.json` do legado).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EstadoEquipes {
    pub bosses: Vec<Value>,
    pub bosses_em: i64,
    /// Na ordem em que as contas apareceram (a ordem das chaves do objeto no JS).
    pub contas: Vec<(String, Value)>,
}

/// Contas no estado de equipes, no máximo (cada uma guarda até 6000 fichas).
const MAX_CONTAS: usize = 200;

const CAMPOS_FICHA: [&str; 19] = [
    "id",
    "speciesId",
    "looktype",
    "nome",
    "level",
    "quality",
    "potencia",
    "ivs",
    "ivTotal",
    "nota",
    "stats",
    "maxHp",
    "poder",
    "tipos",
    "shiny",
    "slot",
    "tmElemental",
    "tmAoe",
    "refinoTotal",
];

/// Objeto JSON lido mantendo a ordem das chaves (o `Map` do `serde_json` ordena).
#[derive(Default)]
struct Ordenado(Vec<(String, Value)>);

impl<'de> serde::Deserialize<'de> for Ordenado {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct Visitante;
        impl<'de> serde::de::Visitor<'de> for Visitante {
            type Value = Ordenado;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("um objeto")
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut m: A,
            ) -> Result<Ordenado, A::Error> {
                let mut v = Vec::new();
                while let Some(par) = m.next_entry::<String, Value>()? {
                    v.push(par);
                }
                Ok(Ordenado(v))
            }
            fn visit_unit<E>(self) -> Result<Ordenado, E> {
                Ok(Ordenado::default())
            }
        }
        d.deserialize_any(Visitante)
    }
}

/// Lê o `equipes-estado.json` mantendo a ordem das contas.
impl<'de> serde::Deserialize<'de> for EstadoEquipes {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        struct Cru {
            #[serde(default)]
            bosses: Option<Vec<Value>>,
            #[serde(default, rename = "bossesEm")]
            bosses_em: Option<f64>,
            #[serde(default)]
            contas: Ordenado,
        }
        let c = Cru::deserialize(d)?;
        Ok(EstadoEquipes {
            bosses: c.bosses.unwrap_or_default(),
            bosses_em: c.bosses_em.map_or(0, |x| x as i64),
            contas: c.contas.0,
        })
    }
}

impl EstadoEquipes {
    /// Lê a partir de um `Value` já montado. A ordem das contas vira alfabética (o `Map` do
    /// `serde_json` ordena as chaves); para manter a do arquivo, desserialize do texto.
    pub fn de_json(v: &Value) -> Self {
        EstadoEquipes {
            bosses: v
                .get("bosses")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default(),
            bosses_em: v
                .get("bossesEm")
                .and_then(Value::as_f64)
                .map_or(0, |x| x as i64),
            contas: v
                .get("contas")
                .and_then(Value::as_object)
                .map(|m| m.iter().map(|(k, c)| (k.clone(), c.clone())).collect())
                .unwrap_or_default(),
        }
    }

    /// `EST.conta(nick)`: sem nick, a primeira conta.
    pub fn conta(&self, nick: Option<&str>) -> Option<&Value> {
        let k = nick.unwrap_or_default().to_lowercase();
        match self.contas.iter().find(|(n, _)| *n == k) {
            Some((_, c)) => Some(c),
            None if k.is_empty() => self.contas.first().map(|(_, c)| c),
            None => None,
        }
    }

    /// `EST.ingest(corpo)`. Devolve o nick e o resumo `{bosses, pokemons}`.
    pub fn ingerir(&mut self, b: &Value, agora: i64) -> Result<(String, Value), &'static str> {
        let nick: String = js::texto(b.get("nick").filter(|x| js::verdadeiro(Some(x))))
            .to_lowercase()
            .chars()
            .take(40)
            .collect();
        let nick = if b.get("nick").is_some_and(|x| js::verdadeiro(Some(x))) {
            nick
        } else {
            String::new()
        };
        if nick.is_empty() {
            return Err("nick obrigatório");
        }
        if let Some(bs) = b
            .get("bosses")
            .and_then(Value::as_array)
            .filter(|a| a.len() >= 3)
        {
            self.bosses = bs.iter().take(80).cloned().collect();
            self.bosses_em = agora;
        }
        let i = match self.contas.iter().position(|(n, _)| *n == nick) {
            Some(i) => i,
            None if self.contas.len() >= MAX_CONTAS => return Err("limite de contas atingido"),
            None => {
                self.contas.push((nick.clone(), Value::Object(Map::new())));
                self.contas.len() - 1
            }
        };
        let Some(Value::Object(c)) = self.contas.get_mut(i).map(|(_, c)| c) else {
            return Err("conta inválida");
        };
        let tr = js::mais(b.get("trLv"));
        if tr.is_finite() && tr > 0.0 {
            c.insert("trLv".into(), js::json_num(tr));
        }
        if let Some(v) = b.get("vencidos").and_then(Value::as_array) {
            let lista: Vec<Value> = v
                .iter()
                .take(80)
                .map(|x| Value::from(js::texto(Some(x))))
                .collect();
            c.insert("vencidos".into(), Value::Array(lista));
        }
        if let Some(ps) = b.get("pokemons").and_then(Value::as_array) {
            let lista: Vec<Value> = ps
                .iter()
                .take(6000)
                .filter(|p| p.is_object() || p.is_array())
                .map(|p| {
                    let mut o = Map::new();
                    if let Value::Object(m) = p {
                        for k in CAMPOS_FICHA {
                            if let Some(x) = m.get(k) {
                                o.insert(k.into(), x.clone());
                            }
                        }
                    }
                    Value::Object(o)
                })
                .collect();
            c.insert("pokemons".into(), Value::Array(lista));
        }
        c.insert("em".into(), Value::from(agora));
        let pokemons = c
            .get("pokemons")
            .and_then(Value::as_array)
            .map_or(0, Vec::len);
        Ok((
            nick,
            json!({ "ok": true, "bosses": self.bosses.len(), "pokemons": pokemons }),
        ))
    }
}

/// Erro de uma rota de equipes: status HTTP e mensagem.
#[derive(Debug, Clone, PartialEq)]
pub struct ErroEquipes {
    pub status: u16,
    pub erro: &'static str,
}

fn vencidos_de(conta: Option<&Value>) -> HashSet<String> {
    conta
        .and_then(|c| c.get("vencidos"))
        .and_then(Value::as_array)
        .map(|v| v.iter().map(|x| norm(&js::texto(Some(x)))).collect())
        .unwrap_or_default()
}

fn tr_lv(conta: Option<&Value>) -> f64 {
    conta.map_or(f64::NAN, |c| js::mais(c.get("trLv")))
}

fn achar<'a>(lista: &'a [BossLista], k: &str) -> Option<&'a BossLista> {
    let k = norm(k);
    lista
        .iter()
        .find(|b| b.key == k)
        .or_else(|| lista.iter().find(|b| norm(&b.nome) == k))
}

/// `GET /api/equipes/bosses`.
pub fn resposta_bosses(motor: &MotorEquipes, estado: &EstadoEquipes, nick: Option<&str>) -> Value {
    let c = estado.conta(nick);
    let lista = motor.lista_bosses(&estado.bosses);
    let venc = vencidos_de(c);
    let tr = tr_lv(c);
    let prox = proximo_boss(&lista, tr, &venc);
    let tem_tr = tr != 0.0 && !tr.is_nan();
    let bosses: Vec<Value> = lista
        .iter()
        .map(|b| {
            json!({
                "key": b.key, "nome": b.nome, "teamLevel": js::json_num(b.team_level), "minTr": js::json_num(b.min_tr),
                "tipos": b.tipos, "regiao": b.regiao, "trinca": b.trinca, "pos": b.pos,
                "sprite": motor.sprite(motor.dex.por_nome(&b.nome), false), "feito": venc.contains(&b.key),
                "alcanca": !tem_tr || tr >= b.min_tr, "fonte": b.fonte,
            })
        })
        .collect();
    let conta = c.map(|c| {
        json!({
            "trLv": js::nulo_ou(c.get("trLv"), None),
            "pokemons": c.get("pokemons").and_then(Value::as_array).map_or(0, Vec::len),
            "em": c.get("em"),
        })
    });
    json!({
        "contas": estado.contas.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(),
        "fonte": if estado.bosses.is_empty() { "wiki" } else { "jogo" },
        "atualizadoEm": if estado.bosses_em == 0 { Value::Null } else { Value::from(estado.bosses_em) },
        "conta": conta, "proximo": prox, "bosses": bosses,
    })
}

/// `GET /api/equipes/plano`.
pub fn resposta_plano(
    motor: &MotorEquipes,
    estado: &EstadoEquipes,
    nick: Option<&str>,
    boss: Option<&str>,
    perfil: Option<&str>,
) -> Result<Value, ErroEquipes> {
    let c = estado.conta(nick);
    let lista = motor.lista_bosses(&estado.bosses);
    let venc = vencidos_de(c);
    let tr = tr_lv(c);
    let alvo = match boss.filter(|b| !b.is_empty()) {
        Some(b) => Some(b.to_string()),
        None => proximo_boss(&lista, tr, &venc),
    };
    let Some(boss) = alvo.and_then(|k| achar(&lista, &k)) else {
        return Err(ErroEquipes {
            status: 404,
            erro: "boss não encontrado",
        });
    };
    let perfil = if perfil == Some("ideal") {
        Perfil::Ideal
    } else {
        Perfil::Realista
    };
    let tr_conta = c
        .and_then(|c| js::nulo_ou(c.get("trLv"), None))
        .map(|v| js::mais(Some(v)));
    let tr_filtro = if perfil == Perfil::Realista {
        tr_conta
    } else {
        None
    };
    let Some(mut ideal) = motor.equipe_ideal(boss, perfil, 6, tr_filtro) else {
        return Err(ErroEquipes {
            status: 404,
            erro: "espécie do boss não está na Pokédex",
        });
    };
    let pokes: &[Value] = c
        .and_then(|c| c.get("pokemons"))
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice);
    let mut minha = if pokes.is_empty() {
        None
    } else {
        motor.equipe_colecao(boss, pokes, 6)
    };
    let soma_ideal = ideal.get("soma").and_then(Value::as_f64).unwrap_or(0.0);
    if let Some(Value::Object(m)) = minha.as_mut() {
        if soma_ideal > 0.0 {
            let soma = m.get("soma").and_then(Value::as_f64).unwrap_or(0.0);
            m.insert("poderVsIdeal".into(), js::json_num(soma / soma_ideal));
        }
    }
    // Maior nível e quantidade de cada espécie que a conta tem.
    let mut donos: HashMap<i64, (f64, u64)> = HashMap::new();
    for f in pokes {
        if let Some(sp) = combatente::especie_da_ficha(&motor.dex, f) {
            let nivel = js::mais(f.get("level"));
            match donos.get_mut(&sp.id) {
                Some(x) if nivel.partial_cmp(&x.0) != Some(std::cmp::Ordering::Greater) => x.1 += 1,
                outro => {
                    let qtd = outro.map_or(0, |x| x.1) + 1;
                    donos.insert(sp.id, (nivel, qtd));
                }
            }
        }
    }
    if let Some(Value::Array(eq)) = ideal.get_mut("equipe") {
        for m in eq.iter_mut() {
            let d = m
                .get("speciesId")
                .and_then(Value::as_i64)
                .and_then(|id| donos.get(&id));
            if let Value::Object(o) = m {
                o.insert("possui".into(), Value::Bool(d.is_some()));
                o.insert(
                    "nivelQueTenho".into(),
                    d.map_or(Value::Null, |x| js::json_num(x.0)),
                );
            }
        }
    }
    let mut b = ideal.get("boss").cloned().unwrap_or(Value::Null);
    if let Value::Object(o) = &mut b {
        let tipos: Vec<Tipo> = o
            .get("tipos")
            .and_then(Value::as_array)
            .map(|t| t.iter().filter_map(|x| x.as_str()?.parse().ok()).collect())
            .unwrap_or_default();
        let k = motor.modelo.constantes();
        let resumo: Vec<Value> = Tipo::TODOS
            .iter()
            .map(|t| (t, k.efetividade_hunt(*t, &tipos)))
            .filter(|(_, m)| *m != 1.0)
            .map(|(t, m)| json!({ "t": t.to_string(), "mult": js::json_num(m) }))
            .collect();
        o.insert("minTr".into(), js::json_num(boss.min_tr));
        o.insert("regiao".into(), json!(boss.regiao));
        o.insert("trinca".into(), json!(boss.trinca));
        o.insert("resumoTipos".into(), Value::Array(resumo));
    }
    let conta =
        c.map(|c| json!({ "trLv": js::nulo_ou(c.get("trLv"), None), "pokemons": pokes.len() }));
    Ok(json!({
        "boss": b, "perfil": nome_perfil(perfil), "ideal": ideal, "minha": minha,
        "conta": conta, "fonte": boss.fonte, "nota": NOTA_PLANO,
    }))
}

/// Laboratório: a equipe montada à mão (ids da coleção da conta, até 6) contra um boss, no
/// mesmo modelo do plano. Espécie repetida conta uma vez, como no plano.
pub fn resposta_lab(
    motor: &MotorEquipes,
    estado: &EstadoEquipes,
    nick: Option<&str>,
    boss: &str,
    ids: &[&str],
) -> Result<Value, ErroEquipes> {
    let lista = motor.lista_bosses(&estado.bosses);
    let Some(boss) = achar(&lista, boss) else {
        return Err(ErroEquipes {
            status: 404,
            erro: "boss não encontrado",
        });
    };
    let id_de = |f: &Value| match f.get("id") {
        Some(Value::String(t)) => t.clone(),
        Some(v) => v.to_string(),
        None => String::new(),
    };
    let escolhidos: Vec<Value> = estado
        .conta(nick)
        .and_then(|c| c.get("pokemons"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|f| ids.contains(&id_de(f).as_str()))
        .take(6)
        .cloned()
        .collect();
    motor
        .equipe_colecao(boss, &escolhidos, 6)
        .ok_or(ErroEquipes {
            status: 404,
            erro: "espécie do boss não está na Pokédex",
        })
}

/// Divergência entre dois modelos para um boss: cobertura (soma ÷ HP) da equipe teórica
/// realista em cada um e quantas espécies as duas equipes têm em comum.
#[derive(Debug, Clone, PartialEq)]
pub struct Divergencia {
    pub boss: String,
    pub cobertura_a: f64,
    pub cobertura_b: f64,
    pub coincidentes: usize,
}

/// Compara a equipe teórica de cada boss em dois motores (ex.: legado × jogo).
pub fn comparar_modelos(
    a: &MotorEquipes,
    b: &MotorEquipes,
    lista: &[BossLista],
) -> Vec<Divergencia> {
    let especies = |v: &Value| -> HashSet<i64> {
        v.get("equipe")
            .and_then(Value::as_array)
            .map(|e| {
                e.iter()
                    .take(TAMANHO_EQUIPE)
                    .filter_map(|m| m.get("speciesId")?.as_i64())
                    .collect()
            })
            .unwrap_or_default()
    };
    let cobertura = |v: &Value| {
        v.get("cobertura")
            .and_then(Value::as_f64)
            .unwrap_or(f64::NAN)
    };
    lista
        .iter()
        .filter_map(|boss| {
            let ea = a.equipe_ideal(boss, Perfil::Realista, 5, None)?;
            let eb = b.equipe_ideal(boss, Perfil::Realista, 5, None)?;
            Some(Divergencia {
                boss: boss.key.clone(),
                cobertura_a: cobertura(&ea),
                cobertura_b: cobertura(&eb),
                coincidentes: especies(&ea).intersection(&especies(&eb)).count(),
            })
        })
        .collect()
}

/// Bioma do tipo principal (`docs/design/telas.md`, Biomas). Templo, montanha nevada e pico
/// ainda não têm imagem: caem em montanha e cânion.
pub fn bioma_do_tipo(t: crate::combate::tipos::Tipo) -> &'static str {
    use crate::combate::tipos::Tipo as T;
    match t {
        T::Water => "riacho-lago",
        T::Normal | T::Flying => "campos-penhascos",
        T::Grass | T::Bug => "floresta",
        T::Ghost | T::Dark => "floresta-sombria",
        T::Psychic | T::Fairy => "clareira-mistica",
        T::Fire => "vulcao",
        T::Electric => "planicie-tempestuosa",
        T::Poison => "pantano",
        _ => "montanha-canion",
    }
}

#[cfg(test)]
mod testes_bioma {
    use super::*;
    use crate::combate::tipos::Tipo;

    #[test]
    fn bioma_pelo_tipo() {
        assert_eq!(bioma_do_tipo(Tipo::Water), "riacho-lago");
        assert_eq!(bioma_do_tipo(Tipo::Bug), "floresta");
        assert_eq!(bioma_do_tipo(Tipo::Steel), "montanha-canion");
    }
}
