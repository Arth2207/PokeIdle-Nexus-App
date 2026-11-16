//! Automação: o que o sistema está fazendo sozinho, e está fazendo bem?

use leptos::prelude::*;
use serde_json::Value;

use super::{cabecalho, nick_ativo, todas};
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

const ABAS: &[&str] = &["Controles", "Desempenho", "Decisões", "Fila"];

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

/// Nome e explicação de cada regra do motor (seção da `motor_config`).
fn regra_pt(secao: &str) -> (&'static str, &'static str) {
    match secao {
        "compra" => ("Compras na loja", "Repõe bolas e poções quando acabam."),
        "pocao" => (
            "Poção automática",
            "Usa poção quando o HP cai abaixo do limiar.",
        ),
        "hunt" => (
            "Troca de hunt",
            "Muda para a hunt que rende mais no objetivo escolhido.",
        ),
        "safari" => (
            "Safari · trocar após 1 captura",
            "Prioriza espécies que faltam no Top Catch semanal, sorteando hunts até o nível do Pokémon ativo. Tem prioridade sobre a troca por rendimento.",
        ),
        "rodizio" => (
            "Rodízio de pokémon",
            "Troca o ativo para upar a equipe até a meta do boss.",
        ),
        "retomar" => ("Retomar caçada", "Volta a caçar se a conta ficar parada."),
        "recarregar" => (
            "Recarregar travada",
            "Recarrega a página do jogo quando trava.",
        ),
        "zerar" => (
            "Zerar contadores",
            "Zera os contadores do dia à meia-noite.",
        ),
        "alertas" => (
            "Alertas",
            "Avisos de shiny, desmaio, parada, mercado e suprimentos.",
        ),
        "mercadoCompra" => (
            "Compras no Mercado",
            "Compra pokémon bons e baratos dentro dos tetos.",
        ),
        "venderNpc" => (
            "Vender ao NPC",
            "Vende pokémon fracos ao NPC para liberar o depósito.",
        ),
        "pedras" => ("Usar pedras", "Usa pedras de evolução guardando a reserva."),
        "radar" => ("Radar do Mercado", "Avisa itens subindo de preço."),
        "gemaPrecos" => (
            "Preços em gemas",
            "Quanto vale cada recompensa em gemas (cálculo).",
        ),
        "papel" => (
            "Papel da conta",
            "Farm, PvP ou boss: muda o que o motor prioriza.",
        ),
        _ => ("", ""),
    }
}

/// "minIvTotal" → "min iv total".
fn campo_pt(k: &str) -> String {
    let mut o = String::new();
    for ch in k.chars() {
        if ch.is_uppercase() {
            o.push(' ');
            o.extend(ch.to_lowercase());
        } else {
            o.push(ch);
        }
    }
    o
}

/// Manda a troca de um campo ao Nexus e relê as regras.
fn alterar(
    e: Estado,
    versao: RwSignal<u32>,
    msg: RwSignal<String>,
    nick: String,
    secao: String,
    campo: String,
    valor: Value,
) {
    leptos::task::spawn_local(async move {
        let corpo =
            serde_json::json!({"nick": nick, "secao": secao, "campo": campo, "valor": valor});
        match crate::dados::api_post(&e.url.get_untracked(), "/api/motor/config", &corpo).await {
            Ok(_) => msg.set(format!(
                "{nick}: {} · {} salvo.",
                regra_pt(&secao).0,
                campo_pt(&campo)
            )),
            Err(er) => msg.set(format!("Não salvou: {er}")),
        }
        versao.update(|v| *v += 1);
    });
}

fn chave(ligado: bool, ao_clicar: impl Fn() + 'static) -> impl IntoView {
    view! { <button class="chave-btn" title=if ligado { "Desligar" } else { "Ligar" } on:click=move |_| ao_clicar()><span class="chave" class:on=ligado></span></button> }
}

