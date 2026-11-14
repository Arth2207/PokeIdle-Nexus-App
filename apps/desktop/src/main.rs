//! Casca Tauri do app A2: janelas do jogo (uma por conta), janela do Tracker e a cola com o
//! núcleo (`a2-nucleo`). A lógica testável está em `a2_nucleo::jogo`; aqui só há I/O de janela.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use a2_nucleo::cliente::{rodar, Config};
use a2_nucleo::comandos::Modo;
use a2_nucleo::jogo::{
    conta_do_rotulo, evento_tracker, eventos_do_coletor, rotulo_da_conta, script_passo, Avaliador,
    Correio, ExecutorJanelas,
};
use a2_nucleo::token::ArmazemToken;
use a2_nucleo::ArquivoToken;
use serde_json::Value;
use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;
use std::time::Duration;
use tauri::webview::WebviewBuilder;
use tauri::{
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, State, Webview, WebviewUrl,
    WebviewWindow, WebviewWindowBuilder,
};
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
    /// Estado de cada conta do card (só o que o coletor realmente informou).
    contas: Mutex<HashMap<String, ContaEstado>>,
    /// Último retângulo (x, y, w, h, visível) que a UI mandou para cada conta.
    /// A UI pode mandar antes da webview existir; `criar_filha` aplica depois.
    rects: Mutex<HashMap<String, Rect>>,
}

type Rect = (f64, f64, f64, f64, bool);

fn aplicar_rect(v: &tauri::Webview, r: Rect) -> Result<(), String> {
    let (x, y, w, h, visivel) = r;
    if !visivel || w < 1.0 || h < 1.0 {
        return v.hide().map_err(|e| e.to_string());
    }
    v.set_position(LogicalPosition::new(x, y))
        .map_err(|e| e.to_string())?;
    v.set_size(LogicalSize::new(w, h))
        .map_err(|e| e.to_string())?;
    v.show().map_err(|e| e.to_string())
}

/// Janela de kills usada no kills/h (minutos).
const JANELA_KILLS_MIN: i64 = 10;
/// Sem nenhum evento do coletor por este tempo, a captura da conta é dada como com erro.
const SEM_CAPTURA_MS: i64 = 120_000;

#[derive(Default)]
struct ContaEstado {
    pausada: bool,
    ultimo_evento: Option<i64>,
    hunt: Option<String>,
    kills: VecDeque<i64>,
    /// Última vez que o coletor da página falou (qualquer mensagem): prova que a IPC funciona.
    coletor_visto: Option<i64>,
}

impl ContaEstado {
    fn novo() -> Self {
        Self { ..Self::default() }
    }

    fn registrar(&mut self, ev: &Value, agora: i64) {
        self.ultimo_evento = Some(agora);
        if ev.get("k").and_then(Value::as_str) == Some("kill") {
            self.kills
                .push_back(ev.get("ts").and_then(Value::as_i64).unwrap_or(agora));
        }
        if let Some(h) = ev.get("hunt_slug").and_then(Value::as_str) {
            self.hunt = Some(h.to_owned());
        }
        self.podar(agora);
    }

    /// Sinal de vida do coletor: com a conexão do jogo aberta e mensagem nos últimos 60 s,
    /// a captura conta como ativa (conta parada sem batalha não é erro).
    fn vivo(&mut self, msg: &Value, agora: i64) {
        let ws = msg.get("ws").and_then(Value::as_bool).unwrap_or(false);
        let ultima = msg.get("msg_ms").and_then(Value::as_i64).unwrap_or(0);
        if ws && agora - ultima <= 60_000 {
            self.ultimo_evento = Some(agora);
        }
        if let Some(h) = msg
            .get("hunt")
            .and_then(Value::as_str)
            .filter(|h| !h.is_empty())
        {
            self.hunt = Some(h.to_owned());
        }
    }

