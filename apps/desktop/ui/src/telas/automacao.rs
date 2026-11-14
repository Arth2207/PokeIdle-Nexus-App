//! Automação: o que o sistema está fazendo sozinho, e está fazendo bem?

use leptos::prelude::*;
use serde_json::Value;

use super::cabecalho;
use crate::comp::{carregado, Abas, Kpi, Tabela};
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

const ABAS: &[&str] = &["Regras", "Decisões", "Fila"];

/// Texto curto de uma decisão do motor (`dados.decisao`): o comando, o alerta ou o registro.
fn resumo(d: &Value) -> String {
    let x = &d["decisao"];
    let t = txt(x, "texto");
    if !t.is_empty() {
        return t;
    }
    let titulo = txt(x, "titulo");
    if !titulo.is_empty() {
        return titulo;
    }
    x.get("comando")
        .map(|c| c.to_string().chars().take(120).collect())
        .unwrap_or_default()
}

#[component]
pub fn Automacao(e: Estado) -> impl IntoView {
    let versao = RwSignal::new(0u32);
    let aba = RwSignal::new("Regras");
    let fila = recurso(e, versao, || Some("/api/comandos/fila".into()));
    let motor = recurso(e, versao, || Some("/api/motor/decisoes".into()));
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
        <Abas opcoes=ABAS ativa=aba />
        {move || match aba.get() {
            "Regras" => carregado(motor.get(), |m| {
                let linhas: Vec<Vec<String>> = lista(&m, "regras").iter().map(|r| {
                    let (ok, falhas, total) = (num(r, "ok").unwrap_or(0.0), num(r, "falhas").unwrap_or(0.0), num(r, "total").unwrap_or(0.0));
                    vec![
                        txt(r, "tipo").trim_start_matches("motor.").to_owned(),
                        inteiro(total), inteiro(ok), inteiro(falhas),
                        if ok + falhas > 0.0 { format!("{:.0}%", ok / (ok + falhas) * 100.0) } else { "—".into() },
                        hora(num(r, "ultimo")),
                    ]
                }).collect();
                view! {
                    <div class="dica">"Comandos que o motor mandou ao jogo nas últimas 24 h, por tipo, e quantos o jogo aceitou."</div>
                    <Tabela cab=vec!["Comando", "Mandados", "Aceitos", "Falharam", "Acerto", "Último"] linhas=linhas />
                }.into_any()
            }),
            "Decisões" => carregado(motor.get(), |m| {
                let linhas: Vec<Vec<String>> = lista(&m, "decisoes").iter().map(|d| vec![
                    hora(num(d, "ultimo")), txt(d, "nick"), txt(d, "regra").trim_start_matches("motor").to_owned(),
                    txt(d, "decisao"), inteiro(num(d, "ciclos").unwrap_or(0.0)), resumo(&d["dados"]),
                ]).collect();
                view! {
                    <div class="dica">"O que o motor decidiu nas últimas 6 h, uma linha por conta e regra (ciclos: quantas vezes decidiu igual, a cada 30 s)."</div>
                    <Tabela cab=vec!["Quando", "Conta", "Regra", "Tipo", "Ciclos", "O quê"] linhas=linhas />
                }.into_any()
            }),
            _ => carregado(fila.get(), |v| {
                let c = lista(&v, "comandos");
                let linhas: Vec<Vec<String>> = c.iter().map(|x| vec![
                    txt(x, "tipo"), txt(x, "conta"), txt(x, "origem"), txt(x, "estado"),
                    hora(num(x, "criado")), hora(num(x, "executado")),
                    x.get("resultado").and_then(Value::as_str).unwrap_or("").chars().take(60).collect(),
                ]).collect();
                view! {
                    <Tabela cab=vec!["Comando", "Conta", "Origem", "Estado", "Criado", "Executado", "Resultado"] linhas=linhas />
                }.into_any()
            }),
        }}
    }
}
