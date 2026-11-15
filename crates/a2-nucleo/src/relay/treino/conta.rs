//! O aprendizado de uma conta: a cada luta liberada (o jogo dá uma a cada 5 min), monta um
//! time do meta da faixa como pokémon teste (lado A) e luta com o time da conta (lado B) numa
//! formação candidata. Guarda a nota de cada formação; quando uma vence a atual com folga em
//! lutas repetidas, aplica no PvP (`pvp.time.salvar`).

use std::collections::{BTreeMap, HashMap};

use serde_json::{json, Value};

use super::comum::Comum;

/// Casa de cada pokémon do time: id → casa.
pub type Formacao = BTreeMap<i64, i64>;

/// Sem resposta do treino em 1 min, libera de novo.
const ESPERA_MS: i64 = 60_000;
/// Espera padrão entre lutas (o jogo manda o `cooldownMs`).
const COOLDOWN_MS: i64 = 5 * 60_000;
/// Adversários em comum (lutados pela candidata e pela atual) antes de comparar.
const MIN_COMUNS: usize = 2;
/// Lutas mínimas da candidata nesses adversários.
const MIN_LUTAS: u32 = 3;
/// Folga média de nota, nos adversários em comum, para trocar a formação do PvP.
const FOLGA: f64 = 0.15;
/// Partidas mínimas no meta para montar um adversário.
const MIN_PARTIDAS_META: u64 = 5;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Nota {
    soma: f64,
    lutas: u32,
}

impl Nota {
    fn media(&self) -> f64 {
        if self.lutas == 0 {
            0.0
        } else {
            self.soma / f64::from(self.lutas)
        }
    }
}

#[derive(Default)]
pub struct TreinoConta {
    time: Vec<i64>,
    atual: Formacao,
    especie_de: HashMap<i64, i64>,
    faixa: Option<i64>,
    meta: Option<Value>,
    /// Metas das faixas logo acima (adversários mais fortes que a conta ainda vai pegar).
    pub(super) acima: Vec<Value>,
    /// Times reais do topo (fichas sem nível), ver `ranking.rs`.
    pub(super) reais: Vec<Vec<Value>>,
    /// Nível de cada pokémon da conta (o adversário real luta no nível médio do time).
    nivel_de: HashMap<i64, i64>,
    livre_em: i64,
    esperando: Option<(i64, Formacao, String)>,
    /// Formação → nota contra cada adversário (chave: espécies do time adversário).
    notas: BTreeMap<String, (Formacao, BTreeMap<String, Nota>)>,
    rodada: u64,
    aplicar: Option<Formacao>,
    mudou: bool,
}

fn chave(f: &Formacao) -> String {
    f.iter()
        .map(|(id, c)| format!("{id}:{c}"))
        .collect::<Vec<_>>()
        .join(",")
}

/// Chave de um adversário de treino: as espécies do time, em ordem.
fn chave_adv(a: &[Value]) -> String {
    let mut v: Vec<i64> = a
        .iter()
        .filter_map(|x| x.pointer("/teste/speciesId")?.as_i64())
        .collect();
    v.sort_unstable();
    v.iter().map(i64::to_string).collect::<Vec<_>>().join("-")
}

/// Nota de uma luta para o lado B (o dono): 1 por vitória, mais ou menos 0,5 pela diferença
/// de dano.
pub fn nota_da_luta(r: &Value) -> Option<f64> {
    let venceu_b = !r.get("venceuA")?.as_bool()?;
    let dano = |lado: i64| -> f64 {
        r.get("placar")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            // O jogo manda `lado` (1 = adversário, 2 = a conta); versões antigas, `id`.
            .find(|l| {
                l.get("lado")
                    .or_else(|| l.get("id"))
                    .and_then(Value::as_i64)
                    == Some(lado)
            })
            .and_then(|l| l.get("dano")?.as_f64())
            .unwrap_or(0.0)
    };
    let (a, b) = (dano(1), dano(2));
    let margem = if a + b > 0.0 { (b - a) / (a + b) } else { 0.0 };
    Some(f64::from(u8::from(venceu_b)) + 0.5 * margem)
}

