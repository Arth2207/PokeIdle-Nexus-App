//! Tela do app A2 em Leptos (CSR). Fala com o Tauri pela ponte global `window.__TAURI__`.
use js_sys::{Array, Date, Object, Reflect};
use leptos::prelude::*;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::spawn_local;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = ["__TAURI__", "core"], catch)]
    async fn invoke(cmd: &str, args: JsValue) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(js_namespace = ["__TAURI__", "event"], catch)]
    async fn listen(
        evento: &str,
        tratador: &Closure<dyn FnMut(JsValue)>,
    ) -> Result<JsValue, JsValue>;
}

const CSS: &str = r#"
:root { --texto:#16181C; --texto-2:#4C5560; --texto-3:#7A838D; --vidro:rgba(255,255,255,.75); --vidro-borda:rgba(255,255,255,.75); --trilho:rgba(22,24,28,.10); --neon:#C8FF2E; --sobre-neon:#16181C; --melhora:#1F9D6B; --piora:#E5533D; --raio:20px; --sombra:0 24px 64px rgba(22,24,28,.16); }
* { box-sizing: border-box; }
html, body { height:100%; }
body { margin:0; font:14px/1.45 Inter, system-ui, "Segoe UI", sans-serif; color:var(--texto); background:#cfe3d2 url("biomas/bioma-floresta-dia.webp") center/cover no-repeat fixed; overflow:hidden; }
.nevoa { position:fixed; inset:0; background:linear-gradient(180deg, rgba(255,255,255,.28), rgba(255,255,255,.10)); pointer-events:none; }
.casca { position:relative; display:flex; gap:16px; height:100vh; padding:16px; }
.vidro { background:var(--vidro); border:1px solid var(--vidro-borda); border-radius:var(--raio); backdrop-filter:blur(22px); -webkit-backdrop-filter:blur(22px); box-shadow:var(--sombra); position:relative; }
.marca { position:absolute; top:10px; right:16px; font:11px "JetBrains Mono", Consolas, monospace; color:var(--texto-3); }
h1, h2 { font-family:"Space Grotesk", Inter, system-ui, sans-serif; margin:0; }
.barra { width:224px; flex:none; padding:20px 12px; display:flex; flex-direction:column; overflow:auto; }
.logo { font:700 26px "Space Grotesk", Inter, sans-serif; padding:0 10px 12px; }
.grupo { font-size:11px; letter-spacing:.12em; text-transform:uppercase; color:var(--texto-3); font-weight:700; padding:14px 10px 4px; }
.item { display:block; width:100%; text-align:left; border:0; background:transparent; padding:9px 12px; border-radius:999px; font:inherit; font-weight:500; color:var(--texto); cursor:pointer; }
.item:hover { background:var(--trilho); }
.item.ativo { background:var(--neon); color:var(--sobre-neon); font-weight:700; }
.palco { flex:1; display:flex; flex-direction:column; gap:16px; min-width:0; }
.topo { display:flex; align-items:center; gap:12px; }
.pilula { border-radius:999px; padding:8px 16px; display:flex; align-items:center; gap:8px; }
.pilula select { border:0; background:transparent; font:inherit; font-weight:600; color:var(--texto); outline:none; }
.conteudo { flex:1; padding:24px 28px; overflow:auto; }
h2 { font-size:15px; letter-spacing:.12em; text-transform:uppercase; margin-bottom:16px; }
.pergunta { color:var(--texto-2); margin:-8px 0 16px; }
.campo { margin:14px 0 4px; font-size:11px; letter-spacing:.12em; text-transform:uppercase; font-weight:700; color:var(--texto-3); display:block; }
input, select.caixa { width:100%; padding:10px 16px; border-radius:999px; border:1px solid var(--trilho); background:rgba(255,255,255,.8); color:var(--texto); font:inherit; outline:none; }
input:focus, select.caixa:focus { border-color:var(--texto-2); }
.linha { display:flex; gap:8px; max-width:640px; }
.limite { max-width:640px; }
.dica { color:var(--texto-3); font-size:12px; margin-top:4px; }
.dica.ok { color:var(--melhora); }
.dica.erro { color:var(--piora); }
button.btn { padding:9px 18px; border-radius:999px; border:1px solid var(--trilho); background:rgba(255,255,255,.85); color:var(--texto); font:inherit; font-weight:600; cursor:pointer; }
button.btn.prim { background:var(--neon); border-color:var(--neon); color:var(--sobre-neon); }
button.btn:disabled { opacity:.5; cursor:default; }
.acoes { display:flex; gap:8px; flex-wrap:wrap; margin:16px 0; }
.sub { margin-top:24px; }
.diario { max-width:760px; margin-top:8px; border-top:1px solid var(--trilho); }
.evento { display:flex; gap:16px; padding:8px 0; border-bottom:1px solid var(--trilho); }
.evento .hora { font:12px "JetBrains Mono", Consolas, monospace; color:var(--texto-3); font-variant-numeric:tabular-nums; flex:none; }
.evento.erro .txt { color:var(--piora); }
.evento .txt::before { content:"\25CF  "; }
.evento.erro .txt::before { content:"\26A0  "; }
.vazio { color:var(--texto-3); padding:8px 0; }
.rodape { position:absolute; bottom:12px; left:22px; font:11px "JetBrains Mono", Consolas, monospace; color:var(--texto-3); }
"#;

const CHAVE_URL: &str = "nexus.url";
const CHAVE_CONTAS: &str = "nexus.contas";
const CHAVE_MODO: &str = "nexus.modo";

fn guardado(chave: &str) -> String {
    web_sys::window()
        .and_then(|j| j.local_storage().ok().flatten())
        .and_then(|s| s.get_item(chave).ok().flatten())
        .unwrap_or_default()
}

fn guardar(chave: &str, valor: &str) {
    if let Some(s) = web_sys::window().and_then(|j| j.local_storage().ok().flatten()) {
        // Armazenamento pode estar bloqueado; a tela segue funcionando sem ele.
        let _ = s.set_item(chave, valor);
    }
}

fn erro_texto(e: &JsValue) -> String {
    e.as_string().unwrap_or_else(|| format!("{e:?}"))
}

/// Monta `{ chave: valor, ... }` para os argumentos de um comando Tauri.
fn args(pares: &[(&str, JsValue)]) -> JsValue {
    let o = Object::new();
    for (k, v) in pares {
        // Reflect::set num objeto novo não falha; se falhar, o comando acusa argumento ausente.
        let _ = Reflect::set(&o, &JsValue::from_str(k), v);
    }
    o.into()
}

fn lista_contas(texto: &str) -> Vec<String> {
    texto
        .split(',')
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .map(str::to_owned)
        .collect()
}

fn url_valida(url: &str) -> bool {
    let resto = url
        .strip_prefix("wss://")
        .or_else(|| url.strip_prefix("ws://"));
    resto.is_some_and(|r| {
        r.len() > "/ws".len() && r.ends_with("/ws") && !r.contains(char::is_whitespace)
    })
}

#[derive(Clone)]
struct Linha {
    hora: String,
    texto: String,
    erro: bool,
}

fn registrar(reg: RwSignal<Vec<Linha>>, texto: impl Into<String>, erro: bool) {
    let hora = String::from(Date::new_0().to_locale_time_string("pt-BR"));
    reg.update(|v| {
        v.insert(
            0,
            Linha {
                hora,
                texto: texto.into(),
                erro,
            },
        )
    });
}

const MENU: [(&str, [&str; 2]); 5] = [
    ("Operação", ["Visão geral", "Jogo"]),
    ("Atividades", ["Caçada", "Combate"]),
    ("Economia", ["Mercado", "Finanças"]),
    ("Conhecimento", ["Bestiário", ""]),
    ("Sistema", ["Automação", "Configurações"]),
];

#[derive(Clone, Copy)]
struct Estado {
    url: RwSignal<String>,
    token: RwSignal<String>,
    contas_txt: RwSignal<String>,
    modo: RwSignal<String>,
    tem_token: RwSignal<Option<Result<bool, String>>>,
    abertas: RwSignal<Vec<String>>,
    ocupado: RwSignal<bool>,
    reg: RwSignal<Vec<Linha>>,
}

fn atualizar_token(e: Estado) {
    spawn_local(async move {
        let r = invoke("tem_token", args(&[]))
            .await
            .map(|v| v.as_bool().unwrap_or(false))
            .map_err(|er| erro_texto(&er));
        e.tem_token.set(Some(r));
    });
}

fn salvar_token(e: Estado) {
    let valor = e.token.get_untracked();
    spawn_local(async move {
        match invoke(
            "salvar_token",
            args(&[("token", JsValue::from_str(&valor))]),
        )
        .await
        {
            Ok(_) => {
                e.token.set(String::new());
                registrar(e.reg, "Token salvo.", false);
            }
            Err(er) => registrar(e.reg, format!("Token: {}", erro_texto(&er)), true),
        }
        atualizar_token(e);
    });
}

fn conectar(e: Estado) {
    let u = e.url.get_untracked().trim().to_owned();
    let contas = lista_contas(&e.contas_txt.get_untracked());
    let m = e.modo.get_untracked();
    if !url_valida(&u) {
        registrar(
            e.reg,
            "Endereço inválido: configure wss://…/ws em Configurações.",
            true,
        );
        return;
    }
    if contas.is_empty() {
        registrar(
            e.reg,
            "Informe pelo menos uma conta em Configurações.",
            true,
        );
        return;
    }
    e.ocupado.set(true);
    spawn_local(async move {
        let arr: Array = contas.iter().map(|c| JsValue::from_str(c)).collect();
        let a = args(&[
            ("url", JsValue::from_str(&u)),
            ("contas", arr.into()),
            ("modo", JsValue::from_str(&m)),
        ]);
        match invoke("iniciar_nucleo", a).await {
            Ok(_) => {
                registrar(
                    e.reg,
                    format!("Núcleo iniciado em modo {m} com {} conta(s).", contas.len()),
                    false,
                );
                e.abertas.set(contas);
            }
            Err(er) => registrar(e.reg, format!("Conectar: {}", erro_texto(&er)), true),
        }
        e.ocupado.set(false);
    });
}

fn abrir_tracker(e: Estado) {
    spawn_local(async move {
        if let Err(er) = invoke("abrir_tracker", args(&[])).await {
            registrar(e.reg, format!("Tracker: {}", erro_texto(&er)), true);
        }
    });
}

fn abrir_conta(e: Estado, conta: String) {
    spawn_local(async move {
        if let Err(er) = invoke("abrir_conta", args(&[("conta", JsValue::from_str(&conta))])).await
        {
            registrar(e.reg, format!("{conta}: {}", erro_texto(&er)), true);
        }
    });
}

#[component]
fn Configuracoes(e: Estado) -> impl IntoView {
    view! {
        <h2>"Configurações"</h2>
        <div class="limite">
            <span class="marca">"ID: CF-001"</span>
            <label class="campo" for="url">"Endereço do Nexus"</label>
            <input id="url" placeholder="wss://seu-servidor/ws" autocomplete="off" spellcheck="false"
                prop:value=move || e.url.get()
                on:input=move |ev| {
                    let v = event_target_value(&ev);
                    guardar(CHAVE_URL, v.trim());
                    e.url.set(v);
                } />
            <div class="dica">"O endereço WebSocket do Nexus, terminado em /ws."</div>

            <label class="campo" for="contas">"Contas"</label>
            <input id="contas" placeholder="Conta A, Conta B" autocomplete="off" spellcheck="false"
                prop:value=move || e.contas_txt.get()
                on:input=move |ev| {
                    let v = event_target_value(&ev);
                    guardar(CHAVE_CONTAS, &lista_contas(&v).join(", "));
                    e.contas_txt.set(v);
                } />
            <div class="dica">"Separadas por vírgula. Cada conta abre a própria janela do jogo, com login separado."</div>

            <label class="campo" for="token">"Tokens de acesso"</label>
            <div class="linha">
                <input id="token" type="password" placeholder="cole o token aqui" autocomplete="off"
                    prop:value=move || e.token.get()
                    on:input=move |ev| e.token.set(event_target_value(&ev)) />
                <button class="btn prim" on:click=move |_| salvar_token(e)>"Salvar"</button>
            </div>
            {move || match e.tem_token.get() {
                None => view! { <div class="dica">"Verificando…"</div> }.into_any(),
                Some(Ok(true)) => view! { <div class="dica ok">"▲ Token salvo neste PC"</div> }.into_any(),
                Some(Ok(false)) => view! { <div class="dica erro">"▼ Nenhum token salvo"</div> }.into_any(),
                Some(Err(er)) => view! { <div class="dica erro">{format!("Erro ao ler o token: {er}")}</div> }.into_any(),
            }}
        </div>
    }
}

#[component]
fn Diario(reg: RwSignal<Vec<Linha>>) -> impl IntoView {
    view! {
        <h2 class="sub">"Diário de eventos"</h2>
        <div class="diario">
            {move || {
                let v = reg.get();
                if v.is_empty() {
                    view! { <div class="vazio">"Nenhum evento ainda."</div> }.into_any()
                } else {
                    v.into_iter()
                        .map(|l| {
                            let cls = if l.erro { "evento erro" } else { "evento" };
                            view! {
                                <div class=cls>
                                    <span class="hora">{l.hora}</span>
                                    <span class="txt">{l.texto}</span>
                                </div>
                            }
                        })
                        .collect_view()
                        .into_any()
                }
            }}
        </div>
    }
}

#[component]
fn Jogo(e: Estado) -> impl IntoView {
    view! {
        <h2>"Jogo"</h2>
        <p class="pergunta">"Como estão as contas no jogo agora, e posso intervir?"</p>
        <label class="campo" for="modo">"Modo"</label>
        <select id="modo" class="caixa limite"
            prop:value=move || e.modo.get()
            on:change=move |ev| {
                let v = event_target_value(&ev);
                guardar(CHAVE_MODO, &v);
                e.modo.set(v);
            }>
            <option value="sombra">"Sombra: só coleta, não executa comandos"</option>
            <option value="ativo">"Ativo: coleta e executa comandos do Nexus"</option>
        </select>
        <div class="acoes">
            <button class="btn prim" prop:disabled=move || e.ocupado.get() on:click=move |_| conectar(e)>"Conectar"</button>
            <button class="btn" on:click=move |_| abrir_tracker(e)>"Abrir Tracker"</button>
        </div>
        <label class="campo">"Janelas das contas"</label>
        <div class="acoes">
            {move || {
                let v = e.abertas.get();
                if v.is_empty() {
                    view! { <span class="dica">"Conecte para abrir uma janela por conta."</span> }.into_any()
                } else {
                    v.into_iter()
                        .map(|c| {
                            let nome = c.clone();
                            view! {
                                <button class="btn" title="Abrir ou focar a janela desta conta"
                                    on:click=move |_| abrir_conta(e, c.clone())>{nome}</button>
                            }
                        })
                        .collect_view()
                        .into_any()
                }
            }}
        </div>
        <Diario reg=e.reg />
    }
}

#[component]
fn App() -> impl IntoView {
    let url = guardado(CHAVE_URL);
    let inicial = if url_valida(&url) {
        "Jogo"
    } else {
        "Configurações"
    };
    let e = Estado {
        url: RwSignal::new(url),
        token: RwSignal::new(String::new()),
        contas_txt: RwSignal::new(guardado(CHAVE_CONTAS)),
        modo: RwSignal::new(match guardado(CHAVE_MODO).as_str() {
            "ativo" => "ativo".to_owned(),
            _ => "sombra".to_owned(),
        }),
        tem_token: RwSignal::new(None),
        abertas: RwSignal::new(Vec::new()),
        ocupado: RwSignal::new(false),
        reg: RwSignal::new(Vec::new()),
    };
    let pagina = RwSignal::new(inicial);
    let conta_ativa = RwSignal::new(String::from("Todas as contas"));
    atualizar_token(e);

    spawn_local(async move {
        let tratador = Closure::<dyn FnMut(JsValue)>::new(move |ev: JsValue| {
            let msg = Reflect::get(&ev, &JsValue::from_str("payload"))
                .ok()
                .and_then(|p| p.as_string())
                .unwrap_or_else(|| "Núcleo parou.".to_owned());
            registrar(e.reg, msg, true);
        });
        match listen("nucleo", &tratador).await {
            // O tratador precisa viver enquanto o app viver.
            Ok(_) => tratador.forget(),
            Err(er) => registrar(
                e.reg,
                format!("Ponte do app indisponível: {}", erro_texto(&er)),
                true,
            ),
        }
    });

    view! {
        <style>{CSS}</style>
        <div class="nevoa"></div>
        <div class="casca">
            <nav class="barra vidro">
                <div class="logo">"Nexus"</div>
                {MENU
                    .iter()
                    .map(|(grupo, itens)| {
                        view! {
                            <div class="grupo">{*grupo}</div>
                            {itens
                                .iter()
                                .filter(|i| !i.is_empty())
                                .map(|i| {
                                    let nome: &'static str = i;
                                    view! {
                                        <button class="item" class:ativo=move || pagina.get() == nome
                                            on:click=move |_| pagina.set(nome)>{nome}</button>
                                    }
                                })
                                .collect_view()}
                        }
                    })
                    .collect_view()}
            </nav>
            <div class="palco">
                <div class="topo">
                    <div class="pilula vidro">
                        <select on:change=move |ev| conta_ativa.set(event_target_value(&ev))>
                            <option>"Todas as contas"</option>
                            {move || lista_contas(&e.contas_txt.get())
                                .into_iter()
                                .map(|c| view! { <option>{c}</option> })
                                .collect_view()}
                        </select>
                    </div>
                </div>
                <main class="conteudo vidro">
                    {move || match pagina.get() {
                        "Configurações" => view! { <Configuracoes e=e /> }.into_any(),
                        "Jogo" => view! { <Jogo e=e /> }.into_any(),
                        outra => view! {
                            <h2>{outra}</h2>
                            <div class="vazio">"Em construção."</div>
                        }.into_any(),
                    }}
                </main>
            </div>
        </div>
    }
}

fn main() {
    leptos::mount::mount_to_body(App);
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn contas_ignoram_vazios() {
        assert_eq!(lista_contas(" A, ,B ,"), vec!["A", "B"]);
    }

    #[test]
    fn url_exige_esquema_e_ws() {
        assert!(url_valida("wss://x.exemplo/ws"));
        assert!(!url_valida("https://x/ws"));
        assert!(!url_valida("wss://x/outro"));
        assert!(!url_valida("wss:///ws"));
    }
}
