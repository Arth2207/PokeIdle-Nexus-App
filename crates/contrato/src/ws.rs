//! Mensagens do WebSocket `nexus.v1` (`GET /ws`). O formato está em `docs/websocket.md`.
//!
//! Toda mensagem é um envelope `{v, tipo, seq?, ts, dados}`. Aqui o par `tipo` + `dados` vira um
//! enum (tag adjacente), então `serde` escolhe a variante pelo `tipo` e o app não precisa
//! conferir strings na mão.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Subprotocolo negociado no handshake.
pub const SUBPROTOCOLO: &str = "nexus.v1";
/// Subprotocolo opcional: mensagens grandes do servidor vão como frame binário deflate.
pub const SUBPROTOCOLO_DEFLATE: &str = "nexus.v1.deflate";
/// Versão do envelope (campo `v`).
pub const VERSAO: u8 = 1;

/// O envelope. `M` é [`MensagemServidor`] ou [`MensagemCliente`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Envelope<M> {
    pub v: u8,
    /// Só aparece nas mensagens do servidor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seq: Option<u64>,
    /// Milissegundos desde 1970.
    pub ts: i64,
    #[serde(flatten)]
    pub msg: M,
}

impl<M> Envelope<M> {
    pub fn novo(seq: Option<u64>, ts: i64, msg: M) -> Self {
        Self {
            v: VERSAO,
            seq,
            ts,
            msg,
        }
    }
}

/// Servidor para cliente.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "tipo", content = "dados")]
pub enum MensagemServidor {
    #[serde(rename = "sessao.pronta")]
    SessaoPronta(SessaoPronta),
    #[serde(rename = "comando.novo")]
    ComandoNovo(ComandoNovo),
    #[serde(rename = "comando.cancelado")]
    ComandoCancelado(ComandoCancelado),
    #[serde(rename = "bolsa.atualizada")]
    BolsaAtualizada(Atualizacao),
    #[serde(rename = "alvos.atualizados")]
    AlvosAtualizados(Atualizacao),
    #[serde(rename = "sessao.ressincronizar")]
    SessaoRessincronizar(Ressincronizar),
    /// Resposta a um `evento.lote`.
    #[serde(rename = "evento.ack")]
    EventoAck(EventoAck),
    /// Falha de uma mensagem do cliente (limite de taxa, ingestão desligada, conta proibida).
    #[serde(rename = "erro")]
    Erro(ErroWs),
    /// Informativo (`NEXUS_MELHORIAS`): item raro novo no estado de raros.
    #[serde(rename = "raro.novo")]
    RaroNovo(RaroNovo),
    /// Informativo (`NEXUS_MELHORIAS`): shiny registrado.
    #[serde(rename = "shiny.novo")]
    ShinyNovo(ShinyNovo),
    /// Informativo (`NEXUS_MELHORIAS`): mudança de estado de um comando.
    #[serde(rename = "comando.estado")]
    ComandoEstado(ComandoEstado),
}

/// Cliente para servidor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "tipo", content = "dados")]
pub enum MensagemCliente {
    #[serde(rename = "retomar")]
    Retomar(Retomar),
    #[serde(rename = "comando.resultado")]
    ComandoResultado(ComandoResultado),
    #[serde(rename = "ack")]
    Ack(Ack),
    /// Lote de eventos do jogo, gravado como o `POST /ingest`.
    #[serde(rename = "evento.lote")]
    EventoLote(EventoLote),
}

/// `seq` atual do servidor, para o cliente saber de onde partir.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessaoPronta {
    pub seq: u64,
}

/// Um comando reservado para este cliente (com lease: se não houver resultado a tempo, ele
/// volta à fila e chega de novo).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComandoNovo {
    pub id: i64,
    pub conta: String,
    pub tipo: String,
    pub params: Value,
    pub prioridade: i32,
    /// Instante de expiração em ms; `null` se o comando não expira.
    pub expira_em: Option<i64>,
}

/// Um comando pendente expirou ou foi cancelado.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ComandoCancelado {
    pub id: i64,
    pub conta: String,
    /// Hoje só `expirado`.
    pub motivo: String,
}

/// Aviso de `bolsa.atualizada` e `alvos.atualizados`: quando a mudança aconteceu (ms).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Atualizacao {
    pub em: i64,
}

