//! Aprende a posição do time de PvP na Área de Treinamento: a cada luta liberada (o jogo dá
//! uma a cada 5 min), monta um time do meta da faixa como pokémon teste (lado A) e luta com o
//! time do dono (lado B) numa formação candidata. Guarda a nota de cada formação; quando uma
//! vence a atual com folga em lutas repetidas, aplica no PvP (`pvp.time.salvar`).
//!
//! Só roda com o app como relay oficial e o aprendizado ligado. O meta vem do servidor
//! (`/api/pvp/meta`); as lutas viram eventos `treino.resultado` pelo coletor.

use std::collections::{BTreeMap, HashMap};

use serde_json::{json, Value};

use super::{Entrega, Fluxo};

/// Casa de cada pokémon do time: id → casa.
pub type Formacao = BTreeMap<i64, i64>;

/// Recarrega o meta a cada 30 min.
const META_MS: i64 = 30 * 60_000;
/// Sem resposta do treino em 1 min, libera de novo.
const ESPERA_MS: i64 = 60_000;
/// Espera padrão entre lutas (o jogo manda o `cooldownMs`).
const COOLDOWN_MS: i64 = 5 * 60_000;
/// Lutas mínimas de uma formação antes de comparar.
const MIN_LUTAS: u32 = 3;
/// Folga de nota para trocar a formação do PvP.
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

pub struct Treino {
    dono: String,
    ligado: bool,
    time: Vec<i64>,
    atual: Formacao,
    especie_de: HashMap<i64, i64>,
    faixa: Option<i64>,
    meta: Option<Value>,
    meta_em: Option<i64>,
    livre_em: i64,
    esperando: Option<(i64, Formacao)>,
    notas: BTreeMap<String, (Formacao, Nota)>,
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

/// Nota de uma luta para o lado B (o dono): 1 por vitória, mais ou menos 0,5 pela diferença
/// de dano.
pub fn nota_da_luta(r: &Value) -> Option<f64> {
    let venceu_b = !r.get("venceuA")?.as_bool()?;
    let dano = |lado: i64| -> f64 {
        r.get("placar")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .find(|l| l.get("id").and_then(Value::as_i64) == Some(lado))
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
    let padrao: &[i64] = match alcance {
        i64::MIN..=1 => &[2, 3, 1, 4, 5],
        2 => &[8, 7, 9, 6, 10],
        3 => &[17, 18, 16, 19, 20],
        _ => &[23, 22, 24, 21, 25],
    };
    let mut v: Vec<i64> = Vec::new();
    for c in do_meta.chain(padrao.iter().copied()) {
        if !v.contains(&c) {
            v.push(c);
        }
    }
    v
}

impl Treino {
    pub fn novo(dono: &str, ligado: bool) -> Self {
        Self {
            dono: dono.to_owned(),
            ligado,
            time: Vec::new(),
            atual: Formacao::new(),
            especie_de: HashMap::new(),
            faixa: None,
            meta: None,
            meta_em: None,
            livre_em: i64::MIN / 2,
            esperando: None,
            notas: BTreeMap::new(),
            rodada: 0,
            aplicar: None,
            mudou: false,
        }
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

    /// Time adversário do meta (reveza entre os 3 mais comuns) como pokémon teste.
    fn adversario(&self) -> Option<Vec<Value>> {
        let meta = self.meta.as_ref()?;
        if meta.get("partidas").and_then(Value::as_u64).unwrap_or(0) < MIN_PARTIDAS_META {
            return None;
        }
        let times = meta.get("times")?.as_array()?;
        let t = times.get((self.rodada % times.len().clamp(1, 3) as u64) as usize)?;
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
                .chain(1..=25)
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

    /// Próxima formação a testar: a atual até ter amostra; depois, a melhor com um pokémon
    /// mudado de casa (ou de novo a melhor, para confirmar, se ainda tem poucas lutas).
    fn candidata(&self) -> Formacao {
        let atual_k = chave(&self.atual);
        let atual_lutas = self.notas.get(&atual_k).map_or(0, |x| x.1.lutas);
        if atual_lutas < MIN_LUTAS {
            return self.atual.clone();
        }
        let melhor = self
            .notas
            .values()
            .max_by(|a, b| a.1.media().total_cmp(&b.1.media()))
            .map_or_else(|| self.atual.clone(), |x| x.0.clone());
        let melhor_lutas = self.notas.get(&chave(&melhor)).map_or(0, |x| x.1.lutas);
        if melhor_lutas < MIN_LUTAS || self.time.is_empty() {
            return melhor;
        }
        // Muda um pokémon (revezando) para a próxima casa livre do alcance dele.
        let i = (self.rodada as usize) % self.time.len();
        let id = self.time[i];
        let ocupadas: Vec<i64> = melhor
            .iter()
            .filter(|(k, _)| **k != id)
            .map(|(_, c)| *c)
            .collect();
        let opcoes: Vec<i64> = self
            .meta
            .as_ref()
            .map(|m| casas_para(m, self.alcance(id)))
            .unwrap_or_default()
            .into_iter()
            .filter(|c| !ocupadas.contains(c))
            .collect();
        let mut nova = melhor.clone();
        if !opcoes.is_empty() {
            let pos = (self.rodada / self.time.len() as u64) as usize % opcoes.len();
            nova.insert(id, opcoes[pos]);
        }
        nova
    }

    fn registrar(&mut self, f: Formacao, nota: f64) {
        let e = self.notas.entry(chave(&f)).or_insert((f, Nota::default()));
        e.1.soma += nota;
        e.1.lutas += 1;
        self.mudou = true;
        // A melhor (com amostra) vence a atual com folga? Aplica.
        let atual = self.notas.get(&chave(&self.atual)).map(|x| x.1);
        let melhor = self
            .notas
            .values()
            .filter(|x| x.1.lutas >= MIN_LUTAS)
            .max_by(|a, b| a.1.media().total_cmp(&b.1.media()));
        if let (Some(a), Some((f, n))) = (atual, melhor) {
            if a.lutas >= MIN_LUTAS && *f != self.atual && n.media() >= a.media() + FOLGA {
                self.aplicar = Some(f.clone());
            }
        }
    }
}

impl Fluxo for Treino {
    fn nome(&self) -> &'static str {
        "treino"
    }

    fn receber(&mut self, _msg: &Value, _agora: i64) {}

    fn receber_conta(&mut self, conta: &str, msg: &Value, agora: i64) {
        if !self.ligado || !conta.eq_ignore_ascii_case(&self.dono) {
            return;
        }
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
                    if let (Some((_, f)), Some(n)) = (self.esperando.take(), nota_da_luta(r)) {
                        self.registrar(f, n);
                    }
                }
            }
            _ => {}
        }
    }

