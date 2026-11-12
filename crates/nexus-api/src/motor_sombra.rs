//! Fase 4, etapa 5: o laço do motor em sombra.
//!
//! A cada ciclo, lê do motor do legado o `D` (`GET /api/nexus/dados`) e o que cada janela do
//! jogo leu (`GET /api/nexus/janela`), roda as regras do `nucleo::motor` e grava as decisões em
//! `nexus.motor_decisao`, sem enfileirar comando (`NEXUS_ESCRITA_MOTOR` desligada). As ações
//! novas do legado vão para `nexus.motor_acao_legado`, para o relatório confrontar as duas.
//!
//! Só GETs no legado. As linhas do HUD se acumulam aqui entre ciclos, como o legado faz: a rota
//! manda só as últimas 500, e as regras olham janelas de até uma hora.

use std::collections::HashMap;
use std::time::Duration;

use anyhow::Context;
use nucleo::combate::especie::Pokedex;
use nucleo::motor::regras::{ContaViva, EstadoLaco};
use nucleo::motor::veredito::Especies;
use nucleo::motor::{config, laco, Relogio};
use persistencia::motor::{self as repo, LinhaDecisao};
use serde_json::Value;
use sqlx::PgPool;
use tokio_util::sync::CancellationToken;

/// Linhas do HUD guardadas por conta (o legado guarda 40 mil e corta 10 mil de uma vez).
const LINHAS_MAX: usize = 40_000;
const LINHAS_CORTE: usize = 10_000;

/// Espelha o vetor `conta.linhas` do legado, que só cresce (o `seq` pode recomeçar quando o
/// painel recarrega, mas as linhas antigas ficam). A rota manda as últimas 500: acha nelas a
/// última linha já guardada e acrescenta o que vem depois. Sem sobreposição (ciclo perdido),
/// acrescenta tudo.
fn acumular(guardadas: &mut Vec<(i64, i64, String)>, novas: &[(i64, i64, String)]) {
    let depois = guardadas
        .last()
        .and_then(|u| novas.iter().rposition(|n| n == u))
        .map_or(0, |i| i + 1);
    guardadas.extend(novas[depois..].iter().cloned());
    if guardadas.len() > LINHAS_MAX {
        guardadas.drain(..LINHAS_CORTE);
    }
}

pub struct Sombra {
    url: String,
    http: reqwest::Client,
    fuso_ms: i64,
    /// `NEXUS_ESCRITA_MOTOR`: enfileira os comandos além de gravar as decisões.
    escrita: bool,
    laco: EstadoLaco,
    linhas: HashMap<String, Vec<(i64, i64, String)>>,
    /// Pokédex e preço de NPC por espécie (do `pokedex.json` importado), com o instante da
    /// carga: recarrega a cada hora.
    dex: Option<(Pokedex, HashMap<i64, f64>, i64)>,
    /// Instante do último ciclo do legado já processado: cada ciclo dele roda uma vez aqui.
    ultimo_ciclo: i64,
}

