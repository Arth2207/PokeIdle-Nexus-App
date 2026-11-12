//! Configuração por conta do motor: `CONFIG_PADRAO`, `cfgBase`, `cfg` e a carga do `D`.

use serde_json::{json, Map, Value};

/// `CONFIG_PADRAO` do `motor.js`, na mesma ordem de chaves.
pub fn padrao() -> Value {
    json!({
        "compra": { "ativo": false, "auto": true, "bolaId": null, "bolaMin": 300, "bolaLote": 500, "pocaoId": null, "pocaoMin": 200, "pocaoLote": 300, "reservaOuro": 1_000_000, "maxGastoHora": 500_000 },
        "hunt": { "ativo": false, "soOutland": false, "explorar": true, "margem": 0.15, "intervaloMin": 10, "maxPiorPct": 0.4, "objetivo": "xp" },
        "pocao": { "ativo": true },
        "gemaPrecos": { "shiny": 50, "bronzeToken": 300, "fragChave": 5, "fragShinyStone": 5, "fragBicicleta": 5, "outlandBonus": 1.0 },
        "retomar": { "ativo": false, "minutos": 5 },
        "recarregar": { "ativo": false, "minutos": 10 },
        "zerar": { "ativo": true },
        "alertas": { "ativo": true, "shiny": true, "desmaio": true, "parada": true, "suprimentos": true, "sessao": true, "mercado": true },
        "mercadoCompra": { "ativo": false, "tetoCoins": 0, "tetoGemas": 0, "preferirCoins": true, "minNivel": 1, "minIvTotal": 100, "minQualidade": 1.3, "minPotencia": 3 },
        "rodizio": { "ativo": false, "alvoBoss": "", "extra": 0, "metas": {}, "intervaloMin": 2, "capturar": true, "minQualidade": 0, "minIvTotal": 0, "outland": false, "repetirEspecie": "nao", "melhorar": true, "alvoPct": 0.5, "melhorarMaxMin": 0 },
        "pedras": { "ativo": false, "reserva": 0, "intervaloMin": 30 },
        "radar": { "ativo": true, "altaPct": 20, "metaGemas": 1000 },
        "papel": { "tipo": "", "off": {} },
        "venderNpc": { "v": 3, "ativo": false, "vender": true, "intervaloMin": 15, "minIvTotal": 0, "minQualidade": 0, "minPotencia": 0, "sempreGuardarShiny": true, "sempreGuardarDesejo": true, "substituirMelhor": true, "maxPctDepot": 60, "maxPorRodada": 200, "maxMoverRodada": 30, "confirmado": "" }
    })
}

/// Só estas contas montam equipe e rodízio de boss (`CONTAS_BOSS`).
const CONTAS_BOSS: &[&str] = &["Alllphhha"];

/// `PAPEL_EFEITO`: o que cada papel liga e ajusta.
fn efeito_do_papel(tipo: &str) -> Option<Value> {
    match tipo {
        "farm" => Some(json!({
            "hunt": { "ativo": true, "objetivo": "equilibrio", "soOutland": false },
            "rodizio": { "ativo": true }
        })),
        "espolio" => Some(json!({
            "hunt": { "ativo": true, "objetivo": "gemas", "soOutland": true },
            "venderNpc": { "ativo": true }
        })),
        "shiny" => {
            Some(json!({ "hunt": { "ativo": true, "objetivo": "shiny", "soOutland": false } }))
        }
        _ => None,
    }
}

/// `{ ...a, ...b }` de dois objetos (o que não for objeto não acrescenta chaves).
fn espalhar(a: Option<&Value>, b: Option<&Value>) -> Value {
    let mut o: Map<String, Value> = a.and_then(Value::as_object).cloned().unwrap_or_default();
    if let Some(Value::Object(m)) = b {
        for (k, v) in m {
            o.insert(k.clone(), v.clone());
        }
    }
    Value::Object(o)
}

/// `cfgBase(nick)`: a config gravada da conta por cima do padrão, sem os efeitos do papel.
pub fn base(d: &Value, nick: &str) -> Value {
    let gravada = d.pointer("/config").and_then(|c| c.get(nick));
    let padrao = padrao();
    let mut o = Map::new();
    if let Value::Object(p) = &padrao {
        for (k, pk) in p {
            let da_conta = gravada
                .and_then(|c| c.get(k))
                .filter(|v| nucleo_verdadeiro(v));
            o.insert(k.clone(), espalhar(Some(pk), da_conta));
        }
    }
    Value::Object(o)
}

fn nucleo_verdadeiro(v: &Value) -> bool {
    crate::js::verdadeiro(Some(v))
}