/// Casas candidatas para um alcance: as do meta (melhor taxa primeiro) e as padrão.
fn casas_para(meta: &Value, alcance: i64) -> Vec<i64> {
    let do_meta = meta
        .get("alcances")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|a| a.get("alcance").and_then(Value::as_i64) == Some(alcance))
        .and_then(|a| a.get("casas")?.as_array())
        .into_iter()
        .flatten()
        .filter_map(|c| c.get(0)?.as_i64());
    // Tabuleiro do jogo: fileira 0 (casas 0–5, a da frente), 1 (6–13), 2 (14–21), 3 (22–25),
    // 4 (26–29). Corpo a corpo na frente; quanto maior o alcance, mais atrás.
    let padrao: &[i64] = match alcance {
        i64::MIN..=1 => &[2, 3, 1, 4, 0, 5, 9, 10, 8, 11],
        2 => &[9, 10, 8, 11, 7, 12, 17, 18, 16, 19],
        3 => &[17, 18, 16, 19, 15, 20, 23, 24, 22, 25],
        _ => &[23, 24, 22, 25, 27, 28, 26, 29, 17, 18],
    };
    let mut v: Vec<i64> = Vec::new();
    for c in do_meta.chain(padrao.iter().copied()) {
        if !v.contains(&c) {
            v.push(c);
        }
    }
    v
}

/// O `i`-ésimo (revezando entre os 3 mais comuns) time do meta, como pokémon teste.
fn adversario_do_meta(meta: &Value, i: u64) -> Option<Vec<Value>> {
    let times = meta.get("times")?.as_array()?;
    let t = times.get((i % times.len().clamp(1, 3) as u64) as usize)?;
    let especies = meta.get("especies")?.as_array()?;
    let mut usadas = Vec::new();
    let mut a = Vec::new();
    for sp in t
        .get("especies")?
        .as_array()?
        .iter()
        .filter_map(Value::as_i64)
    {
        let e = especies
            .iter()
            .find(|e| e.get("especie").and_then(Value::as_i64) == Some(sp));
        let nivel = e
            .and_then(|e| e.get("nivel_medio")?.as_i64())
            .unwrap_or(100)
            .clamp(1, 1000);
        let alcance = e.and_then(|e| e.get("alcance")?.as_i64()).unwrap_or(1);
        let preferida = e
            .and_then(|e| e.get("casas")?.as_array()?.first()?.get(0)?.as_i64())
            .into_iter();
        let casa = preferida
            .chain(casas_para(meta, alcance))
            .chain(0..30)
            .find(|c| !usadas.contains(c))?;
        usadas.push(casa);
        a.push(json!({"casa": casa, "teste": {
            "speciesId": sp, "level": nivel,
            "ivs": {"hp": 31, "atk": 31, "def": 31, "spAtk": 31, "spDef": 31, "speed": 31},
            "qualidade": 1.5, "potencia": 1, "shiny": false, "tmFisico": false, "tmElemental": null,
        }}));
    }
    (!a.is_empty()).then_some(a)
}

impl TreinoConta {
    pub fn nova() -> Self {
        Self {
            livre_em: i64::MIN / 2,
            ..Self::default()
        }
    }

    /// Faixa de pontos do PvP da conta (de 100 em 100), quando já veio.
    pub fn faixa(&self) -> Option<i64> {
        self.faixa
    }

    pub fn definir_meta(&mut self, meta: &Value) {
        self.meta = Some(meta.clone());
    }

    /// Metas das faixas acima, da mais perto para a mais longe.
    pub fn definir_acima(&mut self, metas: Vec<Value>) {
        self.acima = metas;
    }

    /// Algo mudou desde o último [`TreinoConta::notas_json`]?
    pub fn mudou(&mut self) -> bool {
        std::mem::take(&mut self.mudou)
    }