    fn consulta(&mut self, agora: i64) -> Option<String> {
        let faixa = self.faixa.filter(|_| self.ligado)?;
        if self.meta_em.is_some_and(|t| agora - t < META_MS) {
            return None;
        }
        self.meta_em = Some(agora);
        Some(format!("/api/pvp/meta?faixa={faixa}&dias=14"))
    }

    fn resposta(&mut self, _rota: &str, v: &Value, _agora: i64) {
        self.meta = Some(v.clone());
    }

    fn pedir(&mut self, agora: i64) -> Option<Value> {
        if !self.ligado || self.time.is_empty() || self.atual.len() != self.time.len() {
            return None;
        }
        if let Some(f) = self.aplicar.take() {
            self.atual = f.clone();
            return Some(json!({"t": "pvp.time.salvar", "casas": f, "pokemonIds": self.time}));
        }
        if self
            .esperando
            .as_ref()
            .is_some_and(|(t, _)| agora - t < ESPERA_MS)
            || agora < self.livre_em
        {
            return None;
        }
        let a = self.adversario()?;
        let f = self.candidata();
        self.rodada += 1;
        self.esperando = Some((agora, f.clone()));
        self.livre_em = agora + COOLDOWN_MS;
        let b: Vec<Value> = f
            .iter()
            .map(|(id, c)| json!({"meu": id, "casa": c}))
            .collect();
        Some(json!({"t": "treino.lutar", "a": a, "b": b}))
    }

    fn entregar(&mut self, _agora: i64) -> Vec<Entrega> {
        Vec::new()
    }

    fn salvar(&mut self) -> Option<Value> {
        std::mem::take(&mut self.mudou).then(|| {
            let n: Vec<Value> = self
                .notas
                .values()
                .map(|(f, n)| json!({"f": f, "soma": n.soma, "lutas": n.lutas}))
                .collect();
            json!({"v": 1, "notas": n})
        })
    }

