//! Telas do app (uma pergunta por tela; ver `docs/design/telas.md`). Cada uma lê a API do
//! Nexus pelo `dados::api` e monta com os componentes de `comp`.

pub mod automacao;
pub mod bestiario;
pub mod bosses;
pub mod cacada;
pub mod combate;
pub mod escaneamento;
pub mod financas;
pub mod mercado;
pub mod pregao;
pub mod pvp;
pub mod saude;
pub mod visao;

use leptos::prelude::*;

use crate::{lista_contas, Estado};

/// Opção do seletor que troca as telas para o modo comparativo.
pub const TODAS: &str = "Todas as contas";

/// Modo "Todas as contas" (as telas de uma conta só seguem na primeira).
pub fn todas(e: Estado) -> bool {
    e.conta_ativa.get() == TODAS
}

/// Rótulo da conta escolhida no topo (minúsculo), ou a primeira da lista.
pub fn rotulo_ativo(e: Estado) -> Option<String> {
    let a = e.conta_ativa.get();
    let l = lista_contas(&e.contas_txt.get());
    let n = if l.contains(&a) {
        Some(a)
    } else {
        l.first().cloned()
    };
    n.map(|n| n.to_lowercase())
}

/// Nick no jogo (minúsculo) da conta escolhida: as rotas de equipe e PvP guardam por ele.
/// Sem o nick ainda, o rótulo.
pub fn nick_ativo(e: Estado) -> Option<String> {
    let r = rotulo_ativo(e)?;
    let nick = e
        .nicks
        .get()
        .into_iter()
        .find(|(c, n)| c.eq_ignore_ascii_case(&r) && !n.is_empty())
        .map(|(_, n)| n.to_lowercase());
    Some(nick.unwrap_or(r))
}

/// Cabeçalho padrão: título e a pergunta que a tela responde.
pub fn cabecalho(
    titulo: &'static str,
    pergunta: &'static str,
    versao: RwSignal<u32>,
) -> impl IntoView {
    // O título brota letra a letra; uma rama cresce embaixo com folhinhas.
    let letras: Vec<(usize, char)> = titulo.chars().enumerate().collect();
    let folhas: Vec<usize> = (1..=4).map(|k| k * 100 / 5).collect();
    view! {
        <crate::mutirao::Faixa nome=titulo />
        <div class="resumo">
            <h2 class="brota" aria-label=titulo>
                {letras.into_iter().map(|(i, c)| view! {
                    <span class="letra" aria-hidden="true" style=format!("--i:{i}")>{if c == ' ' { '\u{a0}' } else { c }}</span>
                }).collect_view()}
                <svg class="rama" viewBox="0 0 100 18" preserveAspectRatio="none" aria-hidden="true">
                    <path d="M0 9 C15 2 25 16 40 9 S65 2 80 9 S95 14 100 8" />
                    {folhas.into_iter().enumerate().map(|(k, x)| view! {
                        <path class="folhinha" style=format!("--i:{k}") d=format!("M{x} 9 c2 -6 6 -8 9 -8 c-1 4 -4 8 -9 8z") />
                    }).collect_view()}
                </svg>
            </h2>
            <button class="btn" on:click=move |_| versao.update(|v| *v += 1)>"Atualizar"</button>
        </div>
        <div class="pergunta">{pergunta}</div>
    }
}
