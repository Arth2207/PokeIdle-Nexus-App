//! Configurações › Saúde: o servidor responde, com qual token, e quando chegou a última janela
//! de cada conta. Métricas de infraestrutura ficam no Grafana, não no app.

use leptos::prelude::*;

use crate::comp::{carregado, Tabela};
use crate::dados::{lista, num, recurso, txt};
use crate::Estado;

/// "há 12 s", "há 3 min", "há 2 h".
pub fn idade(ms: f64) -> String {
    let s = (ms / 1000.0).max(0.0) as i64;
    match s {
        0..=59 => format!("há {s} s"),
        60..=3599 => format!("há {} min", s / 60),
        _ => format!("há {} h", s / 3600),
    }
}

#[component]
pub fn Saude(e: Estado) -> impl IntoView {
    let versao = RwSignal::new(0u32);
    let eu = recurso(e, versao, || Some("/api/eu".into()));
    let contas = recurso(e, versao, || Some("/api/contas".into()));
    view! {
        <h2 class="sub">"Saúde"</h2>
        {move || carregado(eu.get(), |v| view! {
            <div class="dica">{format!("Servidor no ar · token \"{}\" · escopos: {}", txt(&v, "nome"),
                lista(&v, "escopos").iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(", "))}</div>
        })}
        {move || carregado(contas.get(), |c| {
            let agora = js_sys::Date::now();
            let linhas: Vec<Vec<String>> = lista(&c, "contas").iter().map(|x| vec![
                txt(x, "conta"), txt(x, "nick"), txt(x, "hunt"),
                idade(agora - num(x, "em").unwrap_or(0.0)),
            ]).collect();
            view! { <Tabela cab=vec!["Conta", "Nick no jogo", "Hunt", "Última janela"] linhas=linhas /> }
        })}
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn idade_legivel() {
        assert_eq!(idade(12_000.0), "há 12 s");
        assert_eq!(idade(180_000.0), "há 3 min");
        assert_eq!(idade(7_200_000.0), "há 2 h");
    }
}
