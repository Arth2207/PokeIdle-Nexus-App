//! Regras do laço do motor (etapa 4): uma função pura por `motorX` do legado.
//!
//! Cada regra recebe a conta ao vivo (o que a janela do jogo leu), o `D` e o relógio, e
//! devolve a lista de decisões. Nada é executado aqui: quem chama grava as decisões (sombra)
//! ou, com `NEXUS_ESCRITA_MOTOR`, as transforma em comandos para o app.

use std::collections::HashMap;

use serde::Serialize;
use serde_json::Value;

use super::{caminho, config, Relogio};
use crate::js;

/// Uma conta conectada, como a rota `GET /api/nexus/janela` do legado a devolve (e, depois,
/// a A2 pelo `/ws`).
#[derive(Debug, Clone, Default)]
pub struct ContaViva {
    pub nick: String,
    /// `conta.ultimo`: a última leitura da janela (`hdx.eu`, `hdx.pokemons`, ouro...).
    pub ultimo: Value,
    /// `conta.welcome`: hunts, bolas, itens e bosses que o jogo mandou ao conectar.
    pub welcome: Value,
    pub hunt_key: Option<String>,
}

impl ContaViva {
    /// Lê uma conta do JSON da rota `/api/nexus/janela`.
    pub fn de_json(v: &Value) -> Option<Self> {
        Some(Self {
            nick: v.get("nick")?.as_str()?.to_owned(),
            ultimo: v.get("ultimo").cloned().unwrap_or(Value::Null),
            welcome: v.get("welcome").cloned().unwrap_or(Value::Null),
            hunt_key: v.get("huntKey").and_then(Value::as_str).map(str::to_owned),
        })
    }
}

/// O que uma regra decidiu. Em sombra, só é gravado.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "tipo", rename_all = "camelCase")]
pub enum Decisao {
    /// `alerta(nick, chave, titulo, texto, minutos)`: notificação e registro em `acoes`,
    /// no máximo uma vez a cada `minutos` por conta e chave.
    Alerta {
        nick: String,
        chave: String,
        titulo: String,
        texto: String,
        minutos: i64,
    },
}

/// Estado do laço que sobrevive entre ciclos (no legado, variáveis do módulo).
#[derive(Debug, Default, Clone)]
pub struct EstadoLaco {
    /// `avisado`: último alerta por `nick|chave`.
    pub avisado: HashMap<String, i64>,
}

/// `alerta()`: aplica a config de alertas da conta e o intervalo mínimo ao pedido. Devolve a
/// decisão quando o legado de fato avisaria, e marca o horário.
pub fn alerta(d: &Value, laco: &mut EstadoLaco, r: &Relogio, pedido: Decisao) -> Option<Decisao> {
    let Decisao::Alerta {
        nick,
        chave,
        minutos,
        ..
    } = &pedido;
    let c = config::efetiva(d, nick);
    let a = c.get("alertas");
    if !js::verdadeiro(a.and_then(|x| x.get("ativo")))
        || a.and_then(|x| x.get(chave)) == Some(&Value::Bool(false))
    {
        return None;
    }
    let k = format!("{nick}|{chave}");
    if r.agora_ms - laco.avisado.get(&k).copied().unwrap_or(0) < minutos * 60_000 {
        return None;
    }
    laco.avisado.insert(k, r.agora_ms);
    Some(pedido)
}

/// `motorShinyBolas`: na conta de papel Shiny, avisa quando sobram menos de 200 bolas.
pub fn shiny_bolas(
    d: &Value,
    laco: &mut EstadoLaco,
    r: &Relogio,
    conta: &ContaViva,
) -> Vec<Decisao> {
    let base = config::base(d, &conta.nick);
    if base.pointer("/papel/tipo").and_then(Value::as_str) != Some("shiny") {
        return Vec::new();
    }
    let Some(eu) = caminho(&conta.ultimo, &["hdx", "eu"]).filter(|e| js::verdadeiro(Some(e)))
    else {
        return Vec::new();
    };
    // `Object.values(balls).reduce((n, v) => n + (+v || 0), 0)`
    let q: f64 = match eu.get("balls") {
        Some(Value::Object(b)) => b.values().map(|v| super::num_ou_0(Some(v))).sum(),
        _ => 0.0,
    };
    if q >= 200.0 {
        return Vec::new();
    }
    let texto = format!(
        "Só {} bolas. Sem bola boa o shiny que aparecer pode escapar.",
        js::numero(q)
    );
    let pedido = Decisao::Alerta {
        nick: conta.nick.clone(),
        chave: "suprimentos".into(),
        titulo: "Bolas acabando na conta Shiny".into(),
        texto,
        minutos: 60,
    };
    alerta(d, laco, r, pedido).into_iter().collect()
}

#[cfg(test)]
mod testes {
    use super::*;
    use serde_json::json;

    fn conta(bolas: Value) -> ContaViva {
        ContaViva {
            nick: "Ana".into(),
            ultimo: json!({"hdx": {"eu": {"balls": bolas}}}),
            ..Default::default()
        }
    }

    #[test]
    fn shiny_bolas_avisa_uma_vez_por_hora() {
        let d = config::carregar(&json!({"config": {"Ana": {"papel": {"tipo": "shiny"}}}}));
        let mut laco = EstadoLaco::default();
        let mut r = Relogio {
            agora_ms: 10_000_000,
            fuso_ms: 0,
        };
        let c = conta(json!({"1": 120, "2": "30", "3": null}));
        let v = shiny_bolas(&d, &mut laco, &r, &c);
        assert_eq!(
            v,
            [Decisao::Alerta {
                nick: "Ana".into(),
                chave: "suprimentos".into(),
                titulo: "Bolas acabando na conta Shiny".into(),
                texto: "Só 150 bolas. Sem bola boa o shiny que aparecer pode escapar.".into(),
                minutos: 60
            }]
        );
        r.agora_ms += 59 * 60_000;
        assert!(shiny_bolas(&d, &mut laco, &r, &c).is_empty());
        r.agora_ms += 60_000;
        assert_eq!(shiny_bolas(&d, &mut laco, &r, &c).len(), 1);
        assert!(shiny_bolas(&d, &mut laco, &r, &conta(json!({"1": 200}))).is_empty());
    }

    #[test]
    fn shiny_bolas_respeita_papel_e_alertas() {
        let mut laco = EstadoLaco::default();
        let r = Relogio {
            agora_ms: 10_000_000,
            fuso_ms: 0,
        };
        let farm = config::carregar(&json!({"config": {"Ana": {"papel": {"tipo": "farm"}}}}));
        assert!(shiny_bolas(&farm, &mut laco, &r, &conta(json!({}))).is_empty());
        let mudo = config::carregar(&json!({"config": {"Ana": {
            "papel": {"tipo": "shiny"}, "alertas": {"suprimentos": false}}}}));
        assert!(shiny_bolas(&mudo, &mut laco, &r, &conta(json!({}))).is_empty());
    }
}
