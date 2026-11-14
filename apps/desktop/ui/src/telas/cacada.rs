//! Caçada: onde caçar, e a caçada atual está rendendo o esperado?

use leptos::prelude::*;
use serde_json::Value;

use super::{cabecalho, nick_ativo};
use crate::comp::{carregado, Abas, Barras, Kpi, Tabela};
use crate::dados::{curto, inteiro, lista, num, recurso, txt};
use crate::Estado;

const ABAS: &[&str] = &["Sessão", "Mapa de hunts", "Shiny"];

/// A conta do `/api/financeiro` com o nick (sem diferenciar maiúsculas; o legado tem nicks
/// com letras repetidas, então compara sem as repetições também).
fn conta_fin<'a>(f: &'a Value, nick: &str) -> Option<&'a Value> {
    let simples = |s: &str| {
        let mut o = String::new();
        for c in s.to_lowercase().chars() {
            if !o.ends_with(c) {
                o.push(c);
            }
        }
        o
    };
    let alvo = simples(nick);
    lista(f, "contas")
        .iter()
        .find(|c| simples(&txt(c, "nick")) == alvo)
}

#[component]
pub fn Cacada(e: Estado) -> impl IntoView {
    let versao = RwSignal::new(0u32);
    let aba = RwSignal::new("Sessão");
    let fin = recurso(e, versao, || Some("/api/financeiro".into()));
    let hist = recurso(e, versao, || Some("/api/historico".into()));
    let shiny = recurso(e, versao, || Some("/api/shiny/servidor?dias=7".into()));
    view! {
        {cabecalho("Caçada", "Onde caçar, e a caçada atual está rendendo o esperado?", versao)}
        <Abas opcoes=ABAS ativa=aba />
        {move || match aba.get() {
            "Sessão" => carregado(fin.get(), move |f| {
                let nick = nick_ativo(e).unwrap_or_default();
                let Some(c) = conta_fin(&f, &nick).cloned() else {
                    return view! { <div class="vazio">"Sem histórico desta conta ainda."</div> }.into_any();
                };
                let dias = lista(&c, "semana").to_vec();
                let hoje = dias.last().cloned().unwrap_or(Value::Null);
                let anteriores: Vec<&Value> = dias.iter().rev().skip(1).take(7).collect();
                let media = |k: &str| {
                    let n = anteriores.len().max(1) as f64;
                    anteriores.iter().map(|d| num(d, k).unwrap_or(0.0)).sum::<f64>() / n
                };
                let comp = |k: &str| {
                    let m = media(k);
                    let h = num(&hoje, k).unwrap_or(0.0);
                    (format!("média dos 7 anteriores {}", curto(m)), (!anteriores.is_empty()).then_some(h >= m))
                };
                let dia = txt(&hoje, "dia");
                let (ck, bk) = comp("kills");
                let (cl, bl) = comp("loot");
                let (cx, bx) = comp("xp");
                let linhas: Vec<Vec<String>> = dias.iter().rev().map(|d| vec![
                    txt(d, "dia"),
                    inteiro(num(d, "kills").unwrap_or(0.0)),
                    curto(num(d, "loot").unwrap_or(0.0)),
                    curto(num(d, "xp").unwrap_or(0.0)),
                    inteiro(num(d, "bolas").unwrap_or(0.0)),
                    inteiro(num(d, "capt").unwrap_or(0.0)),
                    inteiro(num(d, "shiny").unwrap_or(0.0)),
                    inteiro(num(d, "pot").unwrap_or(0.0)),
                ]).collect();
                view! {
                    <div class="dica">{format!("Último dia com dado: {dia}.")}</div>
                    <div class="kpis">
                        <Kpi titulo="Abates no dia" valor=inteiro(num(&hoje, "kills").unwrap_or(0.0)) comparacao=ck bom=bk
                            ajuda="Abates do dia (UTC−3) contra a média dos 7 dias anteriores." />
                        <Kpi titulo="Loot no dia" valor=curto(num(&hoje, "loot").unwrap_or(0.0)) unidade="gold" comparacao=cl bom=bl
                            ajuda="Ouro do loot da caçada no dia." />
                        <Kpi titulo="XP no dia" valor=curto(num(&hoje, "xp").unwrap_or(0.0)) comparacao=cx bom=bx
                            ajuda="XP do treinador no dia." />
                        <Kpi titulo="Capturas no dia" valor=inteiro(num(&hoje, "capt").unwrap_or(0.0))
                            comparacao=format!("{} bolas", inteiro(num(&hoje, "bolas").unwrap_or(0.0))) />
                    </div>
                    <div class="secao">"Dia a dia"</div>
                    <Tabela cab=vec!["Dia", "Abates", "Loot", "XP", "Bolas", "Capturas", "Shiny", "Poções"] linhas=linhas />
                }.into_any()
            }),
            "Mapa de hunts" => carregado(hist.get(), move |h| {
                let nick = nick_ativo(e).unwrap_or_default();
                let mut linhas: Vec<(String, f64, String)> = Vec::new();
                for (conta, m) in h.get("medidas").and_then(Value::as_object).into_iter().flatten() {
                    let prefixo: String = nick.chars().take(5).collect();
                    let minha = conta.to_lowercase().starts_with(&prefixo);
                    for (k, x) in m.as_object().into_iter().flatten() {
                        if let Some(xph) = num(x, "xpH") {
                            let hunt = k.split('|').next().unwrap_or(k).to_owned();
                            let nome = if minha { hunt } else { format!("{hunt} ({conta})") };
                            linhas.push((nome, xph, format!("{}/h · {} kills/h", curto(xph), inteiro(num(x, "kph").unwrap_or(0.0)))));
                        }
                    }
                }
                linhas.sort_by(|a, b| b.1.total_cmp(&a.1));
                linhas.truncate(20);
                let melhor = linhas.first().map(|l| l.0.clone());
                view! {
                    <div class="secao">"XP por hora medido em cada hunt"</div>
                    <Barras linhas=linhas destaque=melhor.unwrap_or_default() />
                    <div class="dica">"Medido pelo motor nas sessões reais de cada conta (kills/h × XP por abate)."</div>
                }.into_any()
            }),
            _ => carregado(shiny.get(), move |s| {
                let chance = hist.get().and_then(|h| h.and_then(Result::ok)).and_then(|h| num(&h, "chanceShiny"));
                let linhas: Vec<(String, f64, String)> = lista(&s, "especies").iter().take(15).map(|x| {
                    let v = num(x, "vezes").unwrap_or(0.0);
                    (txt(x, "nome"), v, format!("{} · nv {}", inteiro(v), inteiro(num(x, "nivel_medio").unwrap_or(0.0))))
                }).collect();
                let horas: Vec<f64> = s.get("horas").and_then(Value::as_array).map(|a| a.iter().map(|h| h.as_f64().unwrap_or(0.0)).collect()).unwrap_or_default();
                let max = horas.iter().copied().fold(1.0_f64, f64::max);
                view! {
                    <div class="kpis">
                        <Kpi titulo="Shiny no servidor" valor=inteiro(num(&s, "total").unwrap_or(0.0)) unidade="7 dias"
                            ajuda="Avisos globais de shiny capturado, cada pokémon contado uma vez." />
                        <Kpi titulo="Ritmo do servidor" valor=format!("{:.1}", num(&s, "por_hora").unwrap_or(0.0)) unidade="shiny/h" />
                        <Kpi titulo="Sua chance" valor=chance.map_or("—".into(), |c| format!("1 em {}", inteiro(1.0 / c.max(1e-12))))
                            ajuda="Chance de shiny por encontro com os bônus atuais da conta." />
                    </div>
                    <div class="secao">"Espécies mais capturadas shiny"</div>
                    <Barras linhas=linhas />
                    <div class="secao">"Por hora do dia (UTC−3)"</div>
                    <div class="horas">{horas.iter().map(|h| view! { <div style=format!("opacity:{:.2}", 0.12 + 0.88 * h / max) title=format!("{h}")></div> }).collect_view()}</div>
                    <div class="horas-rot">{(0..24).map(|h| view! { <span>{h}</span> }).collect_view()}</div>
                }.into_any()
            }),
        }}
    }
}
