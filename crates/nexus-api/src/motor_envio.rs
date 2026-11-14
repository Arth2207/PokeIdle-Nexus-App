//! Fase 4 com escrita: o que do ciclo do motor vai de fato para a fila de comandos do app.
//!
//! - **Conta:** as regras falam o nick do jogo (`Alllphhha`); o app executa pelo rótulo da
//!   conta dele (`alllphha`), que vem na janela como `id`.
//! - **Sem repetir:** o motor do Nexus não vê a resposta do jogo na hora (a janela chega a cada
//!   30 s). O mesmo comando na mesma conta não sai de novo antes de [`REPETIR_MS`]; na compra,
//!   vale o item, qualquer que seja a quantidade (não compra duas vezes enquanto a janela não
//!   mostra a primeira).

use std::collections::HashMap;

use serde_json::Value;

pub const REPETIR_MS: i64 = 3 * 60_000;

#[derive(Default)]
pub struct Envio {
    /// chave do comando → quando saiu.
    enviados: HashMap<String, i64>,
}

/// nick do jogo (minúsculo) → rótulo da conta no app, das contas da janela.
pub fn contas_por_nick(janela: &Value) -> HashMap<String, String> {
    janela
        .get("contas")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|c| {
            let nick = c.get("nick").and_then(Value::as_str)?;
            let id = c.get("id").and_then(Value::as_str)?;
            Some((nick.to_lowercase(), id.to_owned()))
        })
        .collect()
}

fn chave(conta: &str, tipo: &str, params: &Value) -> String {
    let mut p = params.clone();
    if let Value::Object(o) = &mut p {
        o.remove("qty");
    }
    format!("{conta}|{tipo}|{p}")
}

impl Envio {
    /// `(nick, tipo, params)` do ciclo → os que podem sair agora: `(índice em cmds, conta do
    /// app)`.
    pub fn filtrar(
        &mut self,
        agora: i64,
        contas: &HashMap<String, String>,
        cmds: &[(String, String, Value)],
    ) -> Vec<(usize, String)> {
        self.enviados.retain(|_, t| agora - *t < REPETIR_MS);
        let mut s = Vec::new();
        for (i, (nick, tipo, params)) in cmds.iter().enumerate() {
            let conta = contas
                .get(&nick.to_lowercase())
                .cloned()
                .unwrap_or_else(|| nick.clone());
            let k = chave(&conta, tipo, params);
            if self.enviados.contains_key(&k) {
                continue;
            }
            self.enviados.insert(k, agora);
            s.push((i, conta));
        }
        s
    }
}

#[cfg(test)]
mod testes {
    use super::*;
    use serde_json::json;

    #[test]
    fn conta_do_app_e_sem_repetir() {
        let janela = json!({"contas": [{"id": "alllphha", "nick": "Alllphhha"}]});
        let contas = contas_por_nick(&janela);
        let compra = |q| {
            (
                "Alllphhha".to_owned(),
                "motor.shop.buy".to_owned(),
                json!({"t": "shop.buy", "id": 4, "qty": q}),
            )
        };
        let mover = |id| {
            (
                "Alllphhha".to_owned(),
                "motor.colecao.mover".to_owned(),
                json!({"t": "colecao.mover", "pokemonId": id}),
            )
        };
        let mut e = Envio::default();
        let s = e.filtrar(0, &contas, &[compra(100), mover(1)]);
        assert_eq!(
            s,
            vec![(0, "alllphha".to_owned()), (1, "alllphha".to_owned())]
        );
        // Ciclo seguinte: mesma compra (outra quantidade) e o mesmo pokémon não saem; outro sai.
        let s = e.filtrar(30_000, &contas, &[compra(90), mover(1), mover(2)]);
        assert_eq!(s, vec![(2, "alllphha".to_owned())], "só o pokémon 2");
        // Passado o tempo, pode de novo. Conta fora da janela segue com o nick.
        let s = e.filtrar(REPETIR_MS, &contas, &[compra(90)]);
        assert_eq!(s.len(), 1);
        let s = e.filtrar(
            REPETIR_MS,
            &contas,
            &[("Outro".into(), "x".into(), json!({}))],
        );
        assert_eq!(s, vec![(0, "Outro".to_owned())]);
    }
}