    fn podar(&mut self, agora: i64) {
        let corte = agora - JANELA_KILLS_MIN * 60_000;
        while self.kills.front().is_some_and(|t| *t < corte) {
            self.kills.pop_front();
        }
    }

    fn status(&self, agora: i64) -> &'static str {
        if self.pausada {
            return "pausada";
        }
        // Nunca chegou dado do jogo: conta ainda no login ou fora de caçada; não é erro.
        let Some(visto) = self.ultimo_evento else {
            // Nem o coletor falou: a página do jogo não conversa com o app.
            return if self.coletor_visto.is_some() {
                "aguardando"
            } else {
                "sem_coletor"
            };
        };
        if agora - visto > SEM_CAPTURA_MS {
            "erro"
        } else {
            "execucao"
        }
    }

    fn json(&self, conta: &str, agora: i64) -> Value {
        // kills/h só existe se houve kill dentro da janela; senão fica nulo ("—" na tela).
        let kills_h =
            (!self.kills.is_empty()).then(|| self.kills.len() as i64 * 60 / JANELA_KILLS_MIN);
        let captura_ok = self
            .ultimo_evento
            .is_some_and(|t| agora - t <= SEM_CAPTURA_MS);
        serde_json::json!({
            "conta": conta, "status": self.status(agora), "hunt": self.hunt,
            "kills_h": kills_h, "hp_min": Value::Null,
            "captura_ok": captura_ok, "ultima_captura": self.ultimo_evento,
        })
    }
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
    {
        let mut m = travar(&estado.contas);
        if let Some(c) = m.get_mut(conta) {
            // Conta pausada: a coleta daquela conta para (nada vai ao núcleo).
            if c.pausada {
                return;
            }
            c.registrar(&ev, agora_ms());
        }
    }
    if let Some(o) = ev.as_object_mut() {
        o.insert("_conta".into(), Value::String(conta.to_string()));
    }
    if let Some(tx) = travar(&estado.eventos).as_ref() {
        // Fila cheia: descarta (o jogo continua; o HUD é reenviado pelo próprio jogo).
        let _ = tx.try_send(ev);
    }
}

/// Navegação permitida nas webviews do jogo: o site, o login (Google ou Discord) e a
/// verificação anti-robô, como no legado. A IPC continua só para pokeidle.io (capabilities/jogo.json).
fn host_permitido(u: &tauri::Url) -> bool {
    const DOMINIOS: [&str; 7] = [
        "pokeidle.io",
        "google.com",
        "youtube.com",
        "discord.com",
        "cloudflare.com",
        "hcaptcha.com",
        "recaptcha.net",
    ];
    if u.scheme() != "https" {
        return false;
    }
    let Some(h) = u.host_str() else {
        return false;
    };
    DOMINIOS
        .iter()
        .any(|d| h == *d || h.strip_suffix(d).is_some_and(|r| r.ends_with('.')))
}

/// Popup de login (`window.open`): abre na mesma sessão da conta, como no legado.
fn popup_permitido<R: tauri::Runtime>(
    u: tauri::Url,
    _f: tauri::webview::NewWindowFeatures,
) -> tauri::webview::NewWindowResponse<R> {
    if host_permitido(&u) {
        tauri::webview::NewWindowResponse::Allow
    } else {
        tauri::webview::NewWindowResponse::Deny
    }
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
    let url = url.parse::<tauri::Url>().map_err(|e| e.to_string())?;
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
        .on_navigation(host_permitido)
        .on_new_window(popup_permitido);
    #[cfg(windows)]
    let b = b.additional_browser_args(ARGS_ECO);
    b.build().map(|_| ()).map_err(|e| e.to_string())
}

