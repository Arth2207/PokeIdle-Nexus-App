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
    view! {
        <div class="resumo">
            <h2>{titulo}</h2>
            <span class="carimbo" aria-hidden="true">{titulo}</span>
            <button class="btn" on:click=move |_| versao.update(|v| *v += 1)>"Atualizar"</button>
        </div>
        <div class="pergunta">{pergunta}</div>
    }
}