    fn alcance(&self, id: i64) -> i64 {
        let especie = self.especie_de.get(&id).copied();
        self.meta
            .as_ref()
            .and_then(|m| m.get("especies")?.as_array())
            .into_iter()
            .flatten()
            .find(|e| e.get("especie").and_then(Value::as_i64) == especie)
            .and_then(|e| e.get("alcance")?.as_i64())
            .unwrap_or(1)
    }

    /// Time adversário como pokémon teste: reveza entre o meta da faixa da conta e o das
    /// faixas acima (as que têm amostra) e, em cada um, entre os 3 times mais comuns.
    fn adversario(&self) -> Option<Vec<Value>> {
        let metas: Vec<&Value> = self
            .meta
            .iter()
            .chain(&self.acima)
            .filter(|m| m.get("partidas").and_then(Value::as_u64).unwrap_or(0) >= MIN_PARTIDAS_META)
            .collect();
        // Revezamento: cada meta (sua faixa e as de cima) e cada time real do topo.
        let n = (metas.len() + self.reais.len()) as u64;
        if n == 0 {
            return None;
        }
        let i = (self.rodada % n) as usize;
        match metas.get(i) {
            Some(m) => adversario_do_meta(m, self.rodada / n),
            None => self.adversario_real(self.reais.get(i - metas.len())?),
        }
    }

    /// Nível médio do time da conta (o time real do topo luta nele: mede composição e casas,
    /// não o nível de quem joga há mais tempo).
    fn nivel_do_time(&self) -> i64 {
        let ns: Vec<i64> = self
            .time
            .iter()
            .filter_map(|id| self.nivel_de.get(id))
            .copied()
            .collect();
        if ns.is_empty() {
            100
        } else {
            (ns.iter().sum::<i64>() / ns.len() as i64).clamp(1, 10_000)
        }
    }

    fn adversario_real(&self, fichas: &[Value]) -> Option<Vec<Value>> {
        let nivel = self.nivel_do_time();
        let meta = self.meta.as_ref();
        let mut usadas = Vec::new();
        let mut a = Vec::new();
        for f in fichas {
            let sp = f.get("speciesId")?.as_i64()?;
            let e = meta
                .and_then(|m| m.get("especies")?.as_array())
                .into_iter()
                .flatten()
                .find(|e| e.get("especie").and_then(Value::as_i64) == Some(sp));
            let alcance = e.and_then(|e| e.get("alcance")?.as_i64()).unwrap_or(1);
            let preferida = e
                .and_then(|e| e.get("casas")?.as_array()?.first()?.get(0)?.as_i64())
                .into_iter();
            let casas = meta
                .map(|m| casas_para(m, alcance))
                .unwrap_or_else(|| casas_para(&Value::Null, alcance));
            let casa = preferida
                .chain(casas)
                .chain(0..30)
                .find(|c| !usadas.contains(c))?;
            usadas.push(casa);
            let mut teste = f.clone();
            if let Value::Object(o) = &mut teste {
                o.insert("level".into(), json!(nivel));
                o.insert("tmFisico".into(), json!(false));
            }
            a.push(json!({"casa": casa, "teste": teste}));
        }
        (!a.is_empty()).then_some(a)
    }

    /// Notas de uma formação por adversário (vazio se nunca lutou).
    fn notas_de(&self, f: &Formacao) -> Option<&BTreeMap<String, Nota>> {
        self.notas.get(&chave(f)).map(|x| &x.1)
    }

    /// A formação que mais supera a atual, comparando só nos adversários que as duas
    /// enfrentaram: (formação, ganho médio de nota, adversários em comum).
    fn melhor_pareada(&self) -> Option<(Formacao, f64, usize)> {
        let atual = self.notas_de(&self.atual)?;
        self.notas
            .values()
            .filter(|(f, _)| *f != self.atual)
            .filter_map(|(f, por)| {
                let comuns: Vec<(f64, u32)> = por
                    .iter()
                    .filter_map(|(adv, n)| Some((n.media() - atual.get(adv)?.media(), n.lutas)))
                    .collect();
                let lutas: u32 = comuns.iter().map(|x| x.1).sum();
                let difs: Vec<f64> = comuns.iter().map(|x| x.0).collect();
                (difs.len() >= MIN_COMUNS && lutas >= MIN_LUTAS).then(|| {
                    (
                        f.clone(),
                        difs.iter().sum::<f64>() / difs.len() as f64,
                        difs.len(),
                    )
                })
            })
            .max_by(|a, b| a.1.total_cmp(&b.1))
    }