/// Cria (se não existir) a webview filha do jogo de uma conta, dentro da janela principal.
/// Perfil isolado por conta e coletor injetado, como antes. Nasce escondida: a UI posiciona.
fn criar_filha(app: &AppHandle, conta: &str) -> Result<(), String> {
    let rotulo = rotulo_da_conta(conta);
    if app.get_webview(&rotulo).is_some() {
        return Ok(());
    }
    let janela = app.get_window("main").ok_or("janela principal ausente")?;
    let url = URL_JOGO.parse::<tauri::Url>().map_err(|e| e.to_string())?;
    let dados = app
        .path()
        .app_local_data_dir()
        .map_err(|e| e.to_string())?
        .join("perfis")
        .join(&rotulo);
    let b = WebviewBuilder::new(&rotulo, WebviewUrl::External(url))
        .data_directory(dados)
        .initialization_script(COLETOR)
        .on_navigation(host_permitido)
        .on_new_window(popup_permitido);
    #[cfg(windows)]
    let b = b.additional_browser_args(ARGS_ECO);
    let w = janela
        .add_child(
            b,
            LogicalPosition::new(0.0, 0.0),
            LogicalSize::new(320.0, 180.0),
        )
        .map_err(|e| e.to_string())?;
    let est = app.state::<Estado>();
    let r = travar(&est.rects).get(conta).copied();
    match r {
        Some(r) => aplicar_rect(&w, r),
        None => w.hide().map_err(|e| e.to_string()),
    }
}

/// A UI informa o retângulo (px CSS da janela) da área do jogo de um card.
#[tauri::command]
fn posicionar_conta(
    conta: String,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    visivel: bool,
    app: AppHandle,
) -> Result<(), String> {
    let r = (x, y, w, h, visivel);
    travar(&app.state::<Estado>().rects).insert(conta.clone(), r);
    match app.get_webview(&rotulo_da_conta(&conta)) {
        Some(v) => aplicar_rect(&v, r),
        None => Ok(()),
    }
}

/// Pausa/retoma a coleta e os comandos de uma conta.
#[tauri::command]
fn pausar_conta(conta: String, pausada: bool, estado: State<'_, Estado>) -> Result<(), String> {
    let mut m = travar(&estado.contas);
    let c = m.get_mut(&conta).ok_or("conta desconhecida")?;
    c.pausada = pausada;
    Ok(())
}

/// Recarrega a webview do jogo de uma conta.
#[tauri::command]
fn recarregar_conta(conta: String, app: AppHandle) -> Result<(), String> {
    let v = app
        .get_webview(&rotulo_da_conta(&conta))
        .ok_or("webview da conta ausente")?;
    v.reload().map_err(|e| e.to_string())
}

/// Abre a janela do Tracker. Só abre a página inicial; o resto é navegação do usuário.
#[tauri::command]
async fn abrir_tracker(app: AppHandle) -> Result<(), String> {
    criar_janela(
        &app,
        ROTULO_TRACKER,
        "Tracker",
        URL_TRACKER,
        COLETOR_TRACKER,
    )
}

/// Conta dona da janela: o rótulo perde maiúsculas ("Ana" → "jogo-ana"), então compara pelo
/// rótulo de cada conta cadastrada em vez de decodificar.
fn conta_da_janela(estado: &Estado, rotulo: &str) -> Option<String> {
    conta_do_rotulo(rotulo)?;
    travar(&estado.contas)
        .keys()
        .find(|k| rotulo_da_conta(k) == rotulo)
        .cloned()
}

/// IPC do coletor: a conta vem do rótulo da janela, nunca do que a página disser.
#[tauri::command]
fn coletor_evento(window: Webview, msg: Value, estado: State<'_, Estado>) {
    let Some(conta) = conta_da_janela(&estado, window.label()) else {
        return;
    };
    let agora = agora_ms();
    if let Some(c) = travar(&estado.contas).get_mut(&conta) {
        c.coletor_visto = Some(agora);
    }
    // Sinal de vida: só atualiza o card; não vai ao núcleo.
    if msg.get("t").and_then(Value::as_str) == Some("vivo") {
        if let Some(c) = travar(&estado.contas).get_mut(&conta) {
            c.vivo(&msg, agora);
        }
        return;
    }
    for ev in eventos_do_coletor(&msg, agora_ms()) {
        empurrar(&estado, &conta, ev);
    }
}

