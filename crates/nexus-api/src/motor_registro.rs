//! Fase 4 com escrita: o que o motor do Nexus anota no `D.acoes` (`nexus.motor_acao`), como o
//! legado fazia: o resultado de cada comando (texto de sucesso ou de falha, com o custo da
//! compra, que o teto de ouro por hora soma), os alertas e os registros das regras.
//!
//! Alertas já chegam com o antiflood das regras. Registros que se repetem a cada ciclo
//! ("compra adiada", "parei antes de vender") só entram quando o texto muda.

use std::collections::HashMap;

use nucleo::motor::laco::DecisaoDoCiclo;
use nucleo::motor::regras::{Decisao, Registro};
use serde_json::{json, Value};

/// Comando sem resultado depois disto sai da espera (expirou ou o app caiu).
const ESPERA_MS: i64 = 10 * 60_000;

#[derive(Default)]
pub struct Registros {
    /// id do comando → (nick, registro, quando saiu).
    pendentes: HashMap<i64, (String, Registro, i64)>,
    /// (nick, tipo) → último texto anotado de um registro.
    ultimo: HashMap<(String, String), String>,
    /// Registros do ciclo ainda não gravados.
    novos: Vec<((String, String), String)>,
}

fn acao(agora: i64, nick: &str, tipo: &str, texto: &str, ok: bool) -> Value {
    json!({"t": agora, "nick": nick, "tipo": tipo, "texto": texto, "ok": ok})
}

impl Registros {
    /// Comandos enfileirados com o registro de cada um.
    pub fn aguardar(&mut self, agora: i64, enviados: Vec<(i64, String, Registro)>) {
        for (id, nick, r) in enviados {
            self.pendentes.insert(id, (nick, r, agora));
        }
    }

    pub fn ids(&self) -> Vec<i64> {
        self.pendentes.keys().copied().collect()
    }

    /// Resultados que o app confirmou → ações. O que esperou demais sai sem ação.
    pub fn dos_resultados(&mut self, agora: i64, resultados: &[(i64, String)]) -> Vec<Value> {
        let mut s = Vec::new();
        for (id, r) in resultados {
            let Some((nick, reg, _)) = self.pendentes.remove(id) else {
                continue;
            };
            let ok = r == "ok";
            let texto = if ok {
                reg.texto_ok.clone()
            } else {
                reg.texto_falha.replace("{r}", r)
            };
            let mut a = acao(agora, &nick, &reg.tipo, &texto, ok);
            if let (true, Some(c), Value::Object(o)) = (ok, reg.custo, &mut a) {
                o.insert("custo".into(), json!(c));
            }
            s.push(a);
        }
        self.pendentes.retain(|_, (_, _, t)| agora - *t < ESPERA_MS);
        s
    }

    /// Alertas e registros do ciclo → ações (registro repetido não entra). O texto só conta
    /// como anotado depois de [`Registros::anotados`] (se a gravação falhar, entra no próximo).
    pub fn das_decisoes(&mut self, agora: i64, decisoes: &[DecisaoDoCiclo]) -> Vec<Value> {
        self.novos.clear();
        let mut s = Vec::new();
        for x in decisoes {
            match &x.decisao {
                Decisao::Alerta {
                    nick,
                    titulo,
                    texto,
                    ..
                } => s.push(acao(
                    agora,
                    nick,
                    "alerta",
                    &format!("{titulo}: {texto}"),
                    true,
                )),
                Decisao::Registro {
                    nick,
                    tipo,
                    texto,
                    ok,
                } => {
                    let k = (nick.clone(), tipo.clone());
                    if self.ultimo.get(&k) != Some(texto) {
                        self.novos.push((k, texto.clone()));
                        s.push(acao(agora, nick, tipo, texto, *ok));
                    }
                }
                _ => {}
            }
        }
        s
    }

    /// As ações de [`Registros::das_decisoes`] foram gravadas.
    pub fn anotados(&mut self) {
        for (k, t) in self.novos.drain(..) {
            self.ultimo.insert(k, t);
        }
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    fn reg(custo: Option<f64>) -> Registro {
        Registro {
            tipo: "compra".into(),
            texto_ok: "Comprou 10 bolas".into(),
            texto_falha: "Falhou ao comprar ({r}).".into(),
            custo,
        }
    }

    #[test]
    fn resultado_vira_acao_com_custo_e_falha_com_a_resposta() {
        let mut r = Registros::default();
        r.aguardar(
            0,
            vec![
                (1, "A".into(), reg(Some(500.0))),
                (2, "B".into(), reg(None)),
            ],
        );
        let a = r.dos_resultados(10, &[(1, "ok".into()), (2, "sem-conexao".into())]);
        assert_eq!(a[0]["custo"], 500.0);
        assert_eq!(a[0]["ok"], true);
        assert_eq!(a[1]["texto"], "Falhou ao comprar (sem-conexao).");
        assert!(a[1].get("custo").is_none());
        assert!(r.ids().is_empty());
        r.aguardar(0, vec![(3, "A".into(), reg(None))]);
        assert!(r.dos_resultados(ESPERA_MS, &[]).is_empty());
        assert!(r.ids().is_empty(), "esperou demais: sai");
    }

    #[test]
    fn registro_repetido_nao_entra_de_novo() {
        let d = |texto: &str| DecisaoDoCiclo {
            nick: "A".into(),
            regra: "motorCompra",
            decisao: Decisao::Registro {
                nick: "A".into(),
                tipo: "aviso".into(),
                texto: texto.into(),
                ok: false,
            },
        };
        let mut r = Registros::default();
        assert_eq!(r.das_decisoes(0, &[d("teto")]).len(), 1);
        assert_eq!(
            r.das_decisoes(0, &[d("teto")]).len(),
            1,
            "não gravou: tenta de novo"
        );
        r.anotados();
        assert!(r.das_decisoes(1, &[d("teto")]).is_empty());
        assert_eq!(r.das_decisoes(2, &[d("outro")]).len(), 1);
        r.anotados();
    }
}
