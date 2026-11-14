//! Aprende a posição do time de PvP na Área de Treinamento, em cada conta (o aprendizado de
//! uma conta fica em [`conta`]).
//!
//! Só roda com o app como relay oficial e o aprendizado ligado. O meta vem do servidor
//! (`/api/pvp/meta`, por faixa de pontos); as lutas viram eventos `treino.resultado` pelo
//! coletor. O pedido ao jogo leva `_conta`: o app manda para a janela dessa conta. O que uma
//! conta aprende sobre espécie e casa serve às outras ([`comum`]).

pub mod comum;
pub mod conta;

use std::collections::{BTreeMap, HashMap};

use serde_json::{json, Value};

use super::{Entrega, Fluxo};
use comum::Comum;
use conta::TreinoConta;

pub use conta::{nota_da_luta, Formacao};

/// Recarrega o meta de cada faixa a cada 30 min.
const META_MS: i64 = 30 * 60_000;
/// Faixas acima da conta usadas como adversário (de 100 em 100 pontos).
const FAIXAS_ACIMA: i64 = 2;

pub struct Treino {
    /// Conta dona: recebe as notas do arquivo antigo (de quando só ela aprendia).
    dono: String,
    ligado: bool,
    contas: BTreeMap<String, TreinoConta>,
    /// faixa → quando o meta foi pedido.
    meta_em: HashMap<i64, i64>,
    /// faixa → meta recebido.
    metas: HashMap<i64, Value>,
    /// Revezamento: a próxima conta a pedir.
    vez: usize,
    comum: Comum,
    mudou: bool,
}

impl Treino {
    pub fn novo(dono: &str, ligado: bool) -> Self {
        Self {
            dono: dono.to_lowercase(),
            ligado,
            contas: BTreeMap::new(),
            meta_em: HashMap::new(),
            metas: HashMap::new(),
            vez: 0,
            comum: Comum::default(),
            mudou: false,
        }
    }

    fn conta(&mut self, conta: &str) -> &mut TreinoConta {
        self.contas
            .entry(conta.to_lowercase())
            .or_insert_with(TreinoConta::nova)
    }
}

/// Faixa da rota `/api/pvp/meta?faixa=..`.
fn faixa_da_rota(rota: &str) -> Option<i64> {
    rota.split(['?', '&'])
        .find_map(|p| p.strip_prefix("faixa="))?
        .parse()
        .ok()
}

