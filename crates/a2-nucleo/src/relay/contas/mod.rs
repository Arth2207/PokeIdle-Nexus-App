//! Fluxo das contas: equipe e coleção (`/api/equipes/ingest`), hunt atual (`/api/hunt-sync`),
//! inventário somado (`/api/bolsa/meus-itens`) e raros com histórico (`/api/raros/ingest`).
//! O que é estado global (inventário somado e raros) só sai com o app como relay oficial: em
//! sombra, o legado ainda manda a soma das contas dele e um apagaria o outro.

pub mod conta;
pub mod diario;
pub mod raros;

use std::collections::{BTreeMap, HashMap, VecDeque};

use serde_json::{json, Value};

use super::{Entrega, Fluxo};
use conta::{bosses_do_welcome, Conta};
use diario::Diario;
use raros::Medias;

/// Equipe: reenvia a cada 5 min (como o relay antigo) e no máximo a cada 1 min quando muda.
const EQUIPE_MS: i64 = 5 * 60_000;
const EQUIPE_MUDOU_MS: i64 = 60_000;
/// Inventário somado: a cada 3 min, se mudou.
const ITENS_MS: i64 = 3 * 60_000;
/// Raros: a cada 2 min.
const RAROS_MS: i64 = 2 * 60_000;
/// Médias de preço do mercado (7 dias e hoje): a cada 30 min.
const MEDIAS_MS: i64 = 30 * 60_000;
/// Resumo do dia (o `D.dia` do motor) para o Nexus: a cada 5 min.
const DIARIO_MS: i64 = 5 * 60_000;

/// Saldo de ouro de um dia: início, fim, mínimo, máximo e quando começou e acabou.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Saldo {
    ini: f64,
    fim: f64,
    min: f64,
    max: f64,
    t_ini: i64,
    t_fim: i64,
}

#[derive(Default)]
pub struct Contas {
    oficial: bool,
    contas: BTreeMap<String, Conta>,
    diario: Diario,
    diario_mudou: bool,
    nomes: HashMap<i64, String>,
    bosses: Option<Value>,
    chance_shiny: Option<f64>,
    m7: Medias,
    m1: Medias,
    medias_em: Option<i64>,
    medias_fila: VecDeque<u8>,
    equipe_em: HashMap<String, i64>,
    mudou: HashMap<String, bool>,
    hunt_enviada: HashMap<String, String>,
    itens_sig: String,
    itens_em: i64,
    raros_em: i64,
    /// (nick, dia) → saldo de ouro.
    saldos: HashMap<(String, String), Saldo>,
    diario_em: i64,
}

impl Contas {
    /// `oficial`: o app é o relay oficial (manda também os estados globais).
    pub fn novo(oficial: bool) -> Self {
        Self {
            oficial,
            itens_em: i64::MIN / 2,
            raros_em: i64::MIN / 2,
            diario_em: i64::MIN / 2,
            ..Self::default()
        }
    }