    /// Próxima formação a testar contra o adversário `adv`: a atual, se ainda não lutou contra
    /// ele (é a régua); depois a melhor candidata, se ainda não lutou contra ele; senão uma
    /// variação da melhor (um pokémon mudado de casa, as casas que as contas já aprovaram
    /// para a espécie primeiro).
    fn candidata(&self, comum: &Comum, adv: &str) -> Formacao {
        let lutou = |f: &Formacao| self.notas_de(f).is_some_and(|m| m.contains_key(adv));
        if !lutou(&self.atual) {
            return self.atual.clone();
        }
        let melhor = self.melhor_pareada().filter(|x| x.1 > 0.0).map(|x| x.0);
        if let Some(m) = melhor.as_ref().filter(|m| !lutou(m)) {
            return m.clone();
        }
        let base = melhor.unwrap_or_else(|| self.atual.clone());
        if self.time.is_empty() {
            return base;
        }
        // Muda um pokémon (revezando) para uma casa livre do alcance dele.
        let i = (self.rodada as usize) % self.time.len();
        let id = self.time[i];
        let ocupadas: Vec<i64> = base
            .iter()
            .filter(|(k, _)| **k != id)
            .map(|(_, c)| *c)
            .collect();
        let opcoes: Vec<i64> =
            casas_para(self.meta.as_ref().unwrap_or(&Value::Null), self.alcance(id))
                .into_iter()
                .filter(|c| !ocupadas.contains(c) && base.get(&id) != Some(c))
                .collect();
        let opcoes = comum.ordenar(self.especie_de.get(&id).copied(), opcoes);
        let mut nova = base.clone();
        if !opcoes.is_empty() {
            let pos = (self.rodada / self.time.len() as u64) as usize % opcoes.len().min(4);
            nova.insert(id, opcoes[pos]);
        }
        nova
    }

    fn registrar(&mut self, f: Formacao, adv: String, nota: f64) {
        let e = self
            .notas
            .entry(chave(&f))
            .or_insert_with(|| (f, BTreeMap::new()));
        let n = e.1.entry(adv).or_default();
        n.soma += nota;
        n.lutas += 1;
        self.mudou = true;
        // Uma candidata vence a atual com folga nos mesmos adversários? Aplica no PvP.
        if let Some((f, ganho, _)) = self.melhor_pareada() {
            if ganho >= FOLGA {
                self.aplicar = Some(f);
            }
        }
    }
}

impl TreinoConta {
    /// Aplica uma mensagem da conta. Numa luta de treino terminada, devolve as (espécie, casa)
    /// do time e a nota, para o aprendizado comum.
    pub fn receber(&mut self, msg: &Value, agora: i64) -> Option<(Vec<(i64, i64)>, f64)> {
        match msg.get("t").and_then(Value::as_str) {
            Some("welcome" | "estado") => {
                let e = msg.get("estado");
                for p in ["pokemons", "pkMud"]
                    .iter()
                    .filter_map(|k| e?.get(*k)?.as_array())
                    .flatten()
                {
                    if let (Some(id), Some(sp)) = (
                        p.get("id").and_then(Value::as_i64),
                        p.get("speciesId").and_then(Value::as_i64),
                    ) {
                        self.especie_de.insert(id, sp);
                        if let Some(l) = p.get("level").and_then(Value::as_i64) {
                            self.nivel_de.insert(id, l);
                        }
                    }
                }
            }
            Some("pvp") => {
                if let Some(t) = msg.get("time").and_then(Value::as_array) {
                    self.time = t.iter().filter_map(Value::as_i64).collect();
                }
                if let Some(c) = msg.get("casas").and_then(Value::as_object) {
                    self.atual = c
                        .iter()
                        .filter_map(|(k, v)| Some((k.parse().ok()?, v.as_i64()?)))
                        .collect();
                }
                if let Some(p) = msg.pointer("/rank/pontos").and_then(Value::as_i64) {
                    self.faixa = Some(p.div_euclid(100) * 100);
                }
            }
            Some("treino") => {
                if let Some(cd) = msg.get("cooldownMs").and_then(Value::as_i64) {
                    self.livre_em = agora + cd;
                }
                if let Some(r) = msg.get("resultado") {
                    if let Some(cd) = r.get("cooldownMs").and_then(Value::as_i64) {
                        self.livre_em = agora + cd;
                    }
                    if let (Some((_, f, adv)), Some(n)) = (self.esperando.take(), nota_da_luta(r)) {
                        let pares = f
                            .iter()
                            .filter_map(|(id, c)| Some((*self.especie_de.get(id)?, *c)))
                            .collect();
                        self.registrar(f, adv, n);
                        return Some((pares, n));
                    }
                }
            }
            _ => {}
        }
        None
    }

