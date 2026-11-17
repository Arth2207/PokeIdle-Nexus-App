//! Regras do laço do motor (etapa 4): uma função pura por `motorX` do legado.
//!
//! Cada regra recebe a conta ao vivo (o que a janela do jogo leu), o `D` e o relógio, e
//! devolve a lista de decisões. Nada é executado aqui: quem chama grava as decisões (sombra)
//! ou, com `NEXUS_ESCRITA_MOTOR`, as transforma em comandos para o app.

pub mod compra;
pub mod equipe;
pub mod hunt;
pub mod safari;
pub mod simples;
pub mod vender;

use std::collections::HashMap;

use serde::Serialize;
use serde_json::Value;

use super::{caminho, config, Relogio};
use crate::js;

/// Uma conta conectada, como a rota `GET /api/nexus/janela` do legado a devolve (e, depois,
/// a A2 pelo `/ws`).
#[derive(Debug, Clone, Default)]
pub struct ContaViva {
    pub id: String,
    pub nick: String,
    /// Estimativa semanal carregada pelo Nexus, nunca aceita da janela do cliente.
    pub top_catch: Option<Value>,
    /// `conta.ultimo`: a última leitura da janela (`hdx.eu`, `hdx.pokemons`, ouro, `hunt`...).
    pub ultimo: Value,
    /// `conta.welcome`: hunts, bolas, itens e bosses que o jogo mandou ao conectar.
    pub welcome: Value,
    /// `conta.pub`: dados públicos do jogo (multiplicadores das hunts).
    pub publico: Value,
    pub hunt_key: Option<String>,
    pub sessao_inicio: i64,
    /// `conta.linhas`: `[seq, t, texto]` do HUD, da mais antiga para a mais nova. O Nexus
    /// acumula entre consultas (a rota manda só as últimas 500).
    pub linhas: Vec<(i64, i64, String)>,
    /// `conta.venderBusy` e `conta.venderUltimo` do legado.
    pub vender_ocupado: bool,
    pub vender_ultimo: Value,
    /// `conta.cons.ev`: consumo medido nas últimas 6 h, `[t, item, quanto]`, e
    /// `conta.ouroMax`. Estado do legado; a rota `/api/nexus/janela` manda os dois.
    pub consumo: Vec<compra::Consumo>,
    pub ouro_max: f64,
    /// `conta.huntDesde`: desde quando a conta está na hunt atual.
    pub hunt_desde: i64,
    /// Campos numéricos e lógicos da conta no legado (marcas de tempo, contadores).
    pub campos: serde_json::Map<String, Value>,
    /// Estado de equipe e rodízio do legado (`equipeDesistiu`, `rodizioBloq`...).
    pub objetos: Value,
}

impl ContaViva {
    /// Lê uma conta do JSON da rota `/api/nexus/janela`. O estado do laço (marcas, objetos,
    /// consumo, `ouroMax`, `huntDesde`) vem da foto `antes`, tirada no começo do último ciclo
    /// do legado; sem ela, dos campos de mesmo nome no topo.
    pub fn de_json(v: &Value) -> Option<Self> {
        let texto = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_owned);
        let antes = v.get("antes").filter(|a| a.is_object()).unwrap_or(v);
        // O que o legado leu da janela no começo do ciclo (foto); sem foto, a leitura atual.
        let foto = |k: &str| antes.get(k).filter(|x| !x.is_null()).or_else(|| v.get(k));
        let linhas = v
            .get("linhas")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|l| {
                        Some((
                            l.get(0)?.as_f64()? as i64,
                            l.get(1)?.as_f64()? as i64,
                            l.get(2)?.as_str()?.to_owned(),
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default();
        Some(Self {
            top_catch: None,
            id: texto("id").unwrap_or_default(),
            nick: texto("nick")?,
            ultimo: foto("ultimo").cloned().unwrap_or(Value::Null),
            welcome: v.get("welcome").cloned().unwrap_or(Value::Null),
            publico: foto("pub").cloned().unwrap_or(Value::Null),
            hunt_key: foto("huntKey").and_then(Value::as_str).map(str::to_owned),
            sessao_inicio: v.get("sessaoInicio").and_then(Value::as_i64).unwrap_or(0),
            linhas,
            vender_ocupado: antes
                .pointer("/estado/venderBusy")
                .or_else(|| v.get("venderBusy"))
                .and_then(Value::as_bool)
                .unwrap_or(false),
            vender_ultimo: foto("venderUltimo").cloned().unwrap_or(Value::Null),
            consumo: antes
                .pointer("/cons/ev")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|e| {
                            Some((
                                e.get(0)?.as_f64()? as i64,
                                crate::js::texto(e.get(1)),
                                e.get(2)?.as_f64()?,
                            ))
                        })
                        .collect()
                })
                .unwrap_or_default(),
            ouro_max: antes.get("ouroMax").and_then(Value::as_f64).unwrap_or(0.0),
            hunt_desde: antes
                .get("huntDesde")
                .and_then(Value::as_f64)
                .map_or(0, |x| x as i64),
            campos: antes
                .get("estado")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default(),
            objetos: antes.get("objetos").cloned().unwrap_or(Value::Null),
        })
    }
}

