//! Elementos de toda tela (`docs/design/telas.md`, Navegação): o fundo pelo bioma da hunt da
//! conta ativa (escurecido à noite), a busca rápida (⌘K) e a central de alertas.

use leptos::prelude::*;
use serde_json::Value;
use wasm_bindgen::JsCast;

use crate::dados::{lista, num, txt};
use crate::{icone, lista_contas, Estado, MENU};

/// Noite pelo relógio do computador (18 h às 6 h).
fn noite() -> bool {
    let h = js_sys::Date::new_0().get_hours();
    !(6..18).contains(&h)
}

/// Tema guardado no navegador: "" (automático pelo relógio), "Claro" ou "Escuro".
pub const CHAVE_TEMA: &str = "nexus.tema";

/// Troca o fundo quando muda a conta ativa, o bioma dela ou o tema.
pub fn fundo(e: Estado, tema: RwSignal<String>) {
    Effect::new(move |_| {
        let ativa = e.conta_ativa.get();
        let contas = lista_contas(&e.contas_txt.get());
        let rotulo = if contas.contains(&ativa) {
            Some(ativa)
        } else {
            contas.first().cloned()
        };
        let bioma = rotulo.and_then(|r| {
            e.biomas
                .get()
                .into_iter()
                .find(|(c, _)| c.eq_ignore_ascii_case(&r))
                .map(|(_, b)| b)
        });
        let Some(corpo) = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.body())
        else {
            return;
        };
        let n = match tema.get().as_str() {
            "Claro" => false,
            "Escuro" => true,
            _ => noite(),
        };
        // Só campos e penhascos tem a versão noturna; os outros escurecem por CSS.
        let arquivo = match (bioma.as_deref(), n) {
            (Some("campos-penhascos"), true) => "bioma-campos-penhascos-noite".to_owned(),
            (Some(b), _) => format!("bioma-{b}-dia"),
            (None, _) => "bioma-floresta-dia".to_owned(),
        };
        let estilo: &web_sys::HtmlElement = corpo.unchecked_ref();
        let _ = estilo.style().set_property(
            "background-image",
            &format!("url(\"biomas/{arquivo}.webp\")"),
        );
        let _ = corpo.class_list().toggle_with_force("noite", n);
    });
}

/// Botão do tema: Auto → Claro → Escuro.
#[component]
pub fn Tema(tema: RwSignal<String>) -> impl IntoView {
    let proximo = move |_| {
        let novo = match tema.get_untracked().as_str() {
            "" => "Claro",
            "Claro" => "Escuro",
            _ => "",
        };
        crate::guardar(CHAVE_TEMA, novo);
        tema.set(novo.to_owned());
    };
    view! {
        <button class="pilula vidro" title="Tema (automático segue o relógio: escuro das 18 h às 6 h)" on:click=proximo>
            {move || match tema.get().as_str() {
                "Claro" => "☀ Claro",
                "Escuro" => "☾ Escuro",
                _ => "◐ Auto",
            }}
        </button>
    }
}

/// Busca rápida: telas e contas; Enter ou clique leva até lá.
#[component]
pub fn Busca(e: Estado) -> impl IntoView {
    let aberta = RwSignal::new(false);
    let q = RwSignal::new(String::new());
    let opcoes = move || {
        let q = q.get().to_lowercase();
        let mut v: Vec<(String, &'static str, Option<String>)> = MENU
            .iter()
            .flat_map(|(_, itens)| itens.iter())
            .map(|(n, _, _)| (format!("Ir para {n}"), *n, None))
            .collect();
        v.extend(
            lista_contas(&e.contas_txt.get())
                .into_iter()
                .map(|c| (format!("Conta {c}"), "Visão geral", Some(c))),
        );
        v.retain(|o| q.is_empty() || o.0.to_lowercase().contains(&q));
        v
    };
    let ir = move |pagina: &'static str, conta: Option<String>| {
        if let Some(c) = conta {
            e.conta_ativa.set(c);
        }
        e.pagina.set(pagina);
        aberta.set(false);
        q.set(String::new());
    };
    view! {
        <button class="pilula vidro" title="Busca rápida" on:click=move |_| aberta.set(true)>{icone("busca")}"⌘K"</button>
        {move || aberta.get().then(|| view! {
            <div class="sobre" on:click=move |_| aberta.set(false)>
                <div class="paleta vidro" on:click=|ev| ev.stop_propagation()>
                    <input class="caixa" placeholder="Tela ou conta…" autofocus=true prop:value=move || q.get()
                        on:input=move |ev| q.set(event_target_value(&ev))
                        on:keydown=move |ev| {
                            if ev.key() == "Enter" {
                                if let Some((_, p, c)) = opcoes().into_iter().next() { ir(p, c); }
                            } else if ev.key() == "Escape" {
                                aberta.set(false);
                            }
                        } />
                    {move || opcoes().into_iter().take(10).map(|(rot, p, c)| view! {
                        <div class="opcao" on:click=move |_| ir(p, c.clone())>{rot}</div>
                    }).collect_view()}
                </div>
            </div>
        })}
    }
}

/// Central de alertas: os alertas do motor da última hora (de `/api/historico`).
#[component]
pub fn Alertas(e: Estado) -> impl IntoView {
    let aberta = RwSignal::new(false);
    let hist = LocalResource::new(move || {
        let url = e.url.get();
        aberta.track();
        async move { crate::dados::api(&url, "/api/historico").await.ok() }
    });
    let recentes = move || -> Vec<Value> {
        let agora = js_sys::Date::now();
        hist.get()
            .flatten()
            .map(|h| {
                lista(&h, "acoes")
                    .iter()
                    .filter(|a| {
                        txt(a, "tipo") == "alerta"
                            && agora - num(a, "t").unwrap_or(0.0) < 3_600_000.0
                    })
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    };
    view! {
        <button class="pilula vidro" title="Alertas da última hora" on:click=move |_| aberta.update(|a| *a = !*a)>
            {icone("sino")}
            {move || { let n = recentes().len(); (n > 0).then(|| view! { <span class="contagem">{n}</span> }) }}
        </button>
        {move || aberta.get().then(|| view! {
            <div class="alertas vidro">
                {move || {
                    let l = recentes();
                    if l.is_empty() {
                        view! { <div class="vazio">"Nenhum alerta na última hora."</div> }.into_any()
                    } else {
                        l.into_iter().take(15).map(|a| view! {
                            <div class="opcao"><b>{txt(&a, "nick")}</b>" "{txt(&a, "texto")}</div>
                        }).collect_view().into_any()
                    }
                }}
            </div>
        })}
    }
}
