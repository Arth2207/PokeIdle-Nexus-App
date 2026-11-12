//! Fase 4: import do estado do motor de automação e comparação arquivo × banco.
//!
//!   nexus-api importar motor < holodex-dados.json
//!   nexus-api importar motor-contas < contas.json
//!   nexus-api comparar-motor motor < holodex-dados.json
//!
//! Só lê os arquivos do legado (pelo stdin). A chave `acesso` não entra no banco nem no log.

use anyhow::{bail, ensure, Context};
use persistencia::motor::{self as repo, CHAVES_FORA};
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::PgPool;

fn ler(texto: &str) -> anyhow::Result<(Value, String)> {
    let v: Value = serde_json::from_str(texto).context("JSON inválido")?;
    Ok((v, hex::encode(Sha256::digest(texto.as_bytes()))))
}

pub async fn importar(db: &PgPool, tipo: &str, texto: &str) -> anyhow::Result<()> {
    let (v, sha) = ler(texto)?;
    match tipo {
        "motor" => {
            // Sem as duas chaves a importação apagaria a config ou o histórico do banco.
            ensure!(
                v.as_object()
                    .is_some_and(|o| o.contains_key("config") && o.contains_key("acoes")),
                "arquivo sem `config` ou `acoes`: não parece o holodex-dados.json; nada foi alterado"
            );
            let n = repo::importar_dados(db, &v, &sha).await?;
            println!(
                "estado do motor importado: {} seções de config, {} ações, {} chaves de estado",
                n.config, n.acoes, n.estado
            );
        }
        "motor-contas" => {
            ensure!(
                v.as_array().is_some_and(|a| !a.is_empty()),
                "contas.json vazio; nada foi alterado"
            );
            let n = repo::importar_contas(db, &v, &sha).await?;
            println!("contas do motor importadas: {n}");
        }
        outro => bail!("tipo do motor desconhecido: {outro:?}"),
    }
    Ok(())
}

/// Compara o arquivo com o que o banco devolve. Sai com erro se houver diferença.
pub async fn comparar(db: &PgPool, tipo: &str, texto: &str) -> anyhow::Result<()> {
    let (mut arquivo, _) = ler(texto)?;
    ensure!(
        tipo != "motor" || arquivo.is_object(),
        "o topo do arquivo não é objeto"
    );
    let banco = match tipo {
        "motor" => {
            if let Value::Object(o) = &mut arquivo {
                CHAVES_FORA.iter().for_each(|k| {
                    o.remove(*k);
                });
                // Chaves ausentes no arquivo voltam do banco vazias: compara no mesmo formato.
                o.entry("config")
                    .or_insert_with(|| Value::Object(Default::default()));
                o.entry("acoes").or_insert_with(|| Value::Array(Vec::new()));
            }
            repo::carregar_dados(db).await?
        }
        "motor-contas" => repo::carregar_contas(db).await?,
        outro => bail!("tipo do motor desconhecido: {outro:?}"),
    };
    match repo::primeira_diferenca(&arquivo, &banco, "") {
        None => {
            println!("{tipo}: arquivo e banco iguais");
            Ok(())
        }
        Some(d) => bail!("{tipo}: arquivo e banco diferem em {d}"),
    }
}