    /// Nick da conta do app (o do jogo, se já veio; senão o nome da conta).
    fn nick(&self, conta: &str) -> String {
        self.contas
            .get(conta)
            .map(|c| c.nick.clone())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| conta.to_owned())
    }

    fn welcome(&mut self, w: &Value) {
        for i in w
            .get("itensNossos")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let (Some(id), Some(n)) = (
                i.get("id").and_then(Value::as_i64),
                i.get("name").or(i.get("nome")).and_then(Value::as_str),
            ) {
                self.nomes.insert(id, n.to_owned());
            }
        }
        if let Some(b) = bosses_do_welcome(w) {
            self.bosses = Some(b);
        }
        if let Some(c) = w
            .get("chanceShiny")
            .and_then(Value::as_f64)
            .filter(|c| *c > 0.0)
        {
            self.chance_shiny = Some(c);
        }
    }

    fn ouro(&mut self, conta: &str, msg: &Value, agora: i64) {
        let Some(g) = msg.pointer("/estado/gold").and_then(Value::as_f64) else {
            return;
        };
        let chave = (self.nick(conta), super::tempo::dia_de(agora));
        let s = self.saldos.entry(chave).or_insert(Saldo {
            ini: g,
            fim: g,
            min: g,
            max: g,
            t_ini: agora,
            t_fim: agora,
        });
        s.fim = g;
        s.min = s.min.min(g);
        s.max = s.max.max(g);
        s.t_fim = agora;
        // Só os dias recentes interessam.
        let hoje = super::tempo::dia_de(agora);
        let ontem = super::tempo::dia_de(agora - 86_400_000);
        self.saldos.retain(|(_, d), _| *d == hoje || *d == ontem);
    }

    /// O dia de hoje e o de ontem de cada conta, no formato do motor, como eventos `diario`.
    fn entregar_diario(&mut self, agora: i64, saida: &mut Vec<Entrega>) {
        if agora - self.diario_em < DIARIO_MS {
            return;
        }
        self.diario_em = agora;
        let dias = [
            super::tempo::dia_de(agora),
            super::tempo::dia_de(agora - 86_400_000),
        ];
        for conta in self.contas.keys() {
            let nick = self.nick(conta);
            let eventos: Vec<Value> = dias
                .iter()
                .filter_map(|dia| {
                    let mut d = self.diario.dia_json(&nick, dia)?;
                    d["k"] = json!("diario");
                    d["ts"] = json!(agora);
                    d["dia"] = json!(dia);
                    d["jogo_nick"] = json!(nick);
                    if let Some(s) = self.saldos.get(&(nick.clone(), dia.clone())) {
                        d["saldo"] = json!({"ini": s.ini, "fim": s.fim, "min": s.min, "max": s.max,
                                            "tIni": s.t_ini, "tFim": s.t_fim});
                    }
                    Some(d)
                })
                .collect();
            if !eventos.is_empty() {
                saida.push(Entrega::Diario {
                    conta: conta.to_lowercase(),
                    eventos,
                });
            }
        }
    }

    fn entregar_equipes(&mut self, agora: i64, saida: &mut Vec<Entrega>) {
        for (nome, c) in &self.contas {
            if !c.pronta || c.nick.is_empty() {
                continue;
            }
            let em = self.equipe_em.get(nome).copied().unwrap_or(i64::MIN / 2);
            let mudou = self.mudou.get(nome).copied().unwrap_or(false);
            if agora - em >= EQUIPE_MS || (mudou && agora - em >= EQUIPE_MUDOU_MS) {
                saida.push(Entrega::Equipe(c.equipe(self.bosses.as_ref())));
                self.equipe_em.insert(nome.clone(), agora);
                self.mudou.insert(nome.clone(), false);
            }
        }
    }

    fn entregar_hunts(&mut self, agora: i64, saida: &mut Vec<Entrega>) {
        let mut eventos = Vec::new();
        for c in self.contas.values() {
            let (Some(h), false) = (&c.hunt, c.nick.is_empty()) else {
                continue;
            };
            let nick = c.nick.to_lowercase();
            if self.hunt_enviada.get(&nick) != Some(h) {
                eventos.push(json!({"k": "hunt_sync", "ts": agora, "nick": nick, "hunt_slug": h}));
                self.hunt_enviada.insert(nick, h.clone());
            }
        }
        if !eventos.is_empty() {
            saida.push(Entrega::Hunt(eventos));
        }
    }

    fn entregar_itens(&mut self, agora: i64, saida: &mut Vec<Entrega>) {
        if agora - self.itens_em < ITENS_MS {
            return;
        }
        let prontas: Vec<&Conta> = self.contas.values().filter(|c| c.pronta).collect();
        if prontas.is_empty() {
            return;
        }
        self.itens_em = agora;
        let mut soma: BTreeMap<i64, i64> = BTreeMap::new();
        for c in prontas {
            for (&id, &q) in &c.itens {
                if id > 0 && q > 0 {
                    *soma.entry(id).or_default() += q;
                }
            }
        }
        let itens: serde_json::Map<String, Value> = soma
            .iter()
            .map(|(k, v)| (k.to_string(), json!(v)))
            .collect();
        let corpo = json!({"limpar": true, "itens": itens});
        let sig = corpo.to_string();
        if !itens.is_empty() && sig != self.itens_sig {
            self.itens_sig = sig;
            saida.push(Entrega::MeusItens(corpo));
        }
    }

    fn entregar_raros(&mut self, agora: i64, saida: &mut Vec<Entrega>) {
        if agora - self.raros_em < RAROS_MS {
            return;
        }
        let contas: Vec<&Conta> = self.contas.values().filter(|c| c.pronta).collect();
        if contas.is_empty() {
            return;
        }
        self.raros_em = agora;
        let hoje = super::tempo::dia_de(agora);
        let drops = self.diario.drops_por_dia(&hoje);
        let nicks: Vec<String> = contas.iter().map(|c| c.nick.clone()).collect();
        let plano = raros::plano(&raros::Entrada {
            contas,
            nomes: &self.nomes,
            m7: &self.m7,
            m1: &self.m1,
            drops_dia: &drops,
            agora,
        });
        let historico = raros::historico(&self.diario, &nicks, self.chance_shiny, agora);
        saida.push(Entrega::Raros(
            json!({"raros": plano, "historico": historico}),
        ));
    }
}

