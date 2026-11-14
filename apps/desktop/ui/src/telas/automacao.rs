//! Automação: o que o sistema está fazendo sozinho, e está fazendo bem?

use leptos::prelude::*;
use serde_json::Value;

use super::cabecalho;
use crate::comp::{carregado, Barras, Kpi, Tabela};
use crate::dados::{inteiro, lista, num, recurso, txt};
use crate::Estado;

fn hora(ms: Option<f64>) -> String {
    ms.map_or("—".into(), |m| {
        String::from(
            js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(m))
                .to_locale_string("pt-BR", &wasm_bindgen::JsValue::UNDEFINED),
        )
    })
}

#[component]
pub fn Automacao(e: Estado) -> impl IntoView {
    let versao = RwSignal::new(0u32);
    let fila = recurso(e, versao, || Some("/api/comandos/fila".into()));
    view! {
        {cabecalho("Automação", "O que o sistema está fazendo sozinho, e está fazendo bem?", versao)}
        <div class="kpis">
            {move || {
                let modo = e.modo.get();
                let aprender = e.aprender.get() == "sim";
                view! {
                    <Kpi titulo="Modo do app" valor=if modo == "ativo" { "Ativo".into() } else { "Sombra".into() }
                        ajuda="Sombra só coleta; Ativo também executa os comandos do Nexus."
                        comparacao=if aprender && modo == "ativo" { "aprendendo PvP no Treinamento".into() } else { "PvP: sem aprendizado".into() } />
                }
            }}
            {move || carregado(fila.get(), |v| {
                let c = lista(&v, "comandos");
                let conta = |s: &str| c.iter().filter(|x| txt(x, "estado") == s).count() as f64;
                let feitos = conta("feito");
                let falhos = conta("falhou");
                let acerto = if feitos + falhos > 0.0 { feitos / (feitos + falhos) } else { 1.0 };
                let acerto_ok = acerto >= 0.95;
                view! {
                    <Kpi titulo="Pendentes" valor=inteiro(conta("pendente") + conta("executando") + conta("agendado")) unidade="comandos" />
                    <Kpi titulo="Execução sem erro" valor=format!("{:.0}%", acerto * 100.0) bom=Some(acerto_ok)
                        comparacao="meta 95%".to_owned() ajuda="Comandos feitos ÷ (feitos + com erro), nos 100 mais recentes." />
                }
            })}
        </div>
        {move || carregado(fila.get(), |v| {
            let c = lista(&v, "comandos");
            let mut por_tipo: std::collections::BTreeMap<String, f64> = Default::default();
            for x in c {
                *por_tipo.entry(txt(x, "tipo")).or_default() += 1.0;
            }
            let mut barras: Vec<(String, f64, String)> = por_tipo.into_iter().map(|(t, n)| (t, n, inteiro(n))).collect();
            barras.sort_by(|a, b| b.1.total_cmp(&a.1));
            let linhas: Vec<Vec<String>> = c.iter().map(|x| vec![
                txt(x, "tipo"), txt(x, "conta"), txt(x, "origem"), txt(x, "estado"),
                hora(num(x, "criado")), hora(num(x, "executado")),
                x.get("resultado").and_then(Value::as_str).unwrap_or("").chars().take(60).collect(),
            ]).collect();
            view! {
                <div class="secao">"Comandos por tipo"</div>
                <Barras linhas=barras />
                <div class="secao">"Fila"</div>
                <Tabela cab=vec!["Comando", "Conta", "Origem", "Estado", "Criado", "Executado", "Resultado"] linhas=linhas />
            }
        })}
    }
}
