//! Controles web: usa a sessão autenticada do painel; não chama funções exclusivas do Tauri.
use crate::{
    comp::carregado,
    dados::{self, lista, num, txt},
    Estado,
};
use leptos::prelude::*;
use serde_json::{json, Value};
use wasm_bindgen_futures::spawn_local;

fn enviar(
    e: Estado,
    corpo: Value,
    ocupado: RwSignal<bool>,
    mensagem: RwSignal<String>,
    versao: RwSignal<u32>,
) {
    ocupado.set(true);
    spawn_local(async move {
        match dados::api_post(&e.url.get_untracked(), "/api/relay/controle", &corpo).await {
            Ok(_) => {
                mensagem.set("Pedido enviado. Aguarde a confirmação do cliente abaixo.".into())
            }
            Err(er) => mensagem.set(format!("Não foi possível enviar: {er}")),
        }
        ocupado.set(false);
        versao.update(|v| *v += 1);
    });
}

#[component]
pub fn ConfiguracoesWeb(e: Estado) -> impl IntoView {
    let versao = RwSignal::new(0u32);
    let estado = dados::recurso(e, versao, || Some("/api/relay/controle".into()));
    let conta = RwSignal::new(String::new());
    let slug = RwSignal::new(String::new());
    let ocupado = RwSignal::new(false);
    let mensagem = RwSignal::new(String::new());
    let timer = set_interval_with_handle(
        move || versao.update(|v| *v += 1),
        std::time::Duration::from_secs(5),
    )
    .ok();
    on_cleanup(move || {
        if let Some(t) = timer {
            t.clear();
        }
    });
    Effect::new(move |_| {
        if conta.get().is_empty() {
            if let Some(Some(Ok(v))) = estado.get() {
                if let Some(c) = lista(&v, "contas").first() {
                    conta.set(txt(c, "conta"));
                }
            }
        }
    });
    let atual = Signal::derive(move || {
        estado
            .get()
            .and_then(|r| r.and_then(Result::ok))
            .and_then(|v| {
                lista(&v, "contas")
                    .iter()
                    .find(|c| txt(c, "conta") == conta.get())
                    .cloned()
            })
            .unwrap_or(Value::Null)
    });
    view! {
        <h2>"Controle pelo site"</h2>
        <div class="limite">
            <p class="dica">"Os pedidos usam o cliente conectado ao jogo. Você não precisa salvar tokens neste site."</p>
            <label class="campo" for="web-conta">"Conta"</label>
            <select id="web-conta" class="caixa" prop:value=move || conta.get()
                on:change=move |ev| { conta.set(event_target_value(&ev)); slug.set(String::new()); mensagem.set(String::new()); }>
                {move || estado.get().and_then(|r|r.and_then(Result::ok)).map(|v|lista(&v,"contas").iter().map(|c|{
                    let id=txt(c,"conta"); let nick=txt(c,"nick");
                    view!{<option value=id>{nick}</option>}
                }).collect_view())}
            </select>
            {move || carregado(estado.get(), |_| {
                let a=atual.get();
                let recente=js_sys::Date::now()-num(&a,"em").unwrap_or(0.0)<90_000.0;
                let suporte=txt(&a,"suporte_remoto");
                let resultado=txt(&a["controle"],"resultado");
                let status=if suporte=="atualizacao_necessaria" {
                    "O cliente conectado precisa de atualização para aceitar comandos de modo pelo site.".to_owned()
                } else if a["controle"].is_object() && resultado.is_empty() {
                    if js_sys::Date::now()>num(&a["controle"],"expira_em").unwrap_or(0.0) {"Pedido expirou sem confirmação do cliente.".into()}
                    else {"Aguardando confirmação do cliente.".into()}
                } else if txt(&a,"modo")=="desconhecido" {
                    if resultado.is_empty() { "Modo ainda não confirmado pelo cliente.".into() }
                    else {format!("O cliente respondeu: {resultado}")}
                } else {format!("Modo confirmado: {}",txt(&a,"modo"))};
                view! {
                    <p class="dica">{if recente {"Recebendo dados do jogo"} else {"Sem dados recentes do jogo"}}" · "{txt(&a,"hunt")}</p>
                    <p class="dica" role="status">{status}</p>
                }
            })}
            <div class="acoes">
                <button class="btn prim" prop:disabled=move || ocupado.get() || conta.get().is_empty()
                    on:click=move |_| enviar(e,json!({"acao":"modo","conta":conta.get_untracked(),"modo":"ativo"}),ocupado,mensagem,versao)>"Ativar execução"</button>
                <button class="btn" prop:disabled=move || ocupado.get() || conta.get().is_empty()
                    on:click=move |_| enviar(e,json!({"acao":"modo","conta":conta.get_untracked(),"modo":"sombra"}),ocupado,mensagem,versao)>"Só coletar"</button>
            </div>
            <h2 class="sub">"Trocar hunt"</h2>
            <label class="campo" for="web-hunt">"Hunts liberadas"</label>
            <select id="web-hunt" class="caixa" prop:value=move || slug.get() on:change=move |ev|slug.set(event_target_value(&ev))>
                <option value="">"Escolha a hunt"</option>
                {move || lista(&atual.get(),"hunts").iter().map(|h|{
                    let nome=format!("{} · nível {}",txt(h,"nome"),txt(h,"nivel"));
                    view!{<option value=txt(h,"slug")>{nome}</option>}
                }).collect_view()}
            </select>
            <div class="acoes"><button class="btn prim"
                prop:disabled=move || ocupado.get() || slug.get().is_empty() || txt(&atual.get(),"modo")!="ativo" || txt(&atual.get(),"suporte_remoto")!="confirmado"
                on:click=move |_| enviar(e,json!({"acao":"hunt","conta":conta.get_untracked(),"slug":slug.get_untracked()}),ocupado,mensagem,versao)>"Ir para a hunt"</button></div>
            <p class="dica" role="status">{move || mensagem.get()}</p>
            <button class="btn" on:click=move |_|versao.update(|v|*v+=1)>"Atualizar estado"</button>
        </div>
        <crate::telas::saude::Saude e=e />
    }
}