impl Sombra {
    pub fn nova(url: &str, fuso_ms: i64, escrita: bool) -> anyhow::Result<Self> {
        Ok(Self {
            url: url.trim_end_matches('/').to_owned(),
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(25))
                .build()?,
            fuso_ms,
            escrita,
            laco: EstadoLaco::default(),
            linhas: HashMap::new(),
            dex: None,
            ultimo_ciclo: 0,
        })
    }

    async fn ler(&self, caminho: &str) -> anyhow::Result<Value> {
        let r = self
            .http
            .get(format!("{}{caminho}", self.url))
            .send()
            .await
            .with_context(|| format!("GET {caminho}"))?
            .error_for_status()?;
        Ok(r.json().await?)
    }

    async fn pokedex(&mut self, db: &PgPool, agora: i64) -> anyhow::Result<()> {
        if self
            .dex
            .as_ref()
            .is_some_and(|(_, _, em)| agora - em < 3_600_000)
        {
            return Ok(());
        }
        let carga = async {
            let bruta = persistencia::equipes::carregar_pokedex(db).await?;
            let precos = Especies::precos_de_json(&bruta);
            let dex = tokio::task::spawn_blocking(move || Pokedex::de_valor(bruta)).await??;
            anyhow::ensure!(
                !precos.is_empty(),
                "pokédex vazia: rode `nexus-api importar pokedex` antes da sombra do motor"
            );
            Ok((dex, precos))
        };
        match (carga.await, self.dex.as_mut()) {
            (Ok((dex, precos)), _) => self.dex = Some((dex, precos, agora)),
            // Recarga falhou: segue com a pokédex que já tem e tenta de novo daqui a 1 h.
            (Err(erro), Some(atual)) => {
                tracing::warn!(%erro, "não consegui recarregar a pokédex; sigo com a anterior");
                atual.2 = agora;
            }
            (Err(erro), None) => return Err(erro),
        }
        Ok(())
    }

    /// Um ciclo: devolve (decisões gravadas, ações novas do legado).
    pub async fn ciclo(&mut self, db: &PgPool) -> anyhow::Result<(u64, u64)> {
        self.pokedex(db, crate::bolsa::agora_ms()).await?;
        let (dados, janela) =
            tokio::try_join!(self.ler("/api/nexus/dados"), self.ler("/api/nexus/janela"))?;
        // As ações novas do legado entram sempre (o legado guarda só as últimas 300).
        let acoes_todas: Vec<Value> = dados
            .get("acoes")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut novas = repo::gravar_acoes_legado(db, &acoes_todas).await?;
        // O rastro dos comandos sem registro (`mediasEm`, `pedrasEm`...), da foto do ciclo.
        novas += repo::gravar_acoes_legado(db, &laco::marcas_do_legado(&janela)).await?;
        // O ciclo do legado (foto do começo dele): roda uma vez por ciclo, no mesmo instante.
        let agora = janela
            .pointer("/ciclo/t")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        if agora == 0 || agora == self.ultimo_ciclo {
            return Ok((0, novas));
        }
        self.ultimo_ciclo = agora;
        // O `D` como estava no começo do ciclo: sem as ações que o próprio ciclo gravou.
        let mut dados = dados;
        if let Some(Value::Array(a)) = dados.get_mut("acoes") {
            a.retain(|x| x.get("t").and_then(Value::as_i64).is_none_or(|t| t < agora));
        }
        let d = config::carregar(&dados);
        let mut contas: Vec<ContaViva> = janela
            .get("contas")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(ContaViva::de_json).collect())
            .unwrap_or_default();
        // Conta que saiu da janela não guarda mais linhas.
        self.linhas
            .retain(|nick, _| contas.iter().any(|c| &c.nick == nick));
        for c in &mut contas {
            let guardadas = self.linhas.entry(c.nick.clone()).or_default();
            acumular(guardadas, &c.linhas);
            c.linhas.clone_from(guardadas);
        }
        let relogio = Relogio {
            agora_ms: agora,
            fuso_ms: self.fuso_ms,
        };
        if let Some(m) = janela.pointer("/ciclo/avisado").and_then(Value::as_object) {
            // Antiflood dos alertas: o do legado vale (estado do ciclo dele).
            self.laco.avisado = m
                .iter()
                .filter_map(|(k, v)| Some((k.clone(), v.as_f64()? as i64)))
                .collect();
        }
        if let Some(m) = janela
            .pointer("/ciclo/ultimaHunt")
            .and_then(Value::as_object)
        {
            self.laco.ultima_hunt = m.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        }
        let Some((dex, precos, _)) = self.dex.as_ref() else {
            anyhow::bail!("pokédex não carregada");
        };
        let ctx = laco::ContextoCiclo {
            dex,
            precos_npc: precos,
        };
        // Uma regra que entre em pânico não derruba a sombra: o ciclo é perdido e registrado.
        let laco_estado = &mut self.laco;
        let decisoes = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            laco::ciclo(&d, laco_estado, &relogio, &contas, &ctx)
        }))
        .map_err(|_| anyhow::anyhow!("pânico numa regra do motor no ciclo {agora}"))?;
        let marcas = laco::marcas_do_ciclo(&self.laco, agora);
        let linhas: Vec<LinhaDecisao> = decisoes
            .iter()
            .map(|x| LinhaDecisao {
                ciclo: agora,
                nick: x.nick.clone(),
                regra: x.regra.to_owned(),
                decisao: x.decisao.tipo().to_owned(),
                dados: x.dados(),
            })
            .chain(marcas.into_iter().map(|(nick, campo)| LinhaDecisao {
                ciclo: agora,
                nick,
                dados: laco::esperada_da_marca(&campo),
                regra: campo,
                decisao: "marca".into(),
            }))
            .collect();
        let n = repo::gravar_decisoes(db, &linhas).await?;
        if self.escrita {
            let fila: Vec<(String, String, Value)> = decisoes
                .iter()
                .filter_map(|x| {
                    x.decisao
                        .comando_da_fila()
                        .map(|(tipo, params)| (x.nick.clone(), tipo, params))
                })
                .collect();
            // Vale até o próximo ciclo: comando velho com o estado já mudado não serve.
            let ids = repo::enfileirar(db, agora, &fila, agora + 2 * 30_000).await?;
            metrics::counter!("nexus_motor_comandos_total").increment(ids.len() as u64);
        }
        Ok((n, novas))
    }

    /// Laço até o cancelamento.
    pub async fn rodar(mut self, db: PgPool, ciclo: Duration, parar: CancellationToken) {
        tracing::info!(url = %self.url, ciclo_s = ciclo.as_secs(), "sombra do motor iniciada");
        let mut intervalo = tokio::time::interval(ciclo);
        let mut podou_em = 0_i64;
        // Ciclo atrasado (legado lento) não acumula: o próximo sai no ritmo normal.
        intervalo.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                () = parar.cancelled() => break,
                _ = intervalo.tick() => {}
            }
            let agora = crate::bolsa::agora_ms();
            if agora - podou_em > 3_600_000 {
                podou_em = agora;
                // 14 dias de decisões e 30 de ações do legado bastam para o critério de 7 dias.
                match repo::podar_sombra(&db, agora, 14, 30).await {
                    Ok((d, a)) if d + a > 0 => {
                        tracing::info!(decisoes = d, acoes = a, "sombra do motor podada")
                    }
                    Ok(_) => {}
                    Err(erro) => tracing::warn!(%erro, "não consegui podar a sombra do motor"),
                }
            }
            match self.ciclo(&db).await {
                Ok((decisoes, acoes)) => {
                    metrics::counter!("nexus_motor_sombra_decisoes_total").increment(decisoes);
                    metrics::counter!("nexus_motor_sombra_acoes_legado_total").increment(acoes);
                    if decisoes > 0 {
                        tracing::info!(decisoes, acoes, "ciclo do motor em sombra");
                    }
                }
                Err(erro) => {
                    metrics::counter!("nexus_motor_sombra_falhas_total").increment(1);
                    tracing::warn!(%erro, "ciclo do motor em sombra falhou");
                }
            }
        }
        tracing::info!("sombra do motor encerrada");
    }
}