/// O `seq` pedido saiu do buffer: recarregar o estado pelas rotas HTTP.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ressincronizar {
    pub motivo: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Retomar {
    /// Último `seq` que o cliente recebeu.
    pub desde: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ComandoResultado {
    pub id: i64,
    pub resultado: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ack {
    pub seq: u64,
}

/// Lote de eventos. `seq_cliente` identifica o lote: reenviar o mesmo lote com o mesmo
/// `seq_cliente` não grava de novo.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventoLote {
    pub seq_cliente: u64,
    /// Conta (nick) do lote. Opcional em token preso a uma conta; obrigatória em token global.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conta: Option<String>,
    pub eventos: Vec<Value>,
}

/// Resposta a um `evento.lote`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventoAck {
    pub seq_cliente: u64,
    pub ok: bool,
    /// Eventos que já estavam gravados.
    pub duplicados: u64,
    /// Eventos rejeitados pela validação.
    pub erros: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ErroWs {
    /// `limite_taxa`, `ingestao_desligada`, `conta_proibida`, `conta_obrigatoria`,
    /// `banco_indisponivel`.
    pub motivo: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seq_cliente: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RaroNovo {
    pub id: Value,
    pub nome: Value,
    /// Conta dona do item; `null` se o item não tem conta (só tokens globais recebem).
    pub conta: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShinyNovo {
    pub nick: String,
    pub dia: String,
    /// Quantos shiny entraram neste registro.
    pub quantidade: f64,
    /// Total do dia depois do registro.
    pub total_dia: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ComandoEstado {
    pub id: i64,
    pub conta: String,
    /// `criado`, `entregue`, `confirmado` ou `expirado`.
    pub estado: String,
}

#[cfg(test)]
mod testes {
    use super::*;
    use serde_json::json;

    fn ida_e_volta<M>(env: &Envelope<M>, esperado: &Value)
    where
        M: Serialize + for<'a> Deserialize<'a> + PartialEq + std::fmt::Debug,
    {
        assert_eq!(&serde_json::to_value(env).unwrap(), esperado);
        let de: Envelope<M> = serde_json::from_value(esperado.clone()).unwrap();
        assert_eq!(&de, env);
    }

    #[test]
    fn comando_novo_json_exato() {
        let env = Envelope::novo(
            Some(18234),
            1_762_866_000_000,
            MensagemServidor::ComandoNovo(ComandoNovo {
                id: 7,
                conta: "ash".into(),
                tipo: "hunt".into(),
                params: json!({"slug": "x"}),
                prioridade: 5,
                expira_em: Some(1_762_866_300_000),
            }),
        );
        ida_e_volta(
            &env,
            &json!({
                "v": 1, "tipo": "comando.novo", "seq": 18234, "ts": 1_762_866_000_000_i64,
                "dados": {"id": 7, "conta": "ash", "tipo": "hunt", "params": {"slug": "x"},
                          "prioridade": 5, "expiraEm": 1_762_866_300_000_i64}
            }),
        );
    }

    #[test]
    fn mensagens_do_servidor() {
        let ts = 1_762_866_000_000_i64;
        ida_e_volta(
            &Envelope::novo(
                Some(1),
                ts,
                MensagemServidor::SessaoPronta(SessaoPronta { seq: 1 }),
            ),
            &json!({"v":1,"tipo":"sessao.pronta","seq":1,"ts":ts,"dados":{"seq":1}}),
        );
        ida_e_volta(
            &Envelope::novo(
                Some(2),
                ts,
                MensagemServidor::ComandoCancelado(ComandoCancelado {
                    id: 3,
                    conta: "ash".into(),
                    motivo: "expirado".into(),
                }),
            ),
            &json!({"v":1,"tipo":"comando.cancelado","seq":2,"ts":ts,
                    "dados":{"id":3,"conta":"ash","motivo":"expirado"}}),
        );
        ida_e_volta(
            &Envelope::novo(
                Some(3),
                ts,
                MensagemServidor::BolsaAtualizada(Atualizacao { em: ts }),
            ),
            &json!({"v":1,"tipo":"bolsa.atualizada","seq":3,"ts":ts,"dados":{"em":ts}}),
        );
        ida_e_volta(
            &Envelope::novo(
                Some(4),
                ts,
                MensagemServidor::AlvosAtualizados(Atualizacao { em: ts }),
            ),
            &json!({"v":1,"tipo":"alvos.atualizados","seq":4,"ts":ts,"dados":{"em":ts}}),
        );
        ida_e_volta(
            &Envelope::novo(
                None,
                ts,
                MensagemServidor::SessaoRessincronizar(Ressincronizar {
                    motivo: "fora_do_buffer".into(),
                }),
            ),
            &json!({"v":1,"tipo":"sessao.ressincronizar","ts":ts,
                    "dados":{"motivo":"fora_do_buffer"}}),
        );
    }

    #[test]
    fn mensagens_do_cliente_sem_seq() {
        let ts = 5_i64;
        ida_e_volta(
            &Envelope::novo(None, ts, MensagemCliente::Retomar(Retomar { desde: 40 })),
            &json!({"v":1,"tipo":"retomar","ts":ts,"dados":{"desde":40}}),
        );
        ida_e_volta(
            &Envelope::novo(
                None,
                ts,
                MensagemCliente::ComandoResultado(ComandoResultado {
                    id: 9,
                    resultado: "ok".into(),
                }),
            ),
            &json!({"v":1,"tipo":"comando.resultado","ts":ts,"dados":{"id":9,"resultado":"ok"}}),
        );
        ida_e_volta(
            &Envelope::novo(None, ts, MensagemCliente::Ack(Ack { seq: 12 })),
            &json!({"v":1,"tipo":"ack","ts":ts,"dados":{"seq":12}}),
        );
    }

    #[test]
    fn evento_lote_e_ack() {
        let ts = 7_i64;
        ida_e_volta(
            &Envelope::novo(
                None,
                ts,
                MensagemCliente::EventoLote(EventoLote {
                    seq_cliente: 3,
                    conta: Some("ash".into()),
                    eventos: vec![json!({"k": "kill"})],
                }),
            ),
            &json!({"v":1,"tipo":"evento.lote","ts":ts,
                    "dados":{"seq_cliente":3,"conta":"ash","eventos":[{"k":"kill"}]}}),
        );
        ida_e_volta(
            &Envelope::novo(
                Some(1),
                ts,
                MensagemServidor::EventoAck(EventoAck {
                    seq_cliente: 3,
                    ok: true,
                    duplicados: 1,
                    erros: 0,
                }),
            ),
            &json!({"v":1,"tipo":"evento.ack","seq":1,"ts":ts,
                    "dados":{"seq_cliente":3,"ok":true,"duplicados":1,"erros":0}}),
        );
    }

    #[test]
    fn tipo_desconhecido_falha() {
        let r: Result<Envelope<MensagemCliente>, _> =
            serde_json::from_value(json!({"v":1,"tipo":"x.y","ts":1,"dados":{}}));
        assert!(r.is_err());
    }
}