#[tauri::command]
fn coletor_resultado(window: Webview, id: u64, resultado: String, estado: State<'_, Estado>) {
    if conta_da_janela(&estado, window.label()).is_some() {
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
            let pausada = {
                let est = self.app.state::<Estado>();
                let m = travar(&est.contas);
                m.get(conta).is_some_and(|c| c.pausada)
            };
            if pausada {
                return Err("conta pausada".into());
            }
            let janela = self
                .app
                .get_webview(&rotulo_da_conta(conta))
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
async fn iniciar_nucleo(
    url: String,
    contas: Vec<String>,
    modo: Option<String>,
    app: AppHandle,
    estado: State<'_, Estado>,
) -> Result<(), String> {
    let armazem = ArquivoToken::padrao("nexus").map_err(|e| e.to_string())?;
    {
        // Mantém o estado das contas que continuam; descarta as removidas.
        let mut m = travar(&estado.contas);
        m.retain(|k, _| contas.contains(k));
        for c in &contas {
            m.entry(c.clone()).or_insert_with(ContaEstado::novo);
        }
    }
    for c in &contas {
        criar_filha(&app, c)?;
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
    let app_tela = app.clone();
    let exec = ExecutorJanelas::novo(Janelas {
        app,
        correio: estado.correio.clone(),
    });
    let tela = app_tela;
    tauri::async_runtime::spawn(async move {
        let fim = match rodar(cfg, armazem, exec, eventos_rx, parar_rx).await {
            Ok(()) => "núcleo parado".to_owned(),
            Err(e) => format!("núcleo parou: {e}"),
        };
        let _ = tela.emit("nucleo", fim);
    });
    Ok(())
}

/// Grava o token do Nexus (escopo `relay`) no armazém local do app.
#[tauri::command]
fn salvar_token(token: String) -> Result<(), String> {
    let token = token.trim();
    if token.is_empty() {
        return Err("token vazio".into());
    }
    ArquivoToken::padrao("nexus")
        .and_then(|a| a.gravar(token))
        .map_err(|e| e.to_string())
}

/// Há token salvo? (a tela só mostra sim/não, nunca o valor)
#[tauri::command]
fn tem_token() -> Result<bool, String> {
    ArquivoToken::padrao("nexus")
        .and_then(|a| a.ler())
        .map(|t| t.is_some())
        .map_err(|e| e.to_string())
}

fn main() {
    let r = tauri::Builder::default()
        .manage(Estado::default())
        .setup(|app| {
            // Janela sem webview padrão: a UI entra como filha primeiro e os jogos depois,
            // por cima dela (com WebviewWindow a webview padrão cobria as filhas no Windows).
            let janela = tauri::window::WindowBuilder::new(app, "main")
                .title("Nexus")
                .inner_size(1100.0, 720.0)
                .build()?;
            let tam = janela
                .inner_size()?
                .to_logical::<f64>(janela.scale_factor()?);
            janela.add_child(
                WebviewBuilder::new("ui", WebviewUrl::App("index.html".into())).auto_resize(),
                LogicalPosition::new(0.0, 0.0),
                tam,
            )?;
            // Publica para a UI o estado dos cards a cada 2 s.
            let h = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let mut t = tokio::time::interval(Duration::from_secs(2));
                loop {
                    t.tick().await;
                    let agora = agora_ms();
                    let lista: Vec<Value> = {
                        let est = h.state::<Estado>();
                        let mut m = travar(&est.contas);
                        m.iter_mut()
                            .map(|(k, c)| {
                                c.podar(agora);
                                c.json(k, agora)
                            })
                            .collect()
                    };
                    let _ = h.emit("contas", lista);
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            iniciar_nucleo,
            salvar_token,
            tem_token,
            posicionar_conta,
            pausar_conta,
            recarregar_conta,
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