/// `cfg(nick)`: [`base`] com os efeitos do papel e o rodízio só nas contas de boss.
pub fn efetiva(d: &Value, nick: &str) -> Value {
    let mut o = base(d, nick);
    let rodizio_voce = o.pointer("/rodizio/ativo") == Some(&Value::Bool(true));
    let tipo = crate::js::texto(o.pointer("/papel/tipo"));
    let papel_verdadeiro = crate::js::verdadeiro(o.get("papel"));
    if let (true, Some(Value::Object(ef))) = (papel_verdadeiro, efeito_do_papel(&tipo)) {
        let off = o
            .pointer("/papel/off")
            .filter(|v| nucleo_verdadeiro(v))
            .cloned()
            .unwrap_or_else(|| json!({}));
        for (k, e) in ef {
            let mut e = e.as_object().cloned().unwrap_or_default();
            if crate::js::verdadeiro(off.get(&k)) {
                e.shift_remove("ativo");
            }
            let novo = espalhar(o.get(&k), Some(&Value::Object(e)));
            if let Value::Object(m) = &mut o {
                m.insert(k, novo);
            }
        }
    }
    let de_boss = CONTAS_BOSS
        .iter()
        .any(|n| n.to_lowercase() == nick.to_lowercase());
    if !de_boss && !rodizio_voce {
        let novo = espalhar(o.get("rodizio"), Some(&json!({ "ativo": false })));
        if let Value::Object(m) = &mut o {
            m.insert("rodizio".into(), novo);
        }
    }
    o
}

/// O `D` como o legado o monta ao subir: os padrões de topo, o arquivo por cima
/// (`Object.assign`) e a migração da `venderNpc` antiga para a versão 3.
pub fn carregar(arquivo: &Value) -> Value {
    let mut d = match json!({
        "config": {}, "acoes": [], "calib2": {}, "medidas": {}, "bloqueio": {}, "alertas": {},
        "dia": {}, "builds": {}, "mercadoAvisos": [], "mercadoRejeitado": []
    }) {
        Value::Object(m) => m,
        _ => Map::new(),
    };
    if let Value::Object(a) = arquivo {
        for (k, v) in a {
            d.insert(k.clone(), v.clone());
        }
    }
    if let Some(Value::Object(config)) = d.get_mut("config") {
        for secoes in config.values_mut() {
            let Some(Value::Object(v)) = secoes.get_mut("venderNpc") else {
                continue;
            };
            if v.get("v") != Some(&json!(3)) {
                for (k, x) in [
                    ("v", json!(3)),
                    ("minIvTotal", json!(0)),
                    ("minQualidade", json!(0)),
                    ("minPotencia", json!(0)),
                    ("vender", json!(true)),
                    ("confirmado", json!("")),
                ] {
                    v.insert(k.into(), x);
                }
            }
        }
    }
    Value::Object(d)
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn base_mescla_por_secao_na_ordem_do_padrao() {
        let d = json!({"config": {"Ana": {"hunt": {"ativo": true, "extra": 1}, "radar": 0}}});
        let b = base(&d, "Ana");
        assert_eq!(b["hunt"]["ativo"], json!(true));
        assert_eq!(b["hunt"]["objetivo"], json!("xp"));
        let chaves: Vec<&String> = b["hunt"]
            .as_object()
            .map(|m| m.keys().collect())
            .unwrap_or_default();
        assert_eq!(chaves.last().map(|s| s.as_str()), Some("extra"));
        assert_eq!(b["radar"]["metaGemas"], json!(1000));
        let topo: Vec<&String> = b
            .as_object()
            .map(|m| m.keys().collect())
            .unwrap_or_default();
        assert_eq!(topo.first().map(|s| s.as_str()), Some("compra"));
        assert_eq!(base(&json!({}), "x")["pocao"], json!({"ativo": true}));
    }

    #[test]
    fn papel_liga_automacoes_e_respeita_off() {
        let d = json!({"config": {"Ana": {"papel": {"tipo": "espolio", "off": {"venderNpc": 1}}}}});
        let c = efetiva(&d, "Ana");
        assert_eq!(c["hunt"]["objetivo"], json!("gemas"));
        assert_eq!(c["hunt"]["ativo"], json!(true));
        assert_eq!(c["venderNpc"]["ativo"], json!(false));
        // Conta que não é de boss: rodízio sempre desligado, mesmo com o papel farm.
        let f = json!({"config": {"Ana": {"papel": {"tipo": "farm"}}}});
        assert_eq!(efetiva(&f, "Ana")["rodizio"]["ativo"], json!(false));
        let boss = json!({"config": {"alllphhha": {"papel": {"tipo": "farm"}}}});
        assert_eq!(efetiva(&boss, "alllphhha")["rodizio"]["ativo"], json!(true));
    }

    #[test]
    fn carga_migra_vender_npc() {
        let d = carregar(
            &json!({"config": {"A": {"venderNpc": {"v": 2, "minIvTotal": 9}}}, "pausa": true}),
        );
        assert_eq!(d["config"]["A"]["venderNpc"]["minIvTotal"], json!(0));
        assert_eq!(d["config"]["A"]["venderNpc"]["confirmado"], json!(""));
        assert_eq!(d["mercadoAvisos"], json!([]));
        let chaves: Vec<&String> = d
            .as_object()
            .map(|m| m.keys().collect())
            .unwrap_or_default();
        assert_eq!(chaves.last().map(|s| s.as_str()), Some("pausa"));
    }
}
