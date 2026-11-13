//! Casca Tauri do app A2: janelas do jogo (uma por conta), janela do Tracker e a cola com o
//! núcleo (`a2-nucleo`). A lógica testável está em `a2_nucleo::jogo`; aqui só há I/O de janela.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use a2_nucleo::cliente::{rodar, Config};
use a2_nucleo::comandos::Modo;
use a2_nucleo::jogo::{
    conta_do_rotulo, evento_tracker, eventos_do_coletor, rotulo_da_conta, script_passo, Avaliador,
    Correio, ExecutorJanelas,
};
use a2_nucleo::ArquivoToken;
use serde_json::Value;
use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Manager, State, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use tokio::sync::{mpsc, watch};

const URL_JOGO: &str = "https://pokeidle.io/";
const URL_TRACKER: &str = "https://pokeidle.io/tracker";
const ROTULO_TRACKER: &str = "tracker";
const COLETOR: &str = include_str!("../coletor/coletor.js");
const COLETOR_TRACKER: &str = include_str!("../coletor/tracker.js");
/// Prazo para o coletor responder a um passo.
const PRAZO_PASSO: Duration = Duration::from_secs(15);

/// Modo Eco: o jogo simula no cliente, então o WebView2 não pode atrasar timers nem render em
/// segundo plano ou com a janela coberta. Inclui os padrões do Tauri (o campo os substitui).
#[cfg(windows)]
const ARGS_ECO: &str = "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection \
    --disable-background-timer-throttling --disable-renderer-backgrounding \
    --disable-backgrounding-occluded-windows";

#[derive(Default)]
struct Estado {
    correio: Correio,
    /// Canal de eventos do núcleo; existe depois de `iniciar_nucleo`.
    eventos: Mutex<Option<mpsc::Sender<Value>>>,
    /// Conta que assina as capturas do tracker (a primeira da lista).
    conta_tracker: Mutex<Option<String>>,
    parar: Mutex<Option<watch::Sender<bool>>>,
}

fn agora_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn travar<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn empurrar(estado: &Estado, conta: &str, mut ev: Value) {
    if let Some(o) = ev.as_object_mut() {
        o.insert("_conta".into(), Value::String(conta.to_string()));
    }
    if let Some(tx) = travar(&estado.eventos).as_ref() {
        // Fila cheia: descarta (o jogo continua; o HUD é reenviado pelo próprio jogo).
        let _ = tx.try_send(ev);
    }
}

/// Janelas só navegam dentro de `pokeidle.io`.
fn host_permitido(u: &tauri::Url) -> bool {
    u.scheme() == "https" && matches!(u.host_str(), Some("pokeidle.io") | Some("www.pokeidle.io"))
}

fn criar_janela(
    app: &AppHandle,
    rotulo: &str,
    titulo: &str,
    url: &str,
    script: &str,
) -> Result<(), String> {
    if let Some(w) = app.get_webview_window(rotulo) {
        return w.set_focus().map_err(|e| e.to_string());
    }
    let url: tauri::Url = url
        .parse()
        .map_err(|e: tauri::url::ParseError| e.to_string())?;
    // Perfil isolado por janela (cookies/armazenamento separados por conta).
    let dados = app
        .path()
        .app_local_data_dir()
        .map_err(|e| e.to_string())?
        .join("perfis")
        .join(rotulo);
    let b = WebviewWindowBuilder::new(app, rotulo, WebviewUrl::External(url))
        .title(titulo)
        .inner_size(1100.0, 760.0)
        .data_directory(dados)
        .initialization_script(script)
        .on_navigation(host_permitido);
    #[cfg(windows)]
    let b = b.additional_browser_args(ARGS_ECO);
    b.build().map(|_| ()).map_err(|e| e.to_string())
}

/// Abre (ou foca) a janela do jogo de uma conta.
#[tauri::command]
fn abrir_conta(conta: String, app: AppHandle) -> Result<(), String> {
    let rotulo = rotulo_da_conta(&conta);
    criar_janela(
        &app,
        &rotulo,
        &format!("PokeIdle - {}", conta.trim()),
        URL_JOGO,
        COLETOR,
    )
}

