//! Fila de comandos no `nexus-api`: a entrega (reserva com lease), a confirmação e a tarefa de
//! manutenção/espelho. As rotas HTTP e o WebSocket usam estas funções, então contam as mesmas
//! métricas e seguem as mesmas regras.
//!
//! Métricas: `nexus_comandos_entregues_total{origem}`, `nexus_comandos_duplicados_total`
//! (2ª entrega do mesmo comando ao mesmo relay), `nexus_comandos_confirmados_total{sombra}`,
//! `nexus_comandos_confirmacoes_repetidas_total`, `nexus_comandos_perdidos_total` (expirou sem
//! ninguém receber), `nexus_comandos_expirados_total`, `nexus_comandos_devolvidos_total` e
//! `nexus_comandos_sem_confirmacao_total`.

use std::time::Duration;

use persistencia::comandos::{self, Comando, Confirmacao, Reserva, LEASE_PADRAO_S};
use tokio_util::sync::CancellationToken;

use crate::estado::Estado;

/// Resultado que o relay em modo sombra devolve: recebeu, não executou.
pub const RESULTADO_SOMBRA: &str = "sombra";
/// Tamanho máximo do resultado de um comando (como o legado: 200 caracteres).
pub const MAX_RESULTADO: usize = 200;
const CICLO: Duration = Duration::from_secs(5);
const LOTE_ESPELHO: i64 = 500;

/// Reserva os pendentes das contas para o consumidor `por` e conta as métricas.
pub async fn entregar(
    estado: &Estado,
    contas: &[String],
    por: &str,
    limite: i64,
) -> Result<Vec<Comando>, sqlx::Error> {
    let r = Reserva {
        limite,
        prioridade_desc: estado.melhorias,
        ..Reserva::nova(por)
    };
    let v = comandos::pendentes(&estado.db, contas, &r).await?;
    for c in &v {
        metrics::counter!("nexus_comandos_entregues_total", "origem" => c.origem.clone())
            .increment(1);
        if c.repetido {
            metrics::counter!("nexus_comandos_duplicados_total").increment(1);
        }
    }
    Ok(v)
}

/// Confirma um comando (idempotente) e conta a métrica.
pub async fn confirmar(
    estado: &Estado,
    id: i64,
    resultado: &str,
) -> Result<Confirmacao, sqlx::Error> {
    let c = comandos::confirmar(&estado.db, id, resultado).await?;
    match c {
        Confirmacao::Gravada => {
            let sombra = if resultado == RESULTADO_SOMBRA {
                "sim"
            } else {
                "nao"
            };
            metrics::counter!("nexus_comandos_confirmados_total", "sombra" => sombra).increment(1);
        }
        Confirmacao::Repetida => {
            metrics::counter!("nexus_comandos_confirmacoes_repetidas_total").increment(1);
        }
        Confirmacao::Inexistente | Confirmacao::NaoReservado => {
            tracing::warn!(id, resultado = ?c, "confirmação de comando que não pode ser aceita");
        }
    }
    Ok(c)
}

/// O comando é de uma conta do escopo? Lista vazia = todas (token global).
pub fn no_escopo(contas: &[String], conta: &str) -> bool {
    contas.is_empty() || contas.iter().any(|c| c.eq_ignore_ascii_case(conta))
}

/// Resultado de [`confirmar_no_escopo`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confirmada {
    /// O comando é de outra conta: nada foi gravado.
    Negada,
    Resultado(Confirmacao),
}

/// Confirma checando o dono: token de conta só confirma comando da própria conta. É a mesma
/// regra para o HTTP e para o WebSocket.
pub async fn confirmar_no_escopo(
    estado: &Estado,
    contas: &[String],
    id: i64,
    resultado: &str,
) -> Result<Confirmada, sqlx::Error> {
    if !contas.is_empty() {
        match comandos::conta_de(&estado.db, id).await? {
            Some(c) if no_escopo(contas, &c) => {}
            Some(_) => {
                metrics::counter!("nexus_comandos_confirmacoes_negadas_total").increment(1);
                tracing::warn!(id, "confirmação negada: comando de outra conta");
                return Ok(Confirmada::Negada);
            }
            None => {
                tracing::warn!(id, "confirmação de comando inexistente");
                return Ok(Confirmada::Resultado(Confirmacao::Inexistente));
            }
        }
    }
    confirmar(estado, id, resultado)
        .await
        .map(Confirmada::Resultado)
}

/// Um passo de manutenção: espelha a fila do legado (se ligado), expira o que passou do prazo
/// e devolve os reservados sem confirmação.
pub async fn manter(estado: &Estado, marca: &mut Option<(i64, i64)>) -> Result<(), sqlx::Error> {
    if estado.espelho_comandos {
        // Várias levas por passo, para recuperar o atraso depois de uma pausa.
        loop {
            let (lidas, _) = comandos::espelhar(&estado.db, LOTE_ESPELHO, marca).await?;
            if lidas < LOTE_ESPELHO as u64 {
                break;
            }
        }
    }
    for entregue in comandos::expirar(&estado.db).await? {
        if entregue {
            metrics::counter!("nexus_comandos_expirados_total").increment(1);
        } else {
            metrics::counter!("nexus_comandos_perdidos_total").increment(1);
        }
    }
    let (devolvidos, encerrados) = comandos::devolver_vencidos(&estado.db, LEASE_PADRAO_S).await?;
    metrics::counter!("nexus_comandos_devolvidos_total").increment(devolvidos);
    metrics::counter!("nexus_comandos_sem_confirmacao_total").increment(encerrados);
    Ok(())
}

/// Tarefa em segundo plano: roda a cada 5 s enquanto a fila (ou o espelho) estiver ligada.
pub fn iniciar(estado: Estado, parar: CancellationToken) {
    tokio::spawn(async move {
        let mut marca = None;
        loop {
            if let Err(e) = manter(&estado, &mut marca).await {
                tracing::warn!(erro = %e, "manutenção da fila de comandos falhou");
            }
            tokio::select! {
                () = parar.cancelled() => break,
                () = tokio::time::sleep(CICLO) => {}
            }
        }
    });
}