impl Fluxo for Contas {
    fn nome(&self) -> &'static str {
        "contas"
    }

    /// Resposta do jogo na conta dona: só as médias de preço interessam aqui.
    fn receber(&mut self, msg: &Value, _agora: i64) {
        if msg.get("t").and_then(Value::as_str) != Some("market")
            || msg.get("aba").and_then(Value::as_str) != Some("medias")
        {
            return;
        }
        let m = Medias::da_resposta(msg);
        if msg.get("dias").and_then(Value::as_i64) == Some(1) {
            self.m1 = m;
        } else {
            self.m7 = m;
        }
    }

    fn receber_conta(&mut self, conta: &str, msg: &Value, agora: i64) {
        match msg.get("t").and_then(Value::as_str) {
            Some(t @ ("welcome" | "estado")) => {
                if t == "welcome" {
                    self.welcome(msg);
                }
                self.ouro(conta, msg, agora);
                let e = msg.get("estado");
                let mexeu_pokemon = t == "welcome"
                    || ["pokemons", "pkMud", "pkFora"]
                        .iter()
                        .any(|k| e.is_some_and(|e| e.get(*k).is_some()));
                if self
                    .contas
                    .entry(conta.to_owned())
                    .or_default()
                    .aplicar(msg)
                    && mexeu_pokemon
                {
                    self.mudou.insert(conta.to_owned(), true);
                }
            }
            Some("hud") => {
                let nick = self.nick(conta);
                if let Some(l) = msg.get("linhas").and_then(Value::as_array) {
                    self.diario.linhas(&nick, l);
                    self.diario_mudou = true;
                }
            }
            _ => {}
        }
    }

    fn pedir(&mut self, agora: i64) -> Option<Value> {
        if self.oficial && self.medias_em.is_none_or(|t| agora - t >= MEDIAS_MS) {
            self.medias_em = Some(agora);
            self.medias_fila = VecDeque::from([7, 1]);
        }
        let dias = self.medias_fila.pop_front()?;
        Some(json!({"t": "market.medias", "dias": dias}))
    }

    /// O diário (contagens por dia) sobrevive a reinícios do app.
    fn salvar(&mut self) -> Option<Value> {
        std::mem::take(&mut self.diario_mudou).then(|| self.diario.para_json())
    }

    fn restaurar(&mut self, v: &Value) {
        self.diario = Diario::de_json(v);
    }

    fn entregar(&mut self, agora: i64) -> Vec<Entrega> {
        let mut s = Vec::new();
        self.entregar_equipes(agora, &mut s);
        self.entregar_hunts(agora, &mut s);
        self.entregar_diario(agora, &mut s);
        if self.oficial {
            self.entregar_itens(agora, &mut s);
            self.entregar_raros(agora, &mut s);
        }
        s
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    fn login(nick: &str) -> Value {
        json!({"t": "welcome", "chanceShiny": 0.00004,
               "itensNossos": [{"id": 70013, "name": "Gema Rara"}],
               "estado": {"nick": nick, "level": 50, "huntSlug": "caterpie",
                          "items": {"70013": 2, "23": 5},
                          "pokemons": [{"id": 1, "speciesId": 6, "ivs": {"hp": 1}}]}})
    }

    #[test]
    fn sombra_manda_equipe_e_hunt_mas_nao_o_global() {
        let mut c = Contas::novo(false);
        c.receber_conta("Sikien", &login("Sikien"), 0);
        let e = c.entregar(0);
        assert!(matches!(&e[0], Entrega::Equipe(b) if b["nick"] == "sikien"));
        assert!(matches!(&e[1], Entrega::Hunt(h) if h[0]["hunt_slug"] == "caterpie"));
        assert_eq!(e.len(), 2, "sem linhas do HUD ainda, sem diário");
        // Nada mudou: nada sai até dar o tempo da equipe.
        assert!(c.entregar(30_000).is_empty());
        c.receber_conta(
            "Sikien",
            &json!({"t": "estado", "estado": {"pkFora": [1]}}),
            61_000,
        );
        assert!(matches!(&c.entregar(61_000)[0], Entrega::Equipe(b) if b["pokemons"] == json!([])));
        assert_eq!(c.pedir(0), None);
    }

    #[test]
    fn diario_no_formato_do_motor_com_saldo() {
        let mut c = Contas::novo(false);
        let t = 1_794_632_400_000; // 14/11 02:00 em UTC−3
        let mut w = login("Sikien");
        w["estado"]["gold"] = json!(1000);
        c.receber_conta("Sikien", &w, t);
        c.receber_conta(
            "Sikien",
            &json!({"t": "estado", "estado": {"gold": 1500}}),
            t + 1,
        );
        c.receber_conta(
            "Sikien",
            &json!({"t": "hud", "linhas": [
            [1, t, "A derrotado! +10 xp treinador, +20 xp pokemon, +30 ouro"]]}),
            t + 2,
        );
        let e = c.entregar(t + 3);
        let d = e.iter().find_map(|x| match x {
            Entrega::Diario { conta, eventos } => Some((conta.clone(), eventos.clone())),
            _ => None,
        });
        let (conta, ev) = d.unwrap_or_default();
        assert_eq!(conta, "sikien");
        assert_eq!(ev[0]["dia"], "2026-11-14");
        assert_eq!(ev[0]["k"], "diario");
        assert_eq!(
            (
                ev[0]["kills"].clone(),
                ev[0]["ouro"].clone(),
                ev[0]["xpPk"].clone()
            ),
            (json!(1), json!(30.0), json!(20.0))
        );
        assert_eq!(ev[0]["saldo"]["ini"], 1000.0);
        assert_eq!(ev[0]["saldo"]["fim"], 1500.0);
        // Só a cada 5 min.
        assert!(!c
            .entregar(t + 60_000)
            .iter()
            .any(|x| matches!(x, Entrega::Diario { .. })));
    }

    #[test]
    fn oficial_soma_itens_e_manda_raros() {
        let mut c = Contas::novo(true);
        c.receber_conta("A", &login("A"), 0);
        c.receber_conta("B", &login("B"), 0);
        assert_eq!(c.pedir(0), Some(json!({"t": "market.medias", "dias": 7})));
        assert_eq!(c.pedir(1), Some(json!({"t": "market.medias", "dias": 1})));
        assert_eq!(c.pedir(2), None);
        c.receber(
            &json!({"t": "market", "aba": "medias", "dias": 7,
                          "medias": {"70013": {"orb": {"media": 50}}}}),
            3,
        );
        let e = c.entregar(1_794_632_400_000);
        let itens = e.iter().find_map(|x| match x {
            Entrega::MeusItens(b) => Some(b.clone()),
            _ => None,
        });
        assert_eq!(
            itens,
            Some(json!({"limpar": true, "itens": {"23": 10, "70013": 4}}))
        );
        let raros = e.iter().find_map(|x| match x {
            Entrega::Raros(b) => Some(b.clone()),
            _ => None,
        });
        let raros = raros.unwrap_or_default();
        assert_eq!(raros["raros"]["itens"][0]["nome"], "Gema Rara");
        assert_eq!(raros["raros"]["itens"][0]["valorGemas"], 200.0);
        assert_eq!(raros["historico"]["chanceShiny"], 0.00004);
        // Inventário igual não reenvia.
        let e2 = c.entregar(1_794_632_400_000 + ITENS_MS);
        assert!(!e2.iter().any(|x| matches!(x, Entrega::MeusItens(_))));
    }
}
