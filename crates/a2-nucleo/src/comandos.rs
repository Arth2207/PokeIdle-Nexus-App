//! Execução dos comandos entregues pelo servidor, com confirmação por conta.
//!
//! O servidor entrega "pelo menos uma vez" (lease de 120 s): o mesmo comando pode chegar de novo.
//! O [`Despachante`] garante que o [`Executor`] roda uma vez por id e que a reentrega recebe o
//! mesmo resultado, sem executar de novo. Comandos de uma conta rodam em série.

use contrato::ws::{ComandoNovo, ComandoResultado};
use std::collections::{HashMap, HashSet, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard};

/// Quem age no jogo (a casca Tauri/webview implementa). `Ok(texto)` = sucesso; `Err(motivo)` =
/// falha, confirmada como `erro: <motivo>`.
pub trait Executor: Send + Sync + 'static {
    fn executar<'a>(
        &'a self,
        cmd: &'a ComandoNovo,
    ) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send + 'a>>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Modo {
    /// Executa de verdade.
    Ativo,
    /// Recebe e confirma como `sombra`, sem executar (validação dos 7 dias da fase 3).
    Sombra,
}

pub const RESULTADO_SOMBRA: &str = "sombra";
/// Folga entre dois comandos da mesma conta: o jogo recusa rajadas ("Devagar aí — muitas
/// mensagens"); o motor antigo esperava o mesmo entre um pedido e outro.
pub const FOLGA_JOGO: std::time::Duration = std::time::Duration::from_millis(500);
pub const RESULTADO_EXPIRADO: &str = "expirado";
const MEMORIA: usize = 512;

#[derive(Default)]
struct Estado {
    em_curso: HashSet<i64>,
    cancelados: HashSet<i64>,
    concluidos: VecDeque<(i64, String)>,
}

struct Interno<E> {
    exec: E,
    /// Contas aceitas (nicks normalizados); vazio = todas.
    contas: Vec<String>,
    modo: Modo,
    estado: Mutex<Estado>,
    travas: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
}

pub struct Despachante<E>(Arc<Interno<E>>);

impl<E> Clone for Despachante<E> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

fn travar<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn norm(nick: &str) -> String {
    nick.trim().to_lowercase()
}

impl<E: Executor> Despachante<E> {
    pub fn novo(exec: E, contas: &[String], modo: Modo) -> Self {
        Self(Arc::new(Interno {
            exec,
            contas: contas.iter().map(|c| norm(c)).collect(),
            modo,
            estado: Mutex::default(),
            travas: Mutex::default(),
        }))
    }

    /// `comando.cancelado`: se ainda não começou, não executa e não confirma.
    pub fn cancelar(&self, id: i64) {
        let mut e = travar(&self.0.estado);
        if !e.concluidos.iter().any(|(i, _)| *i == id) {
            e.cancelados.insert(id);
        }
    }

    /// Trata um comando entregue. `None` = nada a confirmar (conta fora do escopo, cancelado,
    /// ou já em execução: o resultado sai quando a primeira execução terminar).
    pub async fn tratar(&self, cmd: &ComandoNovo, agora_ms: i64) -> Option<ComandoResultado> {
        let i = &self.0;
        if !i.contas.is_empty() && !i.contas.contains(&norm(&cmd.conta)) {
            return None;
        }
        {
            let mut e = travar(&i.estado);
            if let Some((_, r)) = e.concluidos.iter().find(|(id, _)| *id == cmd.id) {
                return Some(ComandoResultado {
                    id: cmd.id,
                    resultado: r.clone(),
                });
            }
            if !e.em_curso.insert(cmd.id) {
                return None;
            }
        }
        let trava = travar(&i.travas)
            .entry(norm(&cmd.conta))
            .or_default()
            .clone();
        let _fila_da_conta = trava.lock().await;
        let mut executou = false;

        let resultado = if travar(&i.estado).cancelados.remove(&cmd.id) {
            travar(&i.estado).em_curso.remove(&cmd.id);
            return None;
        } else if cmd.expira_em.is_some_and(|t| t <= agora_ms) {
            RESULTADO_EXPIRADO.to_string()
        } else if i.modo == Modo::Sombra {
            RESULTADO_SOMBRA.to_string()
        } else {
            executou = true;
            match i.exec.executar(cmd).await {
                Ok(r) => r,
                Err(m) => format!("erro: {m}"),
            }
        };

        {
            let mut e = travar(&i.estado);
            e.em_curso.remove(&cmd.id);
            e.concluidos.push_back((cmd.id, resultado.clone()));
            while e.concluidos.len() > MEMORIA {
                e.concluidos.pop_front();
            }
        }
        // Resultado já guardado (a reentrega o recebe na hora); ainda com a vez da conta, o
        // próximo comando dela espera a folga.
        if executou {
            tokio::time::sleep(FOLGA_JOGO).await;
        }
        Some(ComandoResultado {
            id: cmd.id,
            resultado,
        })
    }
}

