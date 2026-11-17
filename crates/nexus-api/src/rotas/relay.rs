//! Controle explícito por conta no painel web. Nenhum token do relay vai ao navegador.
use crate::{auth::Cliente, bolsa::agora_ms, estado::Estado};
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Extension, Json,
};
use persistencia::comandos::{self, NovoComando};
use serde_json::{json, Value};
use sqlx::Row;

fn erro(codigo: StatusCode, texto: &str) -> Response {
    (codigo, Json(json!({"erro":texto}))).into_response()
}

pub async fn estado(State(e): State<Estado>, Extension(c): Extension<Cliente>) -> Response {
    let rows = sqlx::query(
        "SELECT m.conta,(extract(epoch from m.em)*1000)::bigint em,m.janela,m.login,
       (SELECT jsonb_build_object('id',id,'tipo',tipo,'resultado',resultado,
          'criado_em',extract(epoch from criado_em)*1000,
          'expira_em',extract(epoch from expira_em)*1000)
        FROM nexus.comando WHERE conta IN (m.conta,m.janela->>'nick')
        AND tipo='relay.modo' ORDER BY id DESC LIMIT 1) controle,
       (SELECT resultado FROM nexus.comando WHERE conta IN (m.conta,m.janela->>'nick')
        AND executado_em IS NOT NULL ORDER BY executado_em DESC,id DESC LIMIT 1) ultimo
       FROM nexus.motor_janela m WHERE ($1::text IS NULL OR lower(m.conta)=lower($1)
          OR lower(m.janela->>'nick')=lower($1)) ORDER BY m.conta",
    )
    .bind(c.conta.as_deref())
    .fetch_all(&e.db)
    .await;
    let Ok(rows) = rows else {
        return erro(StatusCode::SERVICE_UNAVAILABLE, "controle indisponível");
    };
    let contas: Vec<Value> = rows
        .iter()
        .map(|r| {
            let j: Value = r.get("janela");
            let login: Option<Value> = r.get("login");
            let controle: Option<Value> = r.get("controle");
            let ultimo: Option<String> = r.get("ultimo");
            let nivel = j["ultimo"]["trLv"]
                .as_str()
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0);
            let hunts: Vec<Value> = login
                .as_ref()
                .and_then(|v| v["hunts"].as_array())
                .map(|hs| {
                    hs.iter()
                        .filter(|h| h["nivel"].as_u64().unwrap_or(u64::MAX) <= nivel)
                        .map(|h| json!({"slug":h["slug"],"nome":h["nome"],"nivel":h["nivel"]}))
                        .collect()
                })
                .unwrap_or_default();
            let resultado = controle.as_ref().and_then(|v| v["resultado"].as_str());
            let modo = if ultimo.as_deref() == Some("sombra") {
                "sombra"
            } else {
                match resultado {
                    Some("ok:modo=ativo") => "ativo",
                    Some("ok:modo=sombra") => "sombra",
                    _ => "desconhecido",
                }
            };
            let suporte = match resultado {
                Some("sombra") => "atualizacao_necessaria",
                Some("ok:modo=ativo" | "ok:modo=sombra") => "confirmado",
                _ => "desconhecido",
            };
            let em: i64 = r.get("em");
            json!({"conta":r.get::<String,_>("conta"),"nick":j["nick"],"hunt":j["ultimo"]["hunt"],
           "em":em,"modo":modo,"suporte_remoto":suporte,"controle":controle,"hunts":hunts})
        })
        .collect();
    Json(json!({"contas":contas,"agora":agora_ms()})).into_response()
}

pub async fn controlar(
    State(e): State<Estado>,
    Extension(c): Extension<Cliente>,
    Json(v): Json<Value>,
) -> Response {
    if !e.escrita_comandos {
        return erro(
            StatusCode::SERVICE_UNAVAILABLE,
            "fila de comandos desligada",
        );
    }
    let Some(conta) = v["conta"].as_str().filter(|s| !s.trim().is_empty()) else {
        return erro(StatusCode::BAD_REQUEST, "informe a conta");
    };
    if c.nick_permitido(Some(conta)).is_err() {
        return erro(
            StatusCode::FORBIDDEN,
            "este token só acessa a própria conta",
        );
    }
    let row = sqlx::query("SELECT janela,login FROM nexus.motor_janela WHERE lower(conta)=lower($1) OR lower(janela->>'nick')=lower($1) LIMIT 1")
        .bind(conta).fetch_optional(&e.db).await;
    let Ok(Some(row)) = row else {
        return erro(StatusCode::NOT_FOUND, "conta não encontrada");
    };
    let janela: Value = row.get("janela");
    let nick = janela["nick"].as_str().unwrap_or(conta);
    let (tipo, params) = match v["acao"].as_str() {
        Some("modo") => match v["modo"].as_str() {
            Some(modo @ ("ativo" | "sombra")) => ("relay.modo", json!({"modo":modo})),
            _ => return erro(StatusCode::BAD_REQUEST, "modo deve ser ativo ou sombra"),
        },
        Some("hunt") => {
            let login: Option<Value> = row.get("login");
            let slug = v["slug"].as_str().unwrap_or("");
            let h = login
                .as_ref()
                .and_then(|l| l["hunts"].as_array())
                .and_then(|hs| hs.iter().find(|h| h["slug"].as_str() == Some(slug)));
            let Some(h) = h else {
                return erro(
                    StatusCode::BAD_REQUEST,
                    "hunt não encontrada no catálogo do jogo",
                );
            };
            let nivel = janela["ultimo"]["trLv"]
                .as_str()
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0);
            if h["nivel"].as_u64().unwrap_or(u64::MAX) > nivel {
                return erro(StatusCode::BAD_REQUEST, "hunt acima do nível do treinador");
            }
            (
                "motor.hunt.trocar",
                json!({"slug":h["slug"],"nome":h["nome"],"nivel":h["nivel"]}),
            )
        }
        _ => return erro(StatusCode::BAD_REQUEST, "ação deve ser modo ou hunt"),
    };
    match comandos::inserir(
        &e.db,
        &NovoComando {
            conta: nick,
            tipo,
            params: &params,
            prioridade: 0,
            origem: "painel_web",
            expira_ms: agora_ms() + 120_000,
        },
    )
    .await
    {
        Ok(id) => {
            Json(json!({"ok":true,"id":id,"estado":"aguardando_confirmacao"})).into_response()
        }
        Err(_) => erro(
            StatusCode::SERVICE_UNAVAILABLE,
            "não foi possível enviar o comando",
        ),
    }
}

