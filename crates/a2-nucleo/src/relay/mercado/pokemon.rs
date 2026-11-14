//! Varredura dos anúncios de pokémon (`market.listar`, resposta aba `vitrine`).
//!
//! Sem alvos: faixas de nível × moeda, 2 páginas cada (o filtro de nível do próprio jogo; sem ele
//! as páginas mais baratas só traziam nível alto). Com alvos do servidor: cada espécie × moeda
//! até a página vir incompleta, no máximo [`MAX_PAGS_ESPECIE`]. Diferente do motor antigo, o
//! avanço é pela resposta (página pedida = página recebida), não por tempo.

use super::oferta::ofertas_da_resposta;
use serde_json::{json, Value};

pub const MOEDAS: [&str; 2] = ["orb", "gold"];
pub const FAIXAS: [(&str, &str); 5] = [
    ("1", "400"),
    ("400", "800"),
    ("800", "1300"),
    ("1300", "2000"),
    ("2000", ""),
];
pub const PAGS_FAIXA: u32 = 2;
pub const MAX_PAGS_ESPECIE: u32 = 10;
/// Sem resposta nesse prazo, a unidade é dada por lida e a varredura segue.
pub const ESPERA_RESPOSTA_MS: i64 = 10_000;

/// Espécie alvo (vem de `GET /api/bolsa/alvos-coleta`).
#[derive(Debug, Clone, PartialEq)]
pub struct Alvo {
    pub especie: u64,
    pub nivel_min: String,
    pub nivel_max: String,
}

#[derive(Debug, Clone, PartialEq)]
struct Unidade {
    moeda: &'static str,
    nivel_min: String,
    nivel_max: String,
    especie: u64,
    max_pags: u32,
}

#[derive(Debug)]
pub struct VarreduraPokemon {
    unidades: Vec<Unidade>,
    i: usize,
    pag: u32,
    tam_pag: usize,
    /// Página pedida e quando.
    aguardando: Option<(u32, i64)>,
    pub ofertas: Vec<Value>,
}

impl VarreduraPokemon {
    pub fn nova(alvos: &[Alvo]) -> Self {
        let unidades = if alvos.is_empty() {
            MOEDAS
                .iter()
                .flat_map(|m| {
                    FAIXAS.iter().map(move |(a, b)| Unidade {
                        moeda: m,
                        nivel_min: (*a).into(),
                        nivel_max: (*b).into(),
                        especie: 0,
                        max_pags: PAGS_FAIXA,
                    })
                })
                .collect()
        } else {
            alvos
                .iter()
                .flat_map(|a| {
                    MOEDAS.iter().map(move |m| Unidade {
                        moeda: m,
                        nivel_min: a.nivel_min.clone(),
                        nivel_max: a.nivel_max.clone(),
                        especie: a.especie,
                        max_pags: MAX_PAGS_ESPECIE,
                    })
                })
                .collect()
        };
        Self {
            unidades,
            i: 0,
            pag: 0,
            tam_pag: 0,
            aguardando: None,
            ofertas: Vec::new(),
        }
    }

    pub fn concluida(&self) -> bool {
        self.i >= self.unidades.len()
    }

    fn avancar(&mut self, n: Option<usize>) {
        let max = self.unidades.get(self.i).map_or(0, |u| u.max_pags);
        let fim = match n {
            None | Some(0) => true,
            Some(n) => n < self.tam_pag || self.pag + 1 >= max,
        };
        if fim {
            self.i += 1;
            self.pag = 0;
            self.tam_pag = 0;
        } else {
            self.pag += 1;
        }
    }

    pub fn pedir(&mut self, agora: i64) -> Option<Value> {
        if let Some((_, desde)) = self.aguardando {
            if agora - desde < ESPERA_RESPOSTA_MS {
                return None;
            }
            self.aguardando = None;
            self.avancar(None);
        }
        let u = self.unidades.get(self.i)?;
        let filtro_nivel = !u.nivel_min.is_empty() || !u.nivel_max.is_empty();
        let p = pedido_listar(u, self.pag, filtro_nivel);
        self.aguardando = Some((self.pag, agora));
        Some(p)
    }

