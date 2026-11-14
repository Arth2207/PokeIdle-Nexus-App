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
.logo { display:flex; align-items:center; gap:8px; }
.logo .ico { width:28px; height:28px; color:var(--melhora); }
.item { display:flex; align-items:center; gap:10px; }
.ico { width:18px; height:18px; flex:none; }
.rodape-conta { margin-top:auto; padding:10px 12px; border-radius:16px; background:rgba(255,255,255,.55); border:1px solid var(--vidro-borda); display:flex; flex-direction:column; gap:6px; }
.rodape-conta .nome { display:flex; align-items:center; gap:8px; font-weight:600; }
.topo { padding:0 4px; }
.topo .esp { flex:1; }
.pilula .ico { width:16px; height:16px; }
button.pilula { border:1px solid var(--vidro-borda); font:inherit; font-weight:600; color:var(--texto); cursor:pointer; }
.chave { width:38px; height:22px; border-radius:999px; background:var(--trilho); position:relative; display:inline-block; }
.chave::after { content:""; position:absolute; top:3px; left:3px; width:16px; height:16px; border-radius:50%; background:#fff; transition:left .15s; }
.chave.on { background:var(--neon); }
.chave.on::after { left:19px; }
.conteudo.jogo { padding:18px 20px; display:flex; flex-direction:column; gap:14px; overflow:hidden; }
.oculto, .oculto > * { display:none !important; }
.resumo { display:flex; align-items:center; justify-content:space-between; gap:12px; }
.resumo .txt { font:700 15px "Space Grotesk", Inter, sans-serif; letter-spacing:.04em; text-transform:uppercase; }
.seg { display:flex; border-radius:999px; background:rgba(255,255,255,.55); padding:3px; border:1px solid var(--vidro-borda); }
.seg button { border:0; background:transparent; border-radius:999px; padding:7px 16px; font:inherit; font-weight:600; color:var(--texto); display:flex; align-items:center; gap:6px; cursor:pointer; }
.seg button.ativo { background:var(--neon); color:var(--sobre-neon); }
.grade { flex:1; min-height:0; display:grid; grid-template-columns:1fr 1fr; grid-template-rows:1fr 1fr; gap:14px; }
.grade.foco { grid-template-columns:1fr; grid-template-rows:1fr; }
.card { min-height:0; border-radius:var(--raio); background:rgba(255,255,255,.55); border:1px solid var(--vidro-borda); padding:12px 14px; display:flex; flex-direction:column; gap:10px; }
.card .cab { display:flex; align-items:center; gap:12px; font:700 15px "Space Grotesk", Inter, sans-serif; letter-spacing:.06em; text-transform:uppercase; }
.card .cab .ico { width:22px; height:22px; color:var(--texto-2); }
.stat { display:inline-flex; align-items:center; gap:6px; padding:4px 12px; border-radius:999px; font:600 13px Inter, sans-serif; letter-spacing:0; text-transform:none; }
.stat .ico { width:14px; height:14px; }
.stat.execucao { background:var(--neon); color:var(--sobre-neon); }
.stat.pausada { background:var(--trilho); color:var(--texto-2); }
.stat.aguardando { background:var(--trilho); color:var(--texto-2); }
.stat.erro { background:rgba(229,83,61,.18); color:var(--piora); }
.stat.sem_coletor { background:rgba(229,83,61,.18); color:var(--piora); }
.corpo { flex:1; min-height:0; display:flex; gap:14px; }
.area { flex:1.7; min-width:0; border-radius:14px; background:rgba(22,24,28,.10); overflow:hidden; }
.area.demo { background:linear-gradient(135deg,#6aa86a,#2f6b3f); }
.info { flex:1; min-width:0; display:flex; flex-direction:column; justify-content:space-between; gap:6px; }
.info .l { font-size:14px; color:var(--texto); }
.info .l b { font-weight:600; font-size:17px; margin-left:6px; }
.info .l.baixo b { color:var(--piora); }
.info .hunt { font-weight:600; }
.redondos { display:flex; gap:10px; }
button.redondo { width:42px; height:42px; border-radius:50%; border:1px solid var(--vidro-borda); background:rgba(255,255,255,.85); color:var(--texto); display:flex; align-items:center; justify-content:center; cursor:pointer; box-shadow:0 2px 8px rgba(22,24,28,.10); }
button.redondo:disabled { opacity:.4; cursor:default; }
button.redondo .ico { width:20px; height:20px; }
.card.vazio-card { align-items:center; justify-content:center; color:var(--texto-3); }
.card.vazio-card button { border:0; background:transparent; color:var(--texto-2); font:inherit; font-weight:600; text-decoration:underline; cursor:pointer; }
.pe { display:flex; justify-content:space-between; color:var(--texto-2); font-size:13px; }
.pe span { display:flex; align-items:center; gap:8px; }
.pe .ico { width:16px; height:16px; }
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

/// (grupo, [(nome, slug, ícone)]).
type ItemMenu = (&'static str, &'static str, &'static str);
const MENU: [(&str, &[ItemMenu]); 5] = [
    (
        "Operação",
        &[
            ("Visão geral", "visao-geral", "visao"),
            ("Jogo", "jogo", "jogo"),
        ],
    ),
    (
        "Atividades",
        &[
            ("Caçada", "cacada", "cacada"),
            ("Combate", "combate", "combate"),
        ],
    ),
    (
        "Economia",
        &[
            ("Mercado", "mercado", "mercado"),
            ("Finanças", "financas", "financas"),
        ],
    ),
    ("Conhecimento", &[("Bestiário", "bestiario", "bestiario")]),
    (
        "Sistema",
        &[
            ("Automação", "automacao", "automacao"),
            ("Configurações", "configuracoes", "config"),
        ],
    ),
];

fn pagina_do_slug(slug: &str) -> Option<&'static str> {
    MENU.iter()
        .flat_map(|(_, itens)| itens.iter())
        .find(|(_, s, _)| *s == slug)
        .map(|(n, _, _)| *n)
}

fn slug_da_pagina(nome: &str) -> &'static str {
    MENU.iter()
        .flat_map(|(_, itens)| itens.iter())
        .find(|(n, _, _)| *n == nome)
        .map_or("jogo", |(_, s, _)| *s)
}

/// Ícones de linha fina (24x24, traço 1.6, `currentColor`), desenhados aqui, sem CDN.
fn icone(nome: &str) -> impl IntoView {
    let d = match nome {
        "visao" => {
            r#"<rect x="3.5" y="3.5" width="7" height="7" rx="2"/><rect x="13.5" y="3.5" width="7" height="7" rx="2"/><rect x="3.5" y="13.5" width="7" height="7" rx="2"/><rect x="13.5" y="13.5" width="7" height="7" rx="2"/>"#
        }
        "jogo" => {
            r#"<rect x="2.5" y="7" width="19" height="10" rx="5"/><path d="M8 10v4M6 12h4"/><path d="M16 11.2v.01M18.2 13v.01"/>"#
        }
        "cacada" => {
            r#"<circle cx="12" cy="12" r="7"/><path d="M12 2.5v5M12 16.5v5M2.5 12h5M16.5 12h5"/>"#
        }
        "combate" => r#"<path d="M5 5l14 14M19 5L5 19M4 8l4-4M20 8l-4-4"/>"#,
        "mercado" => r#"<path d="M5 20V11M12 20V4M19 20v-7"/>"#,
        "financas" => {
            r#"<rect x="3" y="6" width="18" height="13" rx="3"/><path d="M3 10h18M16 14.5h2"/>"#
        }
        "bestiario" => r#"<path d="M12 3l8 3v6c0 4.5-3.2 7.6-8 9-4.8-1.4-8-4.5-8-9V6z"/>"#,
        "automacao" => r#"<path d="M13 3L5 14h6l-1 7 8-11h-6z"/>"#,
        "config" => {
            r#"<path d="M4 7h9M17 7h3M4 17h3M11 17h9"/><circle cx="15" cy="7" r="2"/><circle cx="9" cy="17" r="2"/>"#
        }
        "conta" => r#"<circle cx="12" cy="8" r="4"/><path d="M4.5 20c1-4 4-6 7.5-6s6.5 2 7.5 6"/>"#,
        "folha" => {
            r#"<path d="M5 19c0-8 5-14 15-14 0 10-6 15-14 15"/><path d="M5 19c3-5 6-8 10-10"/>"#
        }
        "busca" => r#"<circle cx="11" cy="11" r="6.5"/><path d="M16 16l4.5 4.5"/>"#,
        "sino" => r#"<path d="M6 16V11a6 6 0 0112 0v5l1.5 2h-15z"/><path d="M10 21h4"/>"#,
        "chevron" => r#"<path d="M6 9l6 6 6-6"/>"#,
        "grade" => {
            r#"<rect x="3.5" y="3.5" width="7" height="7" rx="2"/><rect x="13.5" y="3.5" width="7" height="7" rx="2"/><rect x="3.5" y="13.5" width="7" height="7" rx="2"/><rect x="13.5" y="13.5" width="7" height="7" rx="2"/>"#
        }
        "foco" => r#"<path d="M4 9V5h4M20 9V5h-4M4 15v4h4M20 15v4h-4"/>"#,
        "pausar" => r#"<path d="M9 5v14M15 5v14"/>"#,
        "retomar" => r#"<path d="M7 4.5l12 7.5-12 7.5z"/>"#,
        "recarregar" => r#"<path d="M20 12a8 8 0 11-2.6-5.9"/><path d="M20 4v5h-5"/>"#,
        "expandir" => r#"<path d="M14 4h6v6M20 4l-7 7M10 20H4v-6M4 20l7-7"/>"#,
        "alerta" => r#"<path d="M12 3.5l9.5 16.5h-19z"/><path d="M12 10v4.5M12 17.2v.01"/>"#,
        "execucao" => r#"<circle cx="12" cy="12" r="8.5"/><path d="M8.5 12.3l2.4 2.4 4.6-5"/>"#,
        "estado-pausa" => r#"<circle cx="12" cy="12" r="8.5"/><path d="M10 9v6M14 9v6"/>"#,
        "monitor" => {
            r#"<rect x="3" y="4.5" width="18" height="12" rx="2.5"/><path d="M9 20h6M12 16.5V20"/>"#
        }
        _ => "",
    };
    view! {
        <svg class="ico" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6"
            stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" inner_html=d></svg>
    }
}

/// Dados de um card, como o Rust os publica no evento `contas`.
#[derive(Clone, PartialEq, Debug)]
struct ContaCard {
    conta: String,
    status: String,
    hunt: Option<String>,
    kills_h: Option<i64>,
    hp_min: Option<i64>,
    captura_ok: bool,
}

fn campo_js(v: &JsValue, k: &str) -> Option<JsValue> {
    Reflect::get(v, &JsValue::from_str(k))
        .ok()
        .filter(|x| !x.is_null() && !x.is_undefined())
}

fn le_card(v: &JsValue) -> Option<ContaCard> {
    Some(ContaCard {
        conta: campo_js(v, "conta")?.as_string()?,
        status: campo_js(v, "status")?.as_string()?,
        hunt: campo_js(v, "hunt").and_then(|x| x.as_string()),
        kills_h: campo_js(v, "kills_h")
            .and_then(|x| x.as_f64())
            .map(|n| n as i64),
        hp_min: campo_js(v, "hp_min")
            .and_then(|x| x.as_f64())
            .map(|n| n as i64),
        captura_ok: campo_js(v, "captura_ok")
            .and_then(|x| x.as_bool())
            .unwrap_or(false),
    })
}

/// 2340 -> "2.340".
fn milhar(n: i64) -> String {
    let d = n.abs().to_string();
    let mut o = String::new();
    for (i, c) in d.chars().enumerate() {
        if i > 0 && (d.len() - i).is_multiple_of(3) {
            o.push('.');
        }
        o.push(c);
    }
    if n < 0 {
        format!("-{o}")
    } else {
        o
    }
}

/// "bosque-nebuloso" -> "Bosque nebuloso".
fn titulo_slug(slug: &str) -> String {
    let t = slug.replace(['-', '_'], " ");
    let mut c = t.chars();
    c.next()
        .map(|p| p.to_uppercase().collect::<String>() + c.as_str())
        .unwrap_or_default()
}

/// HP mínimo abaixo disto fica em vermelho.
const HP_BAIXO: i64 = 30;

/// Resumo da linha do topo da tela Jogo.
fn resumo(total: usize, cards: &[ContaCard]) -> String {
    let exec = cards.iter().filter(|c| c.status != "pausada").count();
    let paus = cards.iter().filter(|c| c.status == "pausada").count();
    let ok = cards.iter().filter(|c| c.captura_ok).count();
    format!("{total} contas · {exec} em execução · {paus} pausada · captura ok em {ok} de {total}")
}

fn tem_tauri() -> bool {
    web_sys::window()
        .and_then(|j| Reflect::get(&j, &JsValue::from_str("__TAURI__")).ok())
        .is_some_and(|v| v.is_object())
}

/// Modo de demonstração: só fora do app (sem `__TAURI__`) e só com `?demo=1`.
fn demo_ativo() -> bool {
    !tem_tauri()
        && web_sys::window()
            .and_then(|j| j.location().search().ok())
            .is_some_and(|q| q.contains("demo=1"))
}

fn demo_cards() -> Vec<ContaCard> {
    let c = |n: &str, st: &str, h: &str, k: Option<i64>, hp: Option<i64>, ok: bool| ContaCard {
        conta: n.into(),
        status: st.into(),
        hunt: Some(h.into()),
        kills_h: k,
        hp_min: hp,
        captura_ok: ok,
    };
    vec![
        c(
            "Conta A",
            "execucao",
            "bosque-nebuloso",
            Some(2340),
            Some(35),
            true,
        ),
        c(
            "Conta B",
            "execucao",
            "riacho-cristalino",
            Some(2180),
            Some(40),
            true,
        ),
        c("Conta C", "pausada", "pico-nevado", None, None, true),
        c(
            "Conta D",
            "erro",
            "gruta-turquesa",
            Some(1920),
            Some(28),
            false,
        ),
    ]
}

#[derive(Clone, Copy)]
struct Estado {
    url: RwSignal<String>,
    token: RwSignal<String>,
    contas_txt: RwSignal<String>,
    modo: RwSignal<String>,
    tem_token: RwSignal<Option<Result<bool, String>>>,
    pagina: RwSignal<&'static str>,
    foco: RwSignal<Option<String>>,
    cards: RwSignal<Vec<ContaCard>>,
    ultima: RwSignal<String>,
    conta_ativa: RwSignal<String>,
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

/// Grava endereço, contas e modo (também é gravado a cada edição).
fn salvar_config(e: Estado) {
    guardar(CHAVE_URL, e.url.get_untracked().trim());
    guardar(
        CHAVE_CONTAS,
        &lista_contas(&e.contas_txt.get_untracked()).join(", "),
    );
    guardar(CHAVE_MODO, &e.modo.get_untracked());
    registrar(e.reg, "Configurações salvas.", false);
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
            <label class="campo" for="modo">"Modo"</label>
            <select id="modo" class="caixa"
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
                <button class="btn prim" on:click=move |_| salvar_config(e)>"Salvar"</button>
                <button class="btn" prop:disabled=move || e.ocupado.get() on:click=move |_| conectar(e)>"Conectar"</button>
                <button class="btn" on:click=move |_| abrir_tracker(e)>"Abrir Tracker"</button>
            </div>
            {move || match e.tem_token.get() {
                None => view! { <div class="dica">"Verificando…"</div> }.into_any(),
                Some(Ok(true)) => view! { <div class="dica ok">"▲ Token salvo neste PC"</div> }.into_any(),
                Some(Ok(false)) => view! { <div class="dica erro">"▼ Nenhum token salvo"</div> }.into_any(),
                Some(Err(er)) => view! { <div class="dica erro">{format!("Erro ao ler o token: {er}")}</div> }.into_any(),
            }}
        </div>
        <Diario reg=e.reg />
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

/// Envia ao Rust o retângulo da área do jogo (px CSS da janela).
fn enviar_rect(e: Estado, conta: &str, el: &web_sys::Element) {
    if !tem_tauri() {
        return;
    }
    let r = el.get_bounding_client_rect();
    let visivel = r.width() >= 1.0 && r.height() >= 1.0;
    let a = args(&[
        ("conta", JsValue::from_str(conta)),
        ("x", JsValue::from_f64(r.x())),
        ("y", JsValue::from_f64(r.y())),
        ("w", JsValue::from_f64(r.width())),
        ("h", JsValue::from_f64(r.height())),
        ("visivel", JsValue::from_bool(visivel)),
    ]);
    let nome = conta.to_owned();
    spawn_local(async move {
        if let Err(er) = invoke("posicionar_conta", a).await {
            registrar(
                e.reg,
                format!("{nome}: posicionar jogo: {}", erro_texto(&er)),
                true,
            );
        }
    });
}

fn comando_conta(e: Estado, cmd: &'static str, conta: String, extra: Option<(&str, bool)>) {
    let mut pares = vec![("conta", JsValue::from_str(&conta))];
    if let Some((k, v)) = extra {
        pares.push((k, JsValue::from_bool(v)));
    }
    let a = args(&pares);
    spawn_local(async move {
        if let Err(er) = invoke(cmd, a).await {
            registrar(e.reg, format!("{conta}: {}", erro_texto(&er)), true);
        }
    });
}

#[component]
fn Slot(i: usize, e: Estado) -> impl IntoView {
    let conta = move || lista_contas(&e.contas_txt.get()).get(i).cloned();
    let area = NodeRef::<leptos::html::Div>::new();
    let dado = move || {
        let c = conta()?;
        e.cards.get().into_iter().find(|d| d.conta == c)
    };
    let escondido = move || match (e.foco.get(), conta()) {
        (Some(f), Some(c)) => f != c,
        (Some(_), None) => true,
        _ => false,
    };
    // O observador guarda o elemento; reenvia quando o tamanho muda (inclui display:none).
    Effect::new(move |_| {
        if let Some(el) = area.get() {
            let el: web_sys::Element = el.into();
            let alvo = el.clone();
            let cb = Closure::<dyn FnMut(Array, web_sys::ResizeObserver)>::new(
                move |_: Array, _: web_sys::ResizeObserver| {
                    if let Some(c) = conta() {
                        enviar_rect(e, &c, &alvo);
                    }
                },
            );
            if let Ok(obs) = web_sys::ResizeObserver::new(cb.as_ref().unchecked_ref()) {
                obs.observe(&el);
            }
            cb.forget();
            let alvo = el.clone();
            let rs = Closure::<dyn FnMut()>::new(move || {
                if let Some(c) = conta() {
                    enviar_rect(e, &c, &alvo);
                }
            });
            if let Some(j) = web_sys::window() {
                let _ = j.add_event_listener_with_callback("resize", rs.as_ref().unchecked_ref());
            }
            rs.forget();
        }
    });
    // Conta trocada ou card voltou a aparecer: reenvia.
    Effect::new(move |_| {
        let _ = (conta(), e.pagina.get(), e.foco.get());
        if let (Some(el), Some(c)) = (area.get(), conta()) {
            enviar_rect(e, &c, &el.into());
        }
    });
    let demo = demo_ativo();
    view! {
        <div class="card" class:oculto=escondido class:vazio-card=move || conta().is_none()>
            {move || match conta() {
                None => view! {
                    <button on:click=move |_| e.pagina.set("Configurações")>"Adicionar conta em Configurações"</button>
                }.into_any(),
                Some(nome) => {
                    let st = move || dado().map_or_else(|| "execucao".to_owned(), |d| d.status);
                    let n_pausar = nome.clone();
                    let n_rec = nome.clone();
                    let n_exp = nome.clone();
                    view! {
                        <div class="cab">
                            {icone("conta")}
                            <span>{nome.clone()}</span>
                            <span class={move || format!("stat {}", st())}>
                                {move || match st().as_str() {
                                    "pausada" => view! { {icone("estado-pausa")} "Pausada" }.into_any(),
                                    "erro" => view! { {icone("alerta")} "Erro de captura" }.into_any(),
                                    "aguardando" => view! { {icone("estado-pausa")} "Aguardando jogo" }.into_any(),
                                    "sem_coletor" => view! { {icone("alerta")} "Coletor sem resposta" }.into_any(),
                                    _ => view! { {icone("execucao")} "Em execução" }.into_any(),
                                }}
                            </span>
                        </div>
                        <div class="corpo">
                            <div class="area" class:demo=demo node_ref=area></div>
                            <div class="info">
                                <div class="l hunt">"Hunt: "{move || dado().and_then(|d| d.hunt).map_or("—".to_owned(), |h| titulo_slug(&h))}</div>
                                <div class="l">"Kills/h:"<b>{move || dado().and_then(|d| d.kills_h).map_or("—".to_owned(), milhar)}</b></div>
                                <div class="l" class:baixo=move || dado().and_then(|d| d.hp_min).is_some_and(|h| h < HP_BAIXO)>
                                    "HP mín.:"<b>{move || dado().and_then(|d| d.hp_min).map_or("—".to_owned(), |h| format!("{h}%"))}</b>
                                </div>
                                <div class="redondos">
                                    <button class="redondo" title="Pausar ou retomar a coleta e os comandos desta conta"
                                        on:click=move |_| comando_conta(e, "pausar_conta", n_pausar.clone(), Some(("pausada", st() != "pausada")))>
                                        {move || if st() == "pausada" { icone("retomar").into_any() } else { icone("pausar").into_any() }}
                                    </button>
                                    <button class="redondo" title="Recarregar o jogo desta conta"
                                        on:click=move |_| comando_conta(e, "recarregar_conta", n_rec.clone(), None)>
                                        {icone("recarregar")}
                                    </button>
                                    <button class="redondo" title="Expandir (modo Foco) ou voltar à grade"
                                        on:click=move |_| {
                                            let atual = e.foco.get_untracked();
                                            e.foco.set(if atual.as_deref() == Some(n_exp.as_str()) { None } else { Some(n_exp.clone()) });
                                        }>
                                        {icone("expandir")}
                                    </button>
                                </div>
                            </div>
                        </div>
                    }.into_any()
                }
            }}
        </div>
    }
}

#[component]
fn Jogo(e: Estado) -> impl IntoView {
    let total = move || lista_contas(&e.contas_txt.get()).len();
    view! {
        <div class="resumo">
            <span class="txt">{move || resumo(total(), &e.cards.get())}</span>
            <div class="seg">
                <button class:ativo=move || e.foco.get().is_none() on:click=move |_| e.foco.set(None)>{icone("grade")}"Grade"</button>
                <button class:ativo=move || e.foco.get().is_some()
                    on:click=move |_| {
                        let primeira = lista_contas(&e.contas_txt.get_untracked()).first().cloned();
                        e.foco.set(primeira);
                    }>{icone("foco")}"Foco"</button>
            </div>
        </div>
        <div class="grade" class:foco=move || e.foco.get().is_some()>
            {(0..4).map(|i| view! { <Slot i=i e=e /> }).collect_view()}
        </div>
        <div class="pe">
            <span>{icone("monitor")}"Janelas fora de foco a 15 fps"</span>
            <span>"Última verificação: "{move || e.ultima.get()}</span>
        </div>
    }
}

#[component]
fn App() -> impl IntoView {
    let url = guardado(CHAVE_URL);
    let hash = web_sys::window()
        .and_then(|j| j.location().hash().ok())
        .unwrap_or_default();
    let inicial = pagina_do_slug(hash.trim_start_matches('#')).unwrap_or(if url_valida(&url) {
        "Jogo"
    } else {
        "Configurações"
    });
    let demo = demo_ativo();
    let e = Estado {
        url: RwSignal::new(url),
        token: RwSignal::new(String::new()),
        contas_txt: RwSignal::new(if demo {
            "Conta A, Conta B, Conta C, Conta D".to_owned()
        } else {
            guardado(CHAVE_CONTAS)
        }),
        modo: RwSignal::new(match guardado(CHAVE_MODO).as_str() {
            "ativo" => "ativo".to_owned(),
            _ => "sombra".to_owned(),
        }),
        tem_token: RwSignal::new(None),
        pagina: RwSignal::new(inicial),
        foco: RwSignal::new(None),
        cards: RwSignal::new(if demo { demo_cards() } else { Vec::new() }),
        ultima: RwSignal::new(if demo {
            "14:27:16".to_owned()
        } else {
            "—".to_owned()
        }),
        conta_ativa: RwSignal::new(String::new()),
        ocupado: RwSignal::new(false),
        reg: RwSignal::new(Vec::new()),
    };
    let pagina = e.pagina;
    let periodo = RwSignal::new(String::from("Últimos 7 dias"));
    let moeda = RwSignal::new(String::from("Orbs"));
    let desempenho = RwSignal::new(true);
    Effect::new(move |_| {
        if let Some(j) = web_sys::window() {
            let _ = j.location().set_hash(slug_da_pagina(pagina.get()));
        }
    });
    if tem_tauri() {
        atualizar_token(e);
        // Conecta sozinho se endereço e contas já estão salvos.
        if url_valida(&e.url.get_untracked())
            && !lista_contas(&e.contas_txt.get_untracked()).is_empty()
        {
            conectar(e);
        }
    }
    let ativa = move || {
        let a = e.conta_ativa.get();
        let l = lista_contas(&e.contas_txt.get());
        if l.contains(&a) {
            Some(a)
        } else {
            l.first().cloned()
        }
    };
    let status_ativa = move || {
        let a = ativa()?;
        e.cards
            .get()
            .into_iter()
            .find(|d| d.conta == a)
            .map(|d| d.status)
    };

    spawn_local(async move {
        if !tem_tauri() {
            return;
        }
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
        let cartoes = Closure::<dyn FnMut(JsValue)>::new(move |ev: JsValue| {
            if let Some(p) = campo_js(&ev, "payload") {
                let v: Vec<ContaCard> =
                    Array::from(&p).iter().filter_map(|x| le_card(&x)).collect();
                e.cards.set(v);
                e.ultima
                    .set(String::from(Date::new_0().to_locale_time_string("pt-BR")));
            }
        });
        if listen("contas", &cartoes).await.is_ok() {
            cartoes.forget();
        }
    });

    view! {
        <style>{CSS}</style>
        <div class="nevoa"></div>
        <div class="casca">
            <nav class="barra vidro">
                <div class="logo">{icone("folha")}"Nexus"</div>
                {MENU
                    .iter()
                    .map(|(grupo, itens)| {
                        view! {
                            <div class="grupo">{*grupo}</div>
                            {itens
                                .iter()
                                .map(|(nome, _, ic)| {
                                    let nome: &'static str = nome;
                                    view! {
                                        <button class="item" class:ativo=move || pagina.get() == nome
                                            on:click=move |_| pagina.set(nome)>{icone(ic)}{nome}</button>
                                    }
                                })
                                .collect_view()}
                        }
                    })
                    .collect_view()}
                <div class="rodape-conta">
                    <div class="nome">{icone("conta")}{move || ativa().unwrap_or_else(|| "Sem conta".to_owned())}</div>
                    {move || {
                        let st = status_ativa().unwrap_or_else(|| "execucao".to_owned());
                        let (ic, tx) = match st.as_str() {
                            "pausada" => ("estado-pausa", "Pausada"),
                            "erro" => ("alerta", "Erro de captura"),
                            "aguardando" => ("estado-pausa", "Aguardando jogo"),
                            "sem_coletor" => ("alerta", "Coletor sem resposta"),
                            _ => ("execucao", "Em execução"),
                        };
                        view! { <span class=format!("stat {st}")>{icone(ic)}{tx}</span> }
                    }}
                </div>
            </nav>
            <div class="palco">
                <div class="topo">
                    <div class="pilula vidro">
                        <select on:change=move |ev| e.conta_ativa.set(event_target_value(&ev))>
                            {move || lista_contas(&e.contas_txt.get())
                                .into_iter()
                                .map(|c| view! { <option>{c}</option> })
                                .collect_view()}
                        </select>
                    </div>
                    <div class="pilula vidro">
                        <select prop:value=move || periodo.get() on:change=move |ev| periodo.set(event_target_value(&ev))>
                            <option>"Hoje"</option>
                            <option>"Últimos 7 dias"</option>
                            <option>"Últimos 30 dias"</option>
                        </select>
                    </div>
                    <div class="pilula vidro">
                        <select prop:value=move || moeda.get() on:change=move |ev| moeda.set(event_target_value(&ev))>
                            <option>"Orbs"</option>
                            <option>"Gold"</option>
                        </select>
                    </div>
                    <span class="esp"></span>
                    <button class="pilula vidro" title="Busca rápida (em breve)">{icone("busca")}"⌘K"</button>
                    <button class="pilula vidro" title="Alertas">{icone("sino")}</button>
                    <button class="pilula vidro" title="Modo desempenho" on:click=move |_| desempenho.update(|v| *v = !*v)>
                        "Modo desempenho"
                        <span class="chave" class:on=move || desempenho.get()></span>
                    </button>
                </div>
                <main class="conteudo vidro" class:jogo=move || pagina.get() == "Jogo">
                    <div class="oculto-jogo" style="display:contents">
                        <div class:oculto=move || pagina.get() != "Jogo" style="display:contents">
                            <Jogo e=e />
                        </div>
                    </div>
                    {move || match pagina.get() {
                        "Configurações" => view! { <Configuracoes e=e /> }.into_any(),
                        "Jogo" => ().into_any(),
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
