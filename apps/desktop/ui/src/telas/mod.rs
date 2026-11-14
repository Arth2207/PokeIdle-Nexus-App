//! Telas do app (uma pergunta por tela; ver `docs/design/telas.md`). Cada uma lê a API do
//! Nexus pelo `dados::api` e monta com os componentes de `comp`.

pub mod automacao;
pub mod bestiario;
pub mod cacada;
pub mod combate;
pub mod escaneamento;
pub mod financas;
pub mod mercado;
pub mod visao;

use leptos::prelude::*;

use crate::{lista_contas, Estado};

/// Nick da conta escolhida no topo (minúsculo), ou a primeira da lista.
pub fn nick_ativo(e: Estado) -> Option<String> {
    let a = e.conta_ativa.get();
    let l = lista_contas(&e.contas_txt.get());
    let n = if l.contains(&a) {
        Some(a)
    } else {
        l.first().cloned()
    };
    n.map(|n| n.to_lowercase())
}

/// Cabeçalho padrão: título e a pergunta que a tela responde.
pub fn cabecalho(
    titulo: &'static str,
    pergunta: &'static str,
    versao: RwSignal<u32>,
) -> impl IntoView {
    view! {
        <div class="resumo">
            <h2>{titulo}</h2>
            <button class="btn" on:click=move |_| versao.update(|v| *v += 1)>"Atualizar"</button>
        </div>
        <div class="pergunta">{pergunta}</div>
    }
}