/// O que o legado grava em `acoes` depois de um comando: um texto se o jogo responder `ok`,
/// outro se não. Em `texto_falha`, `{r}` é a resposta do jogo.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Registro {
    pub tipo: String,
    pub texto_ok: String,
    pub texto_falha: String,
    /// Ouro gasto se o jogo responder `ok` (compra): entra na ação, para o teto por hora.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custo: Option<f64>,
}

/// O que uma regra decidiu. Em sombra, só é gravado.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "decisao", rename_all = "camelCase")]
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
    /// `comando(conta, obj)`: mensagem para o jogo (`__hdxCmd`), com o registro que segue.
    Comando {
        nick: String,
        comando: Value,
        registro: Option<Registro>,
    },
    /// `registrar(nick, tipo, texto, ok)` que não depende de resposta do jogo.
    Registro {
        nick: String,
        tipo: String,
        texto: String,
        ok: bool,
    },
    /// `recarregarPainel(conta.id)`.
    RecarregarPainel { nick: String },
    /// `trocarHunt(conta, slug, nome, nivel)`, com o registro que segue.
    TrocarHunt {
        nick: String,
        slug: String,
        nome: String,
        nivel: Value,
        registro: Option<Registro>,
    },
}

/// Estado do laço que sobrevive entre ciclos (no legado, variáveis do módulo e campos
/// soltos no objeto da conta).
#[derive(Debug, Default, Clone)]
pub struct EstadoLaco {
    /// `avisado`: último alerta por `nick|chave`.
    pub avisado: HashMap<String, i64>,
    /// `conta.<campo>` numéricos que as regras gravam (`pocaoMexida`, `bolaReligadaEm`,
    /// `pedrasEm`...), por `nick` e nome do campo do legado. Ausente vale `undefined`.
    pub marcas: HashMap<(String, String), f64>,
    /// `ultimaHunt`: a última hunt vista por conta (`{nome, lvl, slug, t}`).
    pub ultima_hunt: HashMap<String, Value>,
    /// O Nexus é o dono do laço (Fase 4 sem o legado): os campos que a janela traz entram sem
    /// apagar as marcas que as regras gravaram.
    pub proprio: bool,
}

impl EstadoLaco {
    /// Para guardar no banco (sobrevive a reinícios): marcas finitas, alertas e última hunt.
    pub fn para_json(&self) -> Value {
        let mut marcas: Vec<_> = self
            .marcas
            .iter()
            .filter(|(_, v)| v.is_finite())
            .map(|((n, c), v)| serde_json::json!([n, c, v]))
            .collect();
        marcas.sort_by_key(ToString::to_string);
        serde_json::json!({"marcas": marcas, "avisado": self.avisado, "ultimaHunt": self.ultima_hunt})
    }

