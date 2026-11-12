//! Fase 4: import do estado do motor de automação e comparação arquivo × banco.
//!
//!   nexus-api importar motor < holodex-dados.json
//!   nexus-api importar motor-contas < contas.json
//!   nexus-api comparar-motor motor < holodex-dados.json
//!
//! Só lê os arquivos do legado (pelo stdin). A chave `acesso` não entra no banco nem no log.
//! A comparação confere valores e a ordem das chaves (o legado itera na ordem do arquivo).

use anyhow::{bail, ensure, Context};
use nucleo::motor::leitura;
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
                    o.shift_remove(*k);
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
    let diferenca = repo::primeira_diferenca(&arquivo, &banco, "")
        .or_else(|| repo::primeira_diferenca_de_ordem(&arquivo, &banco, ""));
    match diferenca {
        None => {
            println!("{tipo}: arquivo e banco iguais");
            Ok(())
        }
        Some(d) => bail!("{tipo}: arquivo e banco diferem em {d}"),
    }
}

/// Rotas `GET` do motor servidas pelo Nexus (etapa 3), com o caminho do legado.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rota {
    Builds,
    BuildsPrioridade,
    BuildsSugerir,
    BuildsTeorica,
    EquipeMelhor,
    EquipeMelhorTodas,
    Raros,
    Espolio,
    PedrasPlano,
    PedrasDiag,
    VenderPrevia,
    Historico,
    Financeiro,
    Combate,
    CacheLinhas,
    Diagnostico,
    Estado,
    Backup,
}

impl Rota {
    pub const TODAS: [Rota; 18] = [
        Rota::Builds,
        Rota::BuildsPrioridade,
        Rota::BuildsSugerir,
        Rota::BuildsTeorica,
        Rota::EquipeMelhor,
        Rota::EquipeMelhorTodas,
        Rota::Raros,
        Rota::Espolio,
        Rota::PedrasPlano,
        Rota::PedrasDiag,
        Rota::VenderPrevia,
        Rota::Historico,
        Rota::Financeiro,
        Rota::Combate,
        Rota::CacheLinhas,
        Rota::Diagnostico,
        Rota::Estado,
        Rota::Backup,
    ];

    pub fn caminho(self) -> &'static str {
        match self {
            Rota::Builds => "/api/builds",
            Rota::BuildsPrioridade => "/api/builds/prioridade",
            Rota::BuildsSugerir => "/api/builds/sugerir",
            Rota::BuildsTeorica => "/api/builds/teorica",
            Rota::EquipeMelhor => "/api/equipe/melhor",
            Rota::EquipeMelhorTodas => "/api/equipe/melhor/todas",
            Rota::Raros => "/api/raros",
            Rota::Espolio => "/api/espolio",
            Rota::PedrasPlano => "/api/pedras/plano",
            Rota::PedrasDiag => "/api/pedras/diag",
            Rota::VenderPrevia => "/api/vender/previa",
            Rota::Historico => "/api/historico",
            Rota::Financeiro => "/api/financeiro",
            Rota::Combate => "/api/combate",
            Rota::CacheLinhas => "/api/cache-linhas",
            Rota::Diagnostico => "/api/diagnostico",
            Rota::Estado => "/api/estado",
            Rota::Backup => "/api/backup",
        }
    }

    /// Nome no comparador (`motor.historico`, `motor.pedras/plano`...).
    pub fn nome(self) -> &'static str {
        match self {
            Rota::Builds => "motor.builds",
            Rota::BuildsPrioridade => "motor.builds/prioridade",
            Rota::BuildsSugerir => "motor.builds/sugerir",
            Rota::BuildsTeorica => "motor.builds/teorica",
            Rota::EquipeMelhor => "motor.equipe/melhor",
            Rota::EquipeMelhorTodas => "motor.equipe/melhor/todas",
            Rota::Raros => "motor.raros",
            Rota::Espolio => "motor.espolio",
            Rota::PedrasPlano => "motor.pedras/plano",
            Rota::PedrasDiag => "motor.pedras/diag",
            Rota::VenderPrevia => "motor.vender/previa",
            Rota::Historico => "motor.historico",
            Rota::Financeiro => "motor.financeiro",
            Rota::Combate => "motor.combate",
            Rota::CacheLinhas => "motor.cache-linhas",
            Rota::Diagnostico => "motor.diagnostico",
            Rota::Estado => "motor.estado",
            Rota::Backup => "motor.backup",
        }
    }

    /// A rota recebe `nick` e responde só sobre aquela conta.
    pub fn por_conta(self) -> bool {
        matches!(
            self,
            Rota::Builds
                | Rota::BuildsPrioridade
                | Rota::BuildsSugerir
                | Rota::BuildsTeorica
                | Rota::EquipeMelhor
                | Rota::PedrasDiag
                | Rota::VenderPrevia
                | Rota::Combate
        )
    }
}

/// Calcula a resposta de uma rota do motor. `nick` é o parâmetro da URL (`None` se ausente).
/// `ErroInterno` é o 500 do legado; erro de banco sobe como `Err`.
pub async fn responder(
    db: &PgPool,
    fuso_ms: i64,
    agora_ms: i64,
    rota: Rota,
    nick: Option<&str>,
) -> Result<Result<leitura::Resposta, leitura::ErroInterno>, repo::ErroMotor> {
    let relogio = nucleo::motor::Relogio { agora_ms, fuso_ms };
    // Sem conta conectada, estas não leem o estado guardado.
    let sem_estado = match rota {
        Rota::BuildsPrioridade
        | Rota::BuildsSugerir
        | Rota::BuildsTeorica
        | Rota::EquipeMelhor
        | Rota::PedrasDiag
        | Rota::VenderPrevia => Some(leitura::sem_conta()),
        Rota::EquipeMelhorTodas => Some(leitura::equipe_melhor_todas()),
        Rota::Espolio => Some(leitura::espolio()),
        Rota::PedrasPlano => Some(leitura::pedras_plano()),
        Rota::Combate => Some(leitura::combate()),
        Rota::Diagnostico => Some(leitura::diagnostico()),
        _ => None,
    };
    if let Some(r) = sem_estado {
        return Ok(Ok(r));
    }
    let d = nucleo::motor::config::carregar(&repo::carregar_dados(db).await?);
    Ok(match rota {
        Rota::Builds => leitura::builds(&d, nick),
        Rota::Raros => Ok(leitura::raros(&d, &relogio)),
        Rota::Historico => leitura::historico(&d, &relogio),
        Rota::Financeiro => leitura::financeiro(&d, &relogio),
        Rota::CacheLinhas => leitura::cache_linhas(&d),
        Rota::Estado => leitura::estado(&d, &relogio),
        Rota::Backup => Ok(leitura::backup(&d)),
        _ => Ok(leitura::sem_conta()),
    })
}