#[cfg(test)]
mod testes {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Contador(Arc<AtomicUsize>);
    impl Executor for Contador {
        fn executar<'a>(
            &'a self,
            cmd: &'a ComandoNovo,
        ) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send + 'a>> {
            Box::pin(async move {
                self.0.fetch_add(1, Ordering::SeqCst);
                if cmd.tipo == "falha" {
                    Err("boom".into())
                } else {
                    Ok("ok".into())
                }
            })
        }
    }

    fn cmd(id: i64, conta: &str, tipo: &str, expira: Option<i64>) -> ComandoNovo {
        ComandoNovo {
            id,
            conta: conta.into(),
            tipo: tipo.into(),
            params: json!({}),
            prioridade: 0,
            expira_em: expira,
        }
    }

    fn desp(modo: Modo, contas: &[&str]) -> (Despachante<Contador>, Arc<AtomicUsize>) {
        let n = Arc::new(AtomicUsize::new(0));
        let c: Vec<String> = contas.iter().map(|s| s.to_string()).collect();
        (Despachante::novo(Contador(n.clone()), &c, modo), n)
    }

    #[tokio::test]
    async fn reentrega_nao_executa_de_novo_e_repete_resultado() {
        let (d, n) = desp(Modo::Ativo, &[]);
        let r1 = d.tratar(&cmd(1, "a", "x", None), 0).await;
        let r2 = d.tratar(&cmd(1, "a", "x", None), 0).await;
        assert_eq!(r1, r2);
        assert_eq!(r1.map(|r| r.resultado), Some("ok".into()));
        assert_eq!(n.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn erro_expirado_e_sombra() {
        let (d, n) = desp(Modo::Ativo, &[]);
        let r = d.tratar(&cmd(1, "a", "falha", None), 0).await;
        assert_eq!(r.map(|r| r.resultado), Some("erro: boom".into()));
        let r = d.tratar(&cmd(2, "a", "x", Some(50)), 100).await;
        assert_eq!(r.map(|r| r.resultado), Some("expirado".into()));
        assert_eq!(n.load(Ordering::SeqCst), 1);

        let (s, n) = desp(Modo::Sombra, &[]);
        let r = s.tratar(&cmd(3, "a", "x", None), 0).await;
        assert_eq!(r.map(|r| r.resultado), Some("sombra".into()));
        assert_eq!(n.load(Ordering::SeqCst), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn mesma_conta_espera_a_folga_e_outra_conta_nao() {
        let (d, _) = desp(Modo::Ativo, &[]);
        let (c1, c2, c3) = (
            cmd(1, "a", "x", None),
            cmd(2, "a", "x", None),
            cmd(3, "b", "x", None),
        );
        let t0 = tokio::time::Instant::now();
        let (a, b, c) = tokio::join!(d.tratar(&c1, 0), d.tratar(&c2, 0), d.tratar(&c3, 0));
        assert!(a.is_some() && b.is_some() && c.is_some());
        // Dois da conta "a" em série, cada um com a folga; o de "b" corre em paralelo.
        assert_eq!(t0.elapsed(), FOLGA_JOGO * 2);
    }

    #[tokio::test]
    async fn conta_fora_do_escopo_e_ignorada() {
        let (d, n) = desp(Modo::Ativo, &["Alfa"]);
        assert!(d.tratar(&cmd(1, "beta", "x", None), 0).await.is_none());
        assert!(d.tratar(&cmd(2, " ALFA ", "x", None), 0).await.is_some());
        assert_eq!(n.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn cancelado_antes_de_rodar_nao_executa() {
        let (d, n) = desp(Modo::Ativo, &[]);
        d.cancelar(9);
        assert!(d.tratar(&cmd(9, "a", "x", None), 0).await.is_none());
        assert_eq!(n.load(Ordering::SeqCst), 0);
    }
}