#[cfg(test)]
mod testes {
    use super::*;
    use crate::auth::Escopo;
    use testcontainers::{
        core::{IntoContainerPort, WaitFor},
        runners::AsyncRunner,
        GenericImage, ImageExt,
    };

    #[tokio::test]
    async fn controle_web_valida_conta_catalogo_e_confirmacao() {
        let pg = GenericImage::new("postgres", "16.4-alpine")
            .with_exposed_port(5432.tcp())
            .with_wait_for(WaitFor::message_on_stderr(
                "database system is ready to accept connections",
            ))
            .with_env_var("POSTGRES_PASSWORD", "teste")
            .start()
            .await
            .unwrap();
        let porta = pg.get_host_port_ipv4(5432).await.unwrap();
        let db = sqlx::PgPool::connect(&format!(
            "postgres://postgres:teste@127.0.0.1:{porta}/postgres"
        ))
        .await
        .unwrap();
        sqlx::raw_sql("CREATE SCHEMA nexus;
          CREATE TABLE nexus.motor_janela(conta text, em timestamptz DEFAULT now(), janela jsonb, login jsonb);
          CREATE TABLE nexus.comando(id bigserial PRIMARY KEY,conta text,tipo text,params jsonb,prioridade int,
            origem text,criado_em timestamptz DEFAULT now(),expira_em timestamptz,chave_idem text,
            executar_em timestamptz,agendado_avisado bool,executado_em timestamptz,resultado text);
          CREATE UNIQUE INDEX comando_idem ON nexus.comando(conta,chave_idem) WHERE chave_idem IS NOT NULL;
          INSERT INTO nexus.motor_janela(conta,janela,login) VALUES
          ('sikien','{\"nick\":\"Sikien\",\"ultimo\":{\"trLv\":\"1108\",\"hunt\":\"Silcoon · nível 650\"}}',
          '{\"hunts\":[{\"slug\":\"snover_sinnoh\",\"nome\":\"Snover\",\"nivel\":1000},{\"slug\":\"alto\",\"nome\":\"Alto\",\"nivel\":2000}]}');")
            .execute(&db).await.unwrap();
        let mut e = Estado::com_pool(db.clone());
        e.escrita_comandos = true;
        let c = Cliente {
            nome: "painel".into(),
            escopos: vec![Escopo::Escrita],
            conta: Some("sikien".into()),
        };
        let resposta = controlar(
            State(e.clone()),
            Extension(c.clone()),
            Json(json!({"conta":"outra","acao":"modo","modo":"ativo"})),
        )
        .await;
        assert_eq!(resposta.status(), StatusCode::FORBIDDEN);
        for corpo in [
            json!({"conta":"sikien","acao":"modo","modo":"errado"}),
            json!({"conta":"sikien","acao":"hunt","slug":"inexistente"}),
            json!({"conta":"sikien","acao":"hunt","slug":"alto"}),
        ] {
            assert_eq!(
                controlar(State(e.clone()), Extension(c.clone()), Json(corpo))
                    .await
                    .status(),
                StatusCode::BAD_REQUEST
            );
        }
        assert_eq!(
            controlar(
                State(e.clone()),
                Extension(c.clone()),
                Json(json!({"conta":"sikien","acao":"modo","modo":"ativo"}))
            )
            .await
            .status(),
            StatusCode::OK
        );
        let nick: String = sqlx::query_scalar("SELECT conta FROM nexus.comando")
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(nick, "Sikien");
        sqlx::query("UPDATE nexus.comando SET resultado='sombra',executado_em=now()")
            .execute(&db)
            .await
            .unwrap();
        let r = estado(State(e.clone()), Extension(c.clone())).await;
        let b = axum::body::to_bytes(r.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let v: Value = serde_json::from_slice(&b).unwrap();
        assert_eq!(v["contas"][0]["suporte_remoto"], "atualizacao_necessaria");
        assert_eq!(v["contas"][0]["modo"], "sombra");
        assert_eq!(v["contas"][0]["hunts"].as_array().unwrap().len(), 1);
        sqlx::query("UPDATE nexus.comando SET resultado='ok:modo=ativo'")
            .execute(&db)
            .await
            .unwrap();
        let r = estado(State(e), Extension(c)).await;
        let b = axum::body::to_bytes(r.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let v: Value = serde_json::from_slice(&b).unwrap();
        assert_eq!(v["contas"][0]["modo"], "ativo");
    }
}
