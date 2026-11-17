//! Times reais do topo do PvP como adversários do treino: a cada [`RANKING_MS`] pede a
//! primeira página da tabela (`pvp.tabela`) e, um por vez, o perfil (`ranking.perfil`) dos
//! [`TOP`] primeiros. Do perfil fica só o time (espécie, IVs, qualidade, potência, TM); o
//! nível de cada luta é o do time da conta que treina (ver `conta.rs`).

use std::collections::{BTreeMap, VecDeque};

use serde_json::{json, Value};

/// Tabela do PvP: a cada 12 h.
const RANKING_MS: i64 = 12 * 3_600_000;
/// Entre dois pedidos de perfil.
const PERFIL_MS: i64 = 20_000;
/// Quantos do topo.
pub const TOP: usize = 10;

/// Um pokémon do time de um adversário (o que o treino usa como pokémon teste).
fn ficha(p: &Value) -> Option<Value> {
    let sp = p.get("speciesId")?.as_i64()?;
    Some(json!({
        "speciesId": sp,
        "ivs": p.get("ivs").cloned().unwrap_or(Value::Null),
        "qualidade": p.get("quality").cloned().unwrap_or(json!(1)),
        "potencia": p.get("potencia").cloned().unwrap_or(json!(1)),
        "shiny": p.get("shiny").and_then(Value::as_bool).unwrap_or(false),
        "tmElemental": p.get("tmElemental").cloned().unwrap_or(Value::Null),
    }))
}

#[derive(Default)]
pub struct Ranking {
    pedido_em: Option<i64>,
    perfil_em: Option<i64>,
    fila: VecDeque<String>,
    posicoes: BTreeMap<String, i64>,
    /// nick → time (fichas), na ordem da tabela.
    times: BTreeMap<String, (i64, Vec<Value>)>,
    mudou: bool,
}

impl Ranking {
    /// O próximo pedido ao jogo, se for a hora.
    pub fn pedir(&mut self, agora: i64) -> Option<Value> {
        if self.pedido_em.is_none_or(|t| agora - t >= RANKING_MS) {
            self.pedido_em = Some(agora);
            return Some(json!({"t": "pvp.tabela", "pagina": 1}));
        }
        if self.perfil_em.is_some_and(|t| agora - t < PERFIL_MS) {
            return None;
        }
        let nick = self.fila.pop_front()?;
        self.perfil_em = Some(agora);
        Some(json!({"t": "ranking.perfil", "nick": nick}))
    }