/// `nexus-api motor-sombra-relatorio [--desde <horas>]`: decisões do Nexus × ações do
/// legado no período em que os dois lados têm dados.
pub async fn relatorio(db: &PgPool, desde_h: i64, tolerancia_ms: i64) -> anyhow::Result<()> {
    let desde = crate::bolsa::agora_ms() - desde_h * 3_600_000;
    let decisoes = repo::decisoes_desde(db, desde).await?;
    let Some(inicio) = decisoes.first().map(|x| x.0) else {
        println!("nenhuma decisão do motor em sombra nas últimas {desde_h} h");
        return Ok(());
    };
    let acoes = repo::acoes_legado_desde(db, inicio).await?;
    let r = laco::confrontar(&decisoes, &acoes, tolerancia_ms);
    println!(
        "motor em sombra: {} decisões do Nexus e {} ações do legado desde o primeiro ciclo",
        decisoes.len(),
        acoes.len()
    );
    for (tipo, c) in &r {
        println!(
            "  {tipo:<12} iguais {:>5}  só Nexus {:>5}  só legado {:>5}",
            c.iguais, c.so_nexus, c.so_legado
        );
        for e in &c.exemplos {
            println!("      {e}");
        }
    }
    let diferentes: u64 = r.values().map(|c| c.so_nexus + c.so_legado).sum();
    println!(
        "  veredito: {}",
        if diferentes == 0 {
            "OK (decisões iguais às do legado)"
        } else {
            "DIFERENTE"
        }
    );
    Ok(())
}

#[cfg(test)]
mod testes {
    use super::*;

    fn l(seq: i64) -> (i64, i64, String) {
        (seq, seq * 1000, format!("linha {seq}"))
    }

    #[test]
    fn acumula_pela_sobreposicao() {
        let mut g = Vec::new();
        acumular(&mut g, &[l(1), l(2), l(3)]);
        acumular(&mut g, &[l(2), l(3), l(4)]);
        assert_eq!(g.iter().map(|x| x.0).collect::<Vec<_>>(), [1, 2, 3, 4]);
        // Painel recarregado: o seq recomeça, as linhas antigas ficam.
        acumular(&mut g, &[l(3), l(4), l(1)]);
        assert_eq!(g.iter().map(|x| x.0).collect::<Vec<_>>(), [1, 2, 3, 4, 1]);
        acumular(&mut g, &[l(1)]);
        assert_eq!(g.len(), 5, "nada novo");
    }
}