    fn restaurar(&mut self, v: &Value) {
        for x in v
            .get("notas")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let f: Formacao = x
                .get("f")
                .and_then(Value::as_object)
                .into_iter()
                .flatten()
                .filter_map(|(k, c)| Some((k.parse().ok()?, c.as_i64()?)))
                .collect();
            let nota = Nota {
                soma: x.get("soma").and_then(Value::as_f64).unwrap_or(0.0),
                lutas: x.get("lutas").and_then(Value::as_u64).unwrap_or(0) as u32,
            };
            if !f.is_empty() {
                self.notas.insert(chave(&f), (f, nota));
            }
        }
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    fn meta() -> Value {
        json!({"partidas": 10,
            "especies": [{"especie": 6, "alcance": 3, "nivel_medio": 300, "casas": [[17, 5, 0.6]]},
                         {"especie": 127, "alcance": 1, "nivel_medio": 100, "casas": [[2, 5, 0.6]]}],
            "alcances": [{"alcance": 1, "casas": [[3, 9, 0.7]]}],
            "times": [{"especies": [6, 127]}]})
    }

    fn pronto() -> Treino {
        let mut t = Treino::novo("Sikien", true);
        t.receber_conta(
            "sikien",
            &json!({"t": "welcome", "estado": {"pokemons": [
            {"id": 1, "speciesId": 127}, {"id": 2, "speciesId": 6}]}}),
            0,
        );
        t.receber_conta(
            "Sikien",
            &json!({"t": "pvp", "time": [1, 2], "casas": {"1": 2, "2": 17},
            "rank": {"pontos": 461}}),
            0,
        );
        t
    }

    fn resultado(venceu_b: bool) -> Value {
        json!({"t": "treino", "resultado": {"venceuA": !venceu_b, "cooldownMs": 1000,
            "placar": [{"id": 1, "dano": 100}, {"id": 2, "dano": if venceu_b { 300 } else { 100 }}]}})
    }

    #[test]
    fn nota_e_desligado() {
        let r =
            json!({"venceuA": false, "placar": [{"id": 1, "dano": 100}, {"id": 2, "dano": 300}]});
        assert_eq!(nota_da_luta(&r), Some(1.25));
        let mut off = Treino::novo("Sikien", false);
        off.receber_conta(
            "Sikien",
            &json!({"t": "pvp", "time": [1], "casas": {"1": 2}}),
            0,
        );
        assert_eq!(off.pedir(0), None);
        assert_eq!(off.consulta(0), None);
    }

    #[test]
    fn testa_a_atual_depois_muda_e_aplica_a_melhor() {
        let mut t = pronto();
        assert_eq!(
            t.consulta(0).as_deref(),
            Some("/api/pvp/meta?faixa=400&dias=14")
        );
        assert_eq!(t.pedir(0), None, "sem meta ainda");
        t.resposta("", &meta(), 0);
        let mut agora = 0;
        let mut aplicou = None;
        for i in 0..40 {
            agora += 10_000;
            let Some(p) = t.pedir(agora) else { continue };
            if p["t"] == "pvp.time.salvar" {
                aplicou = Some(p);
                break;
            }
            assert_eq!(p["t"], "treino.lutar");
            assert_eq!(p["a"][0]["teste"]["speciesId"], 6);
            if i < 3 {
                assert_eq!(
                    p["b"],
                    json!([{"meu": 1, "casa": 2}, {"meu": 2, "casa": 17}])
                );
            }
            // A atual perde; qualquer outra formação vence.
            let atual = p["b"] == json!([{"meu": 1, "casa": 2}, {"meu": 2, "casa": 17}]);
            t.receber_conta("Sikien", &resultado(!atual), agora);
        }
        let p = aplicou.unwrap_or_default();
        assert_eq!(p["pokemonIds"], json!([1, 2]));
        assert_ne!(p["casas"], json!({"1": 2, "2": 17}));
        assert!(t.salvar().is_some());
        let mut outro = pronto();
        outro.restaurar(&t.salvar().unwrap_or_else(|| json!({"notas": t.notas.values().map(|(f, n)| json!({"f": f, "soma": n.soma, "lutas": n.lutas})).collect::<Vec<_>>()})));
        assert!(!outro.notas.is_empty());
    }
}
