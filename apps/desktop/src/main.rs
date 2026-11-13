//! Casca Tauri do app A2. Só liga o núcleo (`a2-nucleo`) ao processo desktop.
//!
//! TODO (A2 parte b): webview do jogo (janela com o cliente do jogo) + [`Executor`] real que
//! age nela; UI em Leptos; token via keyring; leitura do estado do jogo -> `eventos`.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use a2_nucleo::cliente::{rodar, Config};
use a2_nucleo::comandos::{Executor, Modo};
use a2_nucleo::ArquivoToken;
use a2_nucleo::ComandoNovo;
use std::future::Future;
use std::pin::Pin;
use tauri::State;
use tokio::sync::{mpsc, watch};

/// Executor provisório: o webview do jogo ainda não existe, então nada é executado.
struct SemWebview;

impl Executor for SemWebview {
    fn executar<'a>(
        &'a self,
        _cmd: &'a ComandoNovo,
    ) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send + 'a>> {
        Box::pin(async { Err("webview do jogo ainda não plugado".to_string()) })
    }
}

#[derive(Default)]
struct Controle {
    parar: std::sync::Mutex<Option<watch::Sender<bool>>>,
}

/// Inicia o núcleo (em modo sombra até o cutover da fase 3). Segunda chamada só reinicia.
#[tauri::command]
fn iniciar_nucleo(url: String, contas: Vec<String>, ctl: State<'_, Controle>) -> Result<(), String> {
    let armazem = ArquivoToken::padrao("nexus").map_err(|e| e.to_string())?;
    let (parar_tx, parar_rx) = watch::channel(false);
    let (_eventos_tx, eventos_rx) = mpsc::channel(256); // TODO: ligar à leitura do jogo
    let cfg = Config {
        url,
        modo: Modo::Sombra,
        contas,
        conta_lote: None,
        lote_max: 50,
    };
    let mut guarda = ctl.parar.lock().map_err(|e| e.to_string())?;
    if let Some(antigo) = guarda.replace(parar_tx) {
        let _ = antigo.send(true);
    }
    tauri::async_runtime::spawn(async move {
        let _ = rodar(cfg, armazem, SemWebview, eventos_rx, parar_rx).await;
    });
    Ok(())
}

fn main() {
    let r = tauri::Builder::default()
        .manage(Controle::default())
        .invoke_handler(tauri::generate_handler![iniciar_nucleo])
        .run(tauri::generate_context!());
    if let Err(e) = r {
        eprintln!("falha ao iniciar o app: {e}");
    }
}