    pub fn receber(&mut self, m: &Value) {
        if m.get("aba").and_then(Value::as_str) != Some("vitrine") {
            return;
        }
        let ofertas = ofertas_da_resposta(m);
        self.ofertas.extend(ofertas);
        let Some((pag, _)) = self.aguardando else {
            return;
        };
        let pagina = m.get("pagina").and_then(Value::as_u64).unwrap_or(0);
        if u64::from(pag) != pagina {
            return;
        }
        let n = m
            .get("linhas")
            .and_then(Value::as_array)
            .map_or(0, Vec::len);
        self.tam_pag = self.tam_pag.max(n);
        self.aguardando = None;
        self.avancar(Some(n));
    }
}

/// Mesmo formato do cliente oficial (capturado no modo Descoberta), ordenado pelos mais baratos.
fn pedido_listar(u: &Unidade, pagina: u32, filtro_nivel: bool) -> Value {
    json!({
        "t": "market.listar", "tipo": "pokemon", "moeda": u.moeda, "busca": "",
        "ordem": "baratos", "pagina": pagina, "especieId": u.especie,
        "criterios": if filtro_nivel { json!(["nivel"]) } else { json!([]) },
        "elementos": [], "tiposUnidos": true, "categoria": "",
        "soShiny": false, "soP5": false, "soMega": false, "soLendario": false,
        "soTmElemental": false, "soTmAoe": false, "semOutland": false,
        "nivelMin": u.nivel_min, "nivelMax": u.nivel_max,
        "potenciaMin": "", "potenciaMax": "", "ivMin": "", "ivMax": "",
        "qualidadeMin": "", "qualidadeMax": "", "notaMin": "", "notaMax": "",
        "precoMin": "", "precoMax": "",
    })
}

#[cfg(test)]
mod testes {
    use super::*;

    fn vitrine(pagina: u64, n: usize) -> Value {
        let linhas: Vec<Value> = (0..n)
            .map(|i| json!({"id": pagina * 100 + i as u64, "tipo": "pokemon", "preco": 1}))
            .collect();
        json!({"t": "market", "aba": "vitrine", "pagina": pagina, "linhas": linhas})
    }

    #[test]
    fn faixas_leem_duas_paginas_cada_e_acabam() {
        let mut v = VarreduraPokemon::nova(&[]);
        let mut pedidos = 0;
        while !v.concluida() {
            let p = v.pedir(0).expect("pedido");
            v.receber(&vitrine(p["pagina"].as_u64().unwrap_or(0), 20));
            pedidos += 1;
        }
        assert_eq!(pedidos, 2 * 5 * 2);
        assert_eq!(v.ofertas.len(), 20 * 20);
    }

    #[test]
    fn alvo_para_na_pagina_incompleta_e_sem_resposta_segue() {
        let a = Alvo {
            especie: 464,
            nivel_min: "100".into(),
            nivel_max: "".into(),
        };
        let mut v = VarreduraPokemon::nova(&[a]);
        let p = v.pedir(0).expect("p0");
        assert_eq!(p["especieId"], 464);
        assert_eq!(p["criterios"], json!(["nivel"]));
        v.receber(&vitrine(0, 20));
        assert_eq!(v.pedir(1).map(|p| p["pagina"].clone()), Some(json!(1)));
        v.receber(&vitrine(1, 5)); // incompleta: próxima moeda
        let p = v.pedir(2).expect("gold");
        assert_eq!(
            (p["moeda"].clone(), p["pagina"].clone()),
            (json!("gold"), json!(0))
        );
        assert_eq!(v.pedir(3), None); // ainda esperando
        assert!(v.pedir(2 + ESPERA_RESPOSTA_MS).is_none()); // sem resposta: acabou
        assert!(v.concluida());
    }
}