    /// `pvpTabela` (posições) e `perfil` (time de um jogador). `true` se mudou algum time.
    pub fn receber(&mut self, msg: &Value) -> bool {
        match msg.get("t").and_then(Value::as_str) {
            Some("pvpTabela") => {
                let mut linhas: Vec<(i64, String)> = msg
                    .get("linhas")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|l| Some((l.get(0)?.as_i64()?, l.get(1)?.as_str()?.to_owned())))
                    .filter(|(pos, _)| *pos >= 1 && *pos as usize <= TOP)
                    .collect();
                if linhas.is_empty() {
                    return false;
                }
                linhas.sort_by_key(|(pos, _)| *pos);
                self.posicoes = linhas
                    .iter()
                    .map(|(pos, nick)| (nick.clone(), *pos))
                    .collect();
                self.times.retain(|n, _| self.posicoes.contains_key(n));
                self.fila = linhas.into_iter().map(|(_, nick)| nick).collect();
                self.mudou = true;
                true
            }
            Some("perfil") => {
                let Some(p) = msg.get("perfil") else {
                    return false;
                };
                let Some(nick) = p.get("nick").and_then(Value::as_str) else {
                    return false;
                };
                let nick = self
                    .posicoes
                    .keys()
                    .find(|n| n.eq_ignore_ascii_case(nick))
                    .map_or(nick, String::as_str);
                let time: Vec<Value> = p
                    .get("pvpTime")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(ficha)
                    .collect();
                if time.is_empty() {
                    return false;
                }
                let pontos = p
                    .pointer("/pvpRank/pontos")
                    .and_then(Value::as_i64)
                    .unwrap_or(0);
                self.times.insert(nick.to_owned(), (pontos, time));
                self.mudou = true;
                true
            }
            _ => false,
        }
    }

    /// Times guardados: `(nick, fichas)`, dos mais fortes para os mais fracos.
    pub fn times(&self) -> Vec<(String, Vec<Value>)> {
        let mut v: Vec<_> = self.times.iter().collect();
        v.sort_by_key(|(_, (p, _))| std::cmp::Reverse(*p));
        v.into_iter()
            .map(|(n, (_, t))| (n.clone(), t.clone()))
            .collect()
    }

    pub fn mudou(&mut self) -> bool {
        std::mem::take(&mut self.mudou)
    }

    /// Só os dados visíveis no mapa, sem as fichas usadas para simular o treino.
    pub fn para_mapa(&self) -> Value {
        let times: serde_json::Map<String, Value> = self
            .times
            .iter()
            .filter(|(nick, _)| self.posicoes.is_empty() || self.posicoes.contains_key(*nick))
            .map(|(nick, (pontos, time))| {
                let especies: Vec<_> = time
                    .iter()
                    .filter_map(|p| p.get("speciesId").and_then(Value::as_i64))
                    .map(|id| json!({"speciesId": id}))
                    .collect();
                (
                    nick.clone(),
                    json!({"pontos": pontos, "posicao": self.posicoes.get(nick), "time": especies}),
                )
            })
            .collect();
        json!({"times": times, "pedidoEm": self.pedido_em})
    }

    pub fn para_json(&self) -> Value {
        let t: serde_json::Map<String, Value> = self
            .times
            .iter()
            .map(|(n, (p, t))| (n.clone(), json!({"pontos": p, "time": t})))
            .collect();
        json!({"times": t, "pedidoEm": self.pedido_em, "posicoes": self.posicoes})
    }

    pub fn restaurar(&mut self, v: &Value) {
        for (n, x) in v
            .get("times")
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
        {
            let pontos = x.get("pontos").and_then(Value::as_i64).unwrap_or(0);
            let time = x
                .get("time")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            if !time.is_empty() {
                self.times.insert(n.clone(), (pontos, time));
            }
        }
        self.posicoes = v
            .get("posicoes")
            .cloned()
            .and_then(|p| serde_json::from_value(p).ok())
            .unwrap_or_default();
        self.pedido_em = v.get("pedidoEm").and_then(Value::as_i64);
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn tabela_depois_perfis_um_por_vez_e_guarda_o_time() {
        let mut r = Ranking::default();
        assert_eq!(r.pedir(0), Some(json!({"t": "pvp.tabela", "pagina": 1})));
        let linhas: Vec<Value> = (1..=12)
            .map(|i| json!([i, format!("n{i}"), "x", "", 9000 - i, 1, 1]))
            .collect();
        r.receber(&json!({"t": "pvpTabela", "linhas": linhas}));
        assert_eq!(
            r.pedir(1),
            Some(json!({"t": "ranking.perfil", "nick": "n1"}))
        );
        assert_eq!(r.pedir(PERFIL_MS - 1), None, "espera entre perfis");
        assert_eq!(
            r.pedir(PERFIL_MS + 1),
            Some(json!({"t": "ranking.perfil", "nick": "n2"}))
        );
        let perfil = json!({"t": "perfil", "perfil": {"nick": "n1", "pvpRank": {"pontos": 8800},
            "pvpTime": [{"speciesId": 3003, "level": 5331, "quality": 1.4, "potencia": 3,
                         "ivs": {"hp": 31}, "shiny": true, "tmElemental": "FIRE", "dono": "n1"}]}});
        assert!(r.receber(&perfil));
        let t = r.times();
        assert_eq!(t[0].0, "n1");
        assert_eq!(t[0].1[0]["speciesId"], 3003);
        assert!(
            t[0].1[0].get("level").is_none(),
            "o nível é o da conta que treina"
        );
        assert!(t[0].1[0].get("dono").is_none());
        let mut volta = Ranking::default();
        volta.restaurar(&r.para_json());
        assert_eq!(volta.times(), t);
        assert_eq!(
            volta.pedir(PERFIL_MS * 2),
            None,
            "tabela recente: não pede de novo"
        );
    }
}