/// Abre a janela do Tracker. Só abre a página inicial; o resto é navegação do usuário.
#[tauri::command]
fn abrir_tracker(app: AppHandle) -> Result<(), String> {
    criar_janela(
        &app,
        ROTULO_TRACKER,
        "Tracker",
        URL_TRACKER,
        COLETOR_TRACKER,
    )
}

/// IPC do coletor: a conta vem do rótulo da janela, nunca do que a página disser.
#[tauri::command]
fn coletor_evento(window: WebviewWindow, msg: Value, estado: State<'_, Estado>) {
    let Some(conta) = conta_do_rotulo(window.label()) else {
        return;
    };
    for ev in eventos_do_coletor(&msg, agora_ms()) {
        empurrar(&estado, &conta, ev);
    }
}

#[tauri::command]
fn coletor_resultado(window: WebviewWindow, id: u64, resultado: String, estado: State<'_, Estado>) {
    if conta_do_rotulo(window.label()).is_some() {
        estado.correio.responder(id, resultado);
    }
}

/// Clique do usuário em "Guardar esta página" na janela do Tracker (uma captura por clique).
#[tauri::command]
fn tracker_captura(
    window: WebviewWindow,
    captura: Value,
    estado: State<'_, Estado>,
) -> Result<(), String> {
    if window.label() != ROTULO_TRACKER {
        return Err("janela não autorizada".into());
    }
    let conta = travar(&estado.conta_tracker)
        .clone()
        .ok_or("núcleo não iniciado")?;
    let ev = evento_tracker(&captura, agora_ms()).ok_or("captura inválida")?;
    empurrar(&estado, &conta, ev);
    Ok(())
}

/// Avalia os passos na janela da conta e espera a resposta do coletor.
struct Janelas {
    app: AppHandle,
    correio: Correio,
}

impl Avaliador for Janelas {
    fn avaliar<'a>(
        &'a self,
        conta: &'a str,
        passo: Value,
    ) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send + 'a>> {
        Box::pin(async move {
            let janela = self
                .app
                .get_webview_window(&rotulo_da_conta(conta))
                .ok_or("janela da conta fechada")?;
            let (id, rx) = self.correio.pedir();
            if let Err(e) = janela.eval(script_passo(id, &passo)) {
                self.correio.cancelar(id);
                return Err(e.to_string());
            }
            match tokio::time::timeout(PRAZO_PASSO, rx).await {
                Ok(Ok(r)) => Ok(r),
                _ => {
                    self.correio.cancelar(id);
                    Err("sem-coletor".into())
                }
            }
        })
    }
}

/// Abre as janelas das contas e liga o núcleo. `modo`: `ativo` ou `sombra` (padrão).
/// Segunda chamada reinicia o núcleo.
#[tauri::command]
fn iniciar_nucleo(
    url: String,
    contas: Vec<String>,
    modo: Option<String>,
    app: AppHandle,
    estado: State<'_, Estado>,
) -> Result<(), String> {
    let armazem = ArquivoToken::padrao("nexus").map_err(|e| e.to_string())?;
    for c in &contas {
        abrir_conta(c.clone(), app.clone())?;
    }
    let (parar_tx, parar_rx) = watch::channel(false);
    let (eventos_tx, eventos_rx) = mpsc::channel(1024);
    *travar(&estado.eventos) = Some(eventos_tx);
    *travar(&estado.conta_tracker) = contas.first().cloned();
    let cfg = Config {
        url,
        modo: if modo.as_deref() == Some("ativo") {
            Modo::Ativo
        } else {
            Modo::Sombra
        },
        contas,
        conta_lote: None,
        lote_max: 50,
    };
    if let Some(antigo) = travar(&estado.parar).replace(parar_tx) {
        let _ = antigo.send(true);
    }
    let exec = ExecutorJanelas::novo(Janelas {
        app,
        correio: estado.correio.clone(),
    });
    tauri::async_runtime::spawn(async move {
        let _ = rodar(cfg, armazem, exec, eventos_rx, parar_rx).await;
    });
    Ok(())
}

fn main() {
    let r = tauri::Builder::default()
        .manage(Estado::default())
        .invoke_handler(tauri::generate_handler![
            iniciar_nucleo,
            abrir_conta,
            abrir_tracker,
            coletor_evento,
            coletor_resultado,
            tracker_captura
        ])
        .run(tauri::generate_context!());
    if let Err(e) = r {
        eprintln!("falha ao iniciar o app: {e}");
    }
}