    pub fn pedir(&mut self, agora: i64, comum: &Comum) -> Option<Value> {
        if self.time.is_empty() || self.atual.len() != self.time.len() {
            return None;
        }
        if let Some(f) = self.aplicar.take() {
            self.atual = f.clone();
            return Some(json!({"t": "pvp.time.salvar", "casas": f, "pokemonIds": self.time}));
        }
        if self
            .esperando
            .as_ref()
            .is_some_and(|(t, _, _)| agora - t < ESPERA_MS)
            || agora < self.livre_em
        {
            return None;
        }
        let a = self.adversario()?;
        let adv = chave_adv(&a);
        let f = self.candidata(comum, &adv);
        self.rodada += 1;
        self.esperando = Some((agora, f.clone(), adv));
        self.livre_em = agora + COOLDOWN_MS;
        let b: Vec<Value> = f
            .iter()
            .map(|(id, c)| json!({"meu": id, "casa": c}))
            .collect();
        Some(json!({"t": "treino.lutar", "a": a, "b": b}))
    }

    pub fn notas_json(&self) -> Vec<Value> {
        self.notas
            .values()
            .map(|(f, por)| {
                let adv: serde_json::Map<String, Value> = por
                    .iter()
                    .map(|(k, n)| (k.clone(), json!([n.soma, n.lutas])))
                    .collect();
                json!({"f": f, "adv": adv})
            })
            .collect()
    }

    /// Notas salvas. As do formato antigo (sem adversário) ficam de fora: misturavam
    /// adversários fáceis e difíceis e não servem para comparar.
    pub fn restaurar(&mut self, notas: &[Value]) {
        for x in notas {
            let f: Formacao = x
                .get("f")
                .and_then(Value::as_object)
                .into_iter()
                .flatten()
                .filter_map(|(k, c)| Some((k.parse().ok()?, c.as_i64()?)))
                .collect();
            let por: BTreeMap<String, Nota> = x
                .get("adv")
                .and_then(Value::as_object)
                .into_iter()
                .flatten()
                .filter_map(|(k, v)| {
                    Some((
                        k.clone(),
                        Nota {
                            soma: v.get(0)?.as_f64()?,
                            lutas: v.get(1)?.as_u64()? as u32,
                        },
                    ))
                })
                .collect();
            if !f.is_empty() && !por.is_empty() {
                self.notas.insert(chave(&f), (f, por));
            }
        }
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn nota_da_luta_soma_vitoria_e_margem() {
        let r = json!({"venceuA": false, "placar": [{"lado": 1, "dano": 100}, {"lado": 2, "dano": 300}]});
        assert_eq!(nota_da_luta(&r), Some(1.25));
        let perdeu = json!({"venceuA": true, "placar": [{"lado": 1, "dano": 300}, {"lado": 2, "dano": 100}]});
        assert_eq!(nota_da_luta(&perdeu), Some(-0.25));
    }
}