/// Uma regra de uma conta: o liga/desliga, as opções sim/não e os números.
fn cartao_regra(
    e: Estado,
    versao: RwSignal<u32>,
    msg: RwSignal<String>,
    r: Value,
) -> impl IntoView {
    let nick = txt(&r, "nick");
    let secao = txt(&r, "secao");
    let (nome, expl) = regra_pt(&secao);
    let d = r
        .get("dados")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let ativo = d.get("ativo").and_then(Value::as_bool);
    let (n2, s2) = (nick.clone(), secao.clone());
    let opcoes: Vec<(String, Value)> = d
        .iter()
        .filter(|(k, v)| {
            k.as_str() != "ativo"
                && k.as_str() != "objetivoManual"
                && (v.is_boolean() || v.is_number())
        })
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let (ng, sg) = (nick.clone(), secao.clone());
    let objetivo = txt(&r["dados"], "objetivo");
    view! {
        <div class="regra" class:desligada=ativo == Some(false)>
            <div class="regra-topo">
                <div><div class="regra-n">{if nome.is_empty() { secao.clone() } else { nome.to_owned() }}</div><div class="dica">{expl}</div></div>
                {ativo.map(|a| chave(a, move || alterar(e, versao, msg, n2.clone(), s2.clone(), "ativo".into(), Value::Bool(!a))))}
            </div>
            {(secao == "hunt").then(|| view! {
                <label class="op"><span>"Objetivo da conta"</span>
                    <select prop:value=objetivo on:change=move |ev| alterar(e,versao,msg,ng.clone(),sg.clone(),"objetivo".into(),Value::String(event_target_value(&ev)))>
                        <option value="xp">"XP"</option><option value="equilibrio">"XP sem prejuízo"</option>
                        <option value="lucro">"Lucro em Coins"</option><option value="gemas">"Gemas (estimativa parcial de tokens nas Outlands)"</option><option value="shiny">"Shiny"</option>
                    </select>
                </label>
                <div class="dica">"Sua escolha de objetivo tem prioridade sobre o papel da conta; não liga vendas ao NPC."</div>
            })}
            <div class="regra-op">
                {opcoes.into_iter().map(|(k, v)| {
                    let (n3, s3, k3) = (nick.clone(), secao.clone(), k.clone());
                    let rotulo = campo_pt(&k);
                    match v {
                        Value::Bool(b) => view! {
                            <label class="op">{chave(b, move || alterar(e, versao, msg, n3.clone(), s3.clone(), k3.clone(), Value::Bool(!b)))}<span>{rotulo}</span></label>
                        }.into_any(),
                        n => view! {
                            <label class="op"><span>{rotulo}</span>
                                <input class="num" type="number" step="any" value=n.to_string()
                                    on:change=move |ev| {
                                        if let Ok(x) = event_target_value(&ev).parse::<f64>() {
                                            alterar(e, versao, msg, n3.clone(), s3.clone(), k3.clone(), serde_json::json!(x));
                                        }
                                    } />
                            </label>
                        }.into_any(),
                    }
                }).collect_view()}
            </div>
        </div>
    }
}

/// Controles: as regras do motor da conta escolhida (ou a grade de todas as contas).
fn controles(e: Estado, versao: RwSignal<u32>) -> impl IntoView {
    let regras = recurso(e, versao, || Some("/api/motor/regras".into()));
    let msg = RwSignal::new(String::new());
    view! {
        {move || (!msg.get().is_empty()).then(|| view! { <div class="aviso-ok">{msg.get()}</div> })}
        {move || carregado(regras.get(), move |v| {
            let todas_r = lista(&v, "regras").to_vec();
            if todas(e) {
                let mut contas: Vec<String> = todas_r.iter().map(|r| txt(r, "nick")).collect();
                contas.dedup();
                let mut secoes: Vec<String> = Vec::new();
                for r in &todas_r {
                    let s = txt(r, "secao");
                    if r.pointer("/dados/ativo").is_some_and(Value::is_boolean) && !secoes.contains(&s) {
                        secoes.push(s);
                    }
                }
                return view! {
                    <div class="tabela-caixa"><table class="tabela grade-regras">
                        <thead><tr><th>"Regra"</th>{contas.iter().map(|c| view! { <th>{c.clone()}</th> }).collect_view()}</tr></thead>
                        <tbody>{secoes.into_iter().map(|s| {
                            let linha: Vec<_> = contas.iter().map(|c| {
                                let a = todas_r.iter().find(|r| txt(r, "nick") == *c && txt(r, "secao") == s).and_then(|r| r.pointer("/dados/ativo")).and_then(Value::as_bool);
                                let (c2, s2) = (c.clone(), s.clone());
                                view! { <td>{a.map(|a| chave(a, move || alterar(e, versao, msg, c2.clone(), s2.clone(), "ativo".into(), Value::Bool(!a))))}</td> }
                            }).collect();
                            view! { <tr><td style="text-align:left">{regra_pt(&s).0}</td>{linha}</tr> }
                        }).collect_view()}</tbody>
                    </table></div>
                }.into_any();
            }
            let nick = nick_ativo(e).unwrap_or_default();
            let dela: Vec<Value> = todas_r.into_iter().filter(|r| txt(r, "nick").eq_ignore_ascii_case(&nick)).collect();
            if dela.is_empty() {
                return view! { <div class="vazio">"Esta conta ainda não tem regras no motor."</div> }.into_any();
            }
            view! { <div class="regras">{dela.into_iter().map(|r| cartao_regra(e, versao, msg, r)).collect_view()}</div> }.into_any()
        })}
    }
}

#[component]
pub fn Automacao(e: Estado) -> impl IntoView {
    let versao = RwSignal::new(0u32);
    let aba = RwSignal::new("Controles");
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
            "Controles" => controles(e, versao).into_any(),
            "Desempenho" => carregado(motor.get(), |m| {
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
