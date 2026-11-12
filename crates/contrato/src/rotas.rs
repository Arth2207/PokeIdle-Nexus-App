//! Corpos e respostas das rotas da fase 3 (ingestão, comandos, raros, boss). O JSON é o do
//! legado: os campos mantêm os nomes de lá, então o app e o relay não notam a troca.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Resposta de `POST /ingest`: quantos eventos entraram, quantos foram rejeitados e o total.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RespostaIngest {
    pub ok: usize,
    pub erros: usize,
    pub total: usize,
}

/// Um item de `POST /api/hunt-sync`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HuntSyncItem {
    pub nick: String,
    pub hunt_slug: String,
}

/// Corpo de `POST /api/hunt-sync`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HuntSync {
    pub eventos: Vec<HuntSyncItem>,
}

/// Resposta de `POST /api/hunt-sync`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RespostaHuntSync {
    pub atualizados: usize,
}

/// Um comando na resposta de `GET /api/comandos?nick=`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ComandoFila {
    pub id: i64,
    pub cmd: String,
    pub params: Value,
    pub origem: String,
}

/// Resposta de `GET /api/comandos?nick=`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RespostaComandos {
    pub nick: String,
    pub comandos: Vec<ComandoFila>,
}

/// Corpo de `POST /api/confirmar-comando`. O `id` pode vir como texto, como no legado.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConfirmarComando {
    pub id: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resultado: Option<String>,
}

/// Corpo de `POST /api/inserir-comando`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InserirComando {
    pub nick: String,
    pub cmd: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origem: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ts_expira_min: Option<f64>,
}

/// Resposta de `POST /api/confirmar-comando` e `POST /api/inserir-comando`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RespostaComandoId {
    pub ok: bool,
    pub id: i64,
}

/// Resposta de `POST /api/raros/ingest`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RespostaRarosIngest {
    pub ok: bool,
    pub itens: usize,
}

/// Resposta de `POST /api/boss/tentativa`: `duplicada` é `true` quando a chave já existia.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RespostaTentativa {
    pub ok: bool,
    pub duplicada: bool,
}

#[cfg(test)]
mod testes {
    use super::*;
    use serde_json::json;

    #[test]
    fn json_das_respostas() {
        let v = serde_json::to_value(RespostaIngest {
            ok: 8,
            erros: 1,
            total: 9,
        })
        .unwrap();
        assert_eq!(v, json!({"ok": 8, "erros": 1, "total": 9}));
        let v = serde_json::to_value(RespostaTentativa {
            ok: true,
            duplicada: false,
        })
        .unwrap();
        assert_eq!(v, json!({"ok": true, "duplicada": false}));
        let v = serde_json::to_value(RespostaComandos {
            nick: "ash".into(),
            comandos: vec![ComandoFila {
                id: 1,
                cmd: "hunt".into(),
                params: json!({}),
                origem: "manual".into(),
            }],
        })
        .unwrap();
        assert_eq!(
            v,
            json!({"nick":"ash","comandos":[{"id":1,"cmd":"hunt","params":{},"origem":"manual"}]})
        );
    }

    #[test]
    fn corpos_ida_e_volta() {
        let c: InserirComando =
            serde_json::from_value(json!({"nick":"ash","cmd":"x","ts_expira_min":5})).unwrap();
        assert_eq!(c.ts_expira_min, Some(5.0));
        assert_eq!(
            serde_json::to_value(&c).unwrap(),
            json!({"nick":"ash","cmd":"x","ts_expira_min":5.0})
        );
        let h: HuntSync =
            serde_json::from_value(json!({"eventos":[{"nick":"a","hunt_slug":"b"}]})).unwrap();
        assert_eq!(h.eventos.len(), 1);
        let r: ConfirmarComando = serde_json::from_value(json!({"id":"12"})).unwrap();
        assert_eq!(r.resultado, None);
    }
}