    /// Volta o que [`EstadoLaco::para_json`] guardou (mantém `proprio`).
    pub fn restaurar(&mut self, v: &Value) {
        for m in v
            .get("marcas")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let (Some(n), Some(c), Some(x)) = (
                m.get(0).and_then(Value::as_str),
                m.get(1).and_then(Value::as_str),
                m.get(2).and_then(Value::as_f64),
            ) {
                self.marcar(n, c, x);
            }
        }
        for (k, t) in v
            .get("avisado")
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
        {
            if let Some(t) = t.as_i64() {
                self.avisado.insert(k.clone(), t);
            }
        }
        for (k, h) in v
            .get("ultimaHunt")
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
        {
            self.ultima_hunt.insert(k.clone(), h.clone());
        }
    }

    /// `conta.<campo> || 0`.
    pub fn marca(&self, nick: &str, campo: &str) -> f64 {
        self.marcas
            .get(&(nick.to_owned(), campo.to_owned()))
            .copied()
            .unwrap_or(0.0)
    }

    pub fn marcar(&mut self, nick: &str, campo: &str, valor: f64) {
        self.marcas
            .insert((nick.to_owned(), campo.to_owned()), valor);
    }

    /// Em sombra, o estado do laço do legado vale a cada ciclo: as marcas da conta passam a
    /// ser os campos numéricos e lógicos que a rota `janela` mandou (`true` = 1).
    pub fn adotar_campos(&mut self, nick: &str, campos: &serde_json::Map<String, Value>) {
        if !self.proprio {
            self.marcas.retain(|(n, _), _| n != nick);
        }
        for (k, v) in campos {
            let x = match v {
                Value::Bool(b) => f64::from(u8::from(*b)),
                outro => outro.as_f64().unwrap_or(f64::NAN),
            };
            self.marcar(nick, k, x);
        }
    }
}

/// `alerta()`: aplica a config de alertas da conta e o intervalo mínimo ao pedido. Devolve a
/// decisão quando o legado de fato avisaria, e marca o horário.
pub fn alerta(d: &Value, laco: &mut EstadoLaco, r: &Relogio, pedido: Decisao) -> Option<Decisao> {
    let Decisao::Alerta {
        nick,
        chave,
        minutos,
        ..
    } = &pedido
    else {
        return Some(pedido);
    };
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

/// Preserva uma sessão paga em vez de trocar sua área automaticamente.
pub fn safari_pago(conta: &ContaViva) -> bool {
    conta.ultimo.get("safari").is_some_and(|v| match v {
        Value::Null | Value::Bool(false) => false,
        Value::Number(n) => n.as_f64().is_some_and(|n| n > 0.0),
        _ => true,
    })
}

#[cfg(test)]
mod testes {
    use super::*;
    use serde_json::json;

    #[test]
    fn laco_proprio_mescla_campos_e_volta_do_banco() {
        let campos = json!({"sessaoInicio": 5, "venderBusy": false});
        let campos = campos.as_object().cloned().unwrap_or_default();
        let mut sombra = EstadoLaco::default();
        sombra.marcar("A", "mediasEm", 100.0);
        sombra.adotar_campos("A", &campos);
        assert_eq!(
            sombra.marca("A", "mediasEm"),
            0.0,
            "em sombra a janela do legado manda"
        );
        let mut l = EstadoLaco {
            proprio: true,
            ..EstadoLaco::default()
        };
        l.marcar("A", "mediasEm", 100.0);
        l.adotar_campos("A", &campos);
        assert_eq!(
            l.marca("A", "mediasEm"),
            100.0,
            "o Nexus guarda as suas marcas"
        );
        assert_eq!(l.marca("A", "sessaoInicio"), 5.0);
        l.avisado.insert("A|bolas".into(), 7);
        l.ultima_hunt.insert("A".into(), json!({"slug": "x"}));
        let mut volta = EstadoLaco::default();
        volta.restaurar(&l.para_json());
        assert_eq!(volta.marca("A", "mediasEm"), 100.0);
        assert_eq!(volta.avisado.get("A|bolas"), Some(&7));
        assert_eq!(volta.ultima_hunt.get("A"), Some(&json!({"slug": "x"})));
    }

    #[test]
    fn de_json_prefere_a_foto_do_comeco_do_ciclo() {
        let v = json!({
            "nick": "Ana", "ultimo": {"gold": "1"}, "huntKey": "nova", "venderBusy": true,
            "antes": {"ultimo": {"gold": "2"}, "huntKey": "velha", "estado": {"venderBusy": false}}
        });
        let c = ContaViva::de_json(&v).unwrap();
        assert_eq!(c.ultimo, json!({"gold": "2"}));
        assert_eq!(c.hunt_key.as_deref(), Some("velha"));
        assert!(!c.vender_ocupado);
        // Sem foto (motor antigo), vale a leitura atual.
        let sem = ContaViva::de_json(&json!({"nick": "Ana", "ultimo": {"gold": "1"}})).unwrap();
        assert_eq!(sem.ultimo, json!({"gold": "1"}));
    }

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