impl Fluxo for Treino {
    fn nome(&self) -> &'static str {
        "treino"
    }

    fn receber(&mut self, _msg: &Value, _agora: i64) {}

    fn receber_conta(&mut self, conta: &str, msg: &Value, agora: i64) {
        if !self.ligado {
            return;
        }
        if let Some((pares, nota)) = self.conta(conta).receber(msg, agora) {
            self.comum.registrar(&pares, nota);
        }
    }

    fn consulta(&mut self, agora: i64) -> Option<String> {
        if !self.ligado {
            return None;
        }
        // A faixa de cada conta e as de cima.
        let mut faixas: Vec<i64> = self
            .contas
            .values()
            .filter_map(TreinoConta::faixa)
            .flat_map(|f| (0..=FAIXAS_ACIMA).map(move |i| f + i * 100))
            .collect();
        faixas.sort_unstable();
        faixas.dedup();
        let faixa = faixas
            .into_iter()
            .find(|f| self.meta_em.get(f).is_none_or(|t| agora - t >= META_MS))?;
        self.meta_em.insert(faixa, agora);
        Some(format!("/api/pvp/meta?faixa={faixa}&dias=14"))
    }

    fn resposta(&mut self, rota: &str, v: &Value, _agora: i64) {
        let Some(faixa) = faixa_da_rota(rota) else {
            return;
        };
        self.metas.insert(faixa, v.clone());
        for c in self.contas.values_mut() {
            let Some(f) = c.faixa() else { continue };
            if let Some(m) = self.metas.get(&f) {
                c.definir_meta(m);
            }
            c.definir_acima(
                (1..=FAIXAS_ACIMA)
                    .filter_map(|i| self.metas.get(&(f + i * 100)).cloned())
                    .collect(),
            );
        }
    }

    fn pedir(&mut self, agora: i64) -> Option<Value> {
        if !self.ligado || self.contas.is_empty() {
            return None;
        }
        let nomes: Vec<String> = self.contas.keys().cloned().collect();
        let n = nomes.len();
        for i in 0..n {
            let nome = &nomes[(self.vez + i) % n];
            let comum = &self.comum;
            let Some(mut p) = self
                .contas
                .get_mut(nome)
                .and_then(|c| c.pedir(agora, comum))
            else {
                continue;
            };
            self.vez = (self.vez + i + 1) % n;
            if let Value::Object(o) = &mut p {
                o.insert("_conta".into(), Value::from(nome.clone()));
            }
            return Some(p);
        }
        None
    }

    fn entregar(&mut self, _agora: i64) -> Vec<Entrega> {
        Vec::new()
    }

    fn salvar(&mut self) -> Option<Value> {
        let mut mudou = std::mem::take(&mut self.mudou) | self.comum.mudou();
        for c in self.contas.values_mut() {
            mudou |= c.mudou();
        }
        mudou.then(|| {
            let contas: serde_json::Map<String, Value> = self
                .contas
                .iter()
                .map(|(k, c)| (k.clone(), Value::from(c.notas_json())))
                .collect();
            json!({"v": 2, "contas": contas, "comum": self.comum.para_json()})
        })
    }

    fn restaurar(&mut self, v: &Value) {
        let lista = |x: &Value| x.as_array().cloned().unwrap_or_default();
        match v.get("contas").and_then(Value::as_object) {
            Some(m) => {
                for (k, notas) in m {
                    self.conta(k).restaurar(&lista(notas));
                }
                if let Some(c) = v.get("comum") {
                    self.comum.restaurar(c);
                }
            }
            // v1: só a conta dona aprendia.
            None => {
                let dono = self.dono.clone();
                if let Some(n) = v.get("notas") {
                    self.conta(&dono).restaurar(&lista(n));
                    self.mudou = true;
                }
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

    fn preparar(t: &mut Treino, conta: &str, pontos: i64) {
        t.receber_conta(
            conta,
            &json!({"t": "welcome", "estado": {"pokemons": [
            {"id": 1, "speciesId": 127}, {"id": 2, "speciesId": 6}]}}),
            0,
        );
        t.receber_conta(
            conta,
            &json!({"t": "pvp", "time": [1, 2], "casas": {"1": 2, "2": 17},
            "rank": {"pontos": pontos}}),
            0,
        );
    }

    fn resultado(venceu_b: bool) -> Value {
        json!({"t": "treino", "resultado": {"venceuA": !venceu_b, "cooldownMs": 1000,
            "placar": [{"id": 1, "dano": 100}, {"id": 2, "dano": if venceu_b { 300 } else { 100 }}]}})
    }

    #[test]
    fn desligado_nao_faz_nada() {
        let mut off = Treino::novo("Sikien", false);
        preparar(&mut off, "Sikien", 461);
        assert_eq!(off.pedir(0), None);
        assert_eq!(off.consulta(0), None);
    }

    #[test]
    fn cada_conta_aprende_com_o_meta_da_sua_faixa() {
        let mut t = Treino::novo("sikien", true);
        preparar(&mut t, "Sikien", 461);
        preparar(&mut t, "Alllphha2", 1210);
        let rotas: Vec<String> = std::iter::from_fn(|| t.consulta(0)).collect();
        let faixas: Vec<&str> = rotas
            .iter()
            .filter_map(|r| r.split("faixa=").nth(1)?.split('&').next())
            .collect();
        assert_eq!(faixas, ["400", "500", "600", "1200", "1300", "1400"]);
        assert_eq!(t.pedir(0), None, "sem meta ainda");
        // A faixa de cima tem outro time (espécie 212): entra no revezamento do Sikien.
        let mut forte = meta();
        forte["times"] = json!([{"especies": [212]}]);
        for r in &rotas {
            let m = if r.contains("faixa=500") {
                forte.clone()
            } else {
                meta()
            };
            t.resposta(r, &m, 0);
        }
        assert_eq!(t.contas["sikien"].acima.len(), 2);
        // As duas contas lutam, cada uma na sua janela.
        let a = t.pedir(0).unwrap_or_default();
        let b = t.pedir(0).unwrap_or_default();
        let mut quem = vec![a["_conta"].clone(), b["_conta"].clone()];
        quem.sort_by_key(ToString::to_string);
        assert_eq!(quem, vec![json!("alllphha2"), json!("sikien")]);
        assert_eq!(a["t"], "treino.lutar");
        let mut viu_forte = false;
        // Só o Sikien: a atual perde, qualquer outra vence → aplica uma formação nova nele.
        let mut agora = 0;
        let mut aplicou = None;
        for _ in 0..80 {
            agora += 10_000;
            let Some(p) = t.pedir(agora) else { continue };
            if p["_conta"] != "sikien" {
                continue;
            }
            if p["t"] == "pvp.time.salvar" {
                aplicou = Some(p);
                break;
            }
            viu_forte |= p["a"][0]["teste"]["speciesId"] == 212;
            let atual = p["b"] == json!([{"meu": 1, "casa": 2}, {"meu": 2, "casa": 17}]);
            t.receber_conta("Sikien", &resultado(!atual), agora);
        }
        assert!(viu_forte, "lutou também contra o time da faixa de cima");
        let p = aplicou.unwrap_or_default();
        assert_eq!(p["pokemonIds"], json!([1, 2]));
        assert_ne!(p["casas"], json!({"1": 2, "2": 17}));
        // As lutas do Sikien entraram no aprendizado comum (espécies 127 e 6).
        assert!(t
            .comum
            .para_json()
            .as_array()
            .is_some_and(|a| !a.is_empty()));
        // Guarda por conta e volta; o arquivo antigo (v1) vai para a dona.
        let salvo = t.salvar().unwrap_or_default();
        let mut outro = Treino::novo("sikien", true);
        outro.restaurar(&salvo);
        assert!(!outro.contas["sikien"].notas_json().is_empty());
        assert_eq!(outro.comum.para_json(), t.comum.para_json());
        let mut velho = Treino::novo("Sikien", true);
        velho.restaurar(&json!({"v": 1, "notas": salvo["contas"]["sikien"]}));
        assert!(!velho.contas["sikien"].notas_json().is_empty());
    }
}
