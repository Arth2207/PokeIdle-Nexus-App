//! Caçada: onde caçar, e a caçada atual está rendendo o esperado? (mockups 03, 04 e 05)
//! Sessão e mapa de hunts vêm de `/api/cacada` (Postgres: `fato_kill`, `fato_hunt_eficiencia`).

use leptos::prelude::*;
use serde_json::Value;

use super::{cabecalho, nick_ativo, rotulo_ativo};
use crate::comp::{carregado, Abas, Barras, Calor, Kpi, Linha, Tabela, Veredito};
use crate::dados::{curto, inteiro, lista, num, recurso, txt};
use crate::Estado;

const ABAS: &[&str] = &["Sessão ao vivo", "Mapa de hunts", "Shiny"];
const METRICAS: &[&str] = &["Lucro/h", "Ouro/h", "XP/h", "Abates/h"];

/// "furious_scizor" → "Furious Scizor".
fn nome_hunt(slug: &str) -> String {
    slug.split('_')
        .map(|p| {
            let mut c = p.chars();
            c.next()
                .map(|a| a.to_uppercase().chain(c).collect::<String>())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Hora local (UTC−3) "HH:00" de um instante em ms.
fn hh(t: f64) -> String {
    let h = (((t / 1000.0) as i64 - 3 * 3600).rem_euclid(86_400)) / 3600;
    format!("{h:02}h")
}

/// Hora local (UTC−3) "HH:MM" de um instante em ms.
fn hora_min(t: f64) -> String {
    let s = ((t / 1000.0) as i64 - 3 * 3600).rem_euclid(86_400);
    format!("{:02}:{:02}", s / 3600, (s % 3600) / 60)
}

/// Comparação "x% vs. média" de um valor contra a média (e se melhorou).
fn vs(v: f64, m: Option<f64>) -> (String, Option<bool>) {
    match m.filter(|m| *m > 0.0) {
        Some(m) => (
            format!("{:+.0}% vs. média de 7 dias", (v / m - 1.0) * 100.0),
            Some(v >= m),
        ),
        None => ("sem média ainda".to_owned(), None),
    }
}

fn sessao(c: &Value, hist: Option<Value>, nick: &str) -> AnyView {
    let s = &c["sessao"];
    let horas = lista(s, "horas");
    // A hora corrente está pela metade: a última hora cheia é a penúltima.
    let ultima = horas
        .len()
        .checked_sub(2)
        .and_then(|i| horas.get(i))
        .or(horas.last());
    let Some(u) = ultima.cloned() else {
        return view! { <div class="vazio">"Sem abates desta conta nas últimas 24 h."</div> }
            .into_any();
    };
    let m = &s["media"];
    let (ck, bk) = vs(num(&u, "kills").unwrap_or(0.0), num(m, "kills"));
    let (co, bo) = vs(num(&u, "ouro").unwrap_or(0.0), num(m, "ouro"));
    let (cx, bx) = vs(num(&u, "xp").unwrap_or(0.0), num(m, "xp"));
    let pontos: Vec<(String, f64)> = horas
        .iter()
        .map(|h| {
            (
                hh(num(h, "hora").unwrap_or(0.0)),
                num(h, "ouro").unwrap_or(0.0),
            )
        })
        .collect();
    let dr = &c["drops"];
    let kills_sessao = num(dr, "kills").unwrap_or(0.0);
    let desde = num(dr, "desde").unwrap_or(0.0);
    let horas_sessao = ((js_sys::Date::now() - desde) / 3.6e6).max(0.05);
    let drops: Vec<Vec<String>> = lista(dr, "itens")
        .iter()
        .map(|d| {
            let q = num(d, "qtd").unwrap_or(0.0);
            vec![
                txt(d, "item"),
                inteiro(q),
                if q > 0.0 {
                    format!("1 a cada {}", inteiro(kills_sessao / q))
                } else {
                    "—".into()
                },
                format!("{:.1}/h", q / horas_sessao),
                txt(d, "de"),
            ]
        })
        .collect();
    let titulo_drops = format!(
        "Drops da sessão · desde {} · {} abates",
        hora_min(desde),
        inteiro(kills_sessao)
    );
    let simples = |n: &str| {
        let mut o = String::new();
        for ch in n.to_lowercase().chars() {
            if !o.ends_with(ch) {
                o.push(ch);
            }
        }
        o
    };
    let eventos: Vec<Vec<String>> = hist
        .map(|h| {
            lista(&h, "acoes")
                .iter()
                .filter(|a| simples(&txt(a, "nick")) == simples(nick))
                .take(8)
                .map(|a| vec![hh(num(a, "t").unwrap_or(0.0)), txt(a, "texto")])
                .collect()
        })
        .unwrap_or_default();
    view! {
        <div class="dica">{format!("Hunt atual: {} · última hora cheia: {}", nome_hunt(&txt(s, "atual")), hh(num(&u, "hora").unwrap_or(0.0)))}</div>
        <div class="kpis">
            <Kpi titulo="Abates/h" valor=inteiro(num(&u, "kills").unwrap_or(0.0)) comparacao=ck bom=bk ajuda="Abates na última hora cheia." />
            <Kpi titulo="Ouro/h" valor=curto(num(&u, "ouro").unwrap_or(0.0)) unidade="gold" comparacao=co bom=bo ajuda="Ouro dos abates na última hora cheia." />
            <Kpi titulo="XP/h" valor=curto(num(&u, "xp").unwrap_or(0.0)) comparacao=cx bom=bx ajuda="XP do treinador na última hora cheia." />
        </div>
        <div class="caixa">
            <div class="caixa-t">"Rendimento da sessão (ouro/h, 24 h)"</div>
            <Linha pontos=pontos media=num(m, "ouro").unwrap_or(0.0) />
            <div class="dica">"Faixa cinza: a média por hora dos 7 dias anteriores, ±20%."</div>
        </div>
        <div class="vg-base" style="margin-top:16px">
            <div class="caixa">
                <div class="caixa-t">{titulo_drops}</div>
                <Tabela cab=vec!["Item", "Qtd", "Taxa", "Por hora", "De"] linhas=drops />
            </div>
            <div class="caixa">
                <div class="caixa-t">"Linha do tempo"</div>
                <Tabela cab=vec!["Hora", "O que houve"] linhas=eventos />
            </div>
        </div>
    }
    .into_any()
}

/// Valor em gold de cada item pelo Mercado (valor justo na moeda gold; só itens que giram em gold).
fn precos_gold(tickers: &[Value]) -> std::collections::HashMap<String, f64> {
    tickers
        .iter()
        .filter(|t| txt(t, "moeda") == "gold")
        .filter_map(|t| Some((txt(t, "nome"), num(t, "base").or_else(|| num(t, "ultimo"))?)))
        .collect()
}

/// Há quanto tempo (texto curto) desde `t` ms.
fn ha(t: f64) -> String {
    let h = (js_sys::Date::now() - t) / 3.6e6;
    if h < 1.0 {
        "há menos de 1 h".into()
    } else if h < 48.0 {
        format!("há {h:.0} h")
    } else {
        format!("há {:.0} dias", h / 24.0)
    }
}

/// Uma hunt medida: a linha da API, os drops (item, por abate) e o valor dos drops por hora.
type HuntMedida = (Value, Vec<(String, f64)>, f64);

fn mapa(c: &Value, metrica: &'static str, tickers: &[Value]) -> AnyView {
    let h = &c["hunts"];
    let atual = txt(&c["sessao"], "atual");
    let precos = precos_gold(tickers);
    let sprite_de = |nome: &str| {
        tickers
            .iter()
            .find(|t| txt(t, "nome").eq_ignore_ascii_case(nome) && !txt(t, "sprite").is_empty())
            .map(|t| txt(t, "sprite"))
    };
    // Por hunt: drops a cada 1000 abates e o valor deles por hora.
    let drops_de = |slug: &str| -> (Vec<(String, f64)>, f64) {
        let nome = nome_hunt(slug);
        let Some(d) = lista(h, "drops")
            .iter()
            .find(|d| txt(d, "alvo").eq_ignore_ascii_case(&nome))
        else {
            return (Vec::new(), 0.0);
        };
        let kills = num(d, "kills").unwrap_or(0.0).max(1.0);
        let mut v: Vec<(String, f64)> = lista(d, "itens")
            .iter()
            .map(|i| (txt(i, "item"), num(i, "qtd").unwrap_or(0.0) / kills))
            .collect();
        v.sort_by(|a, b| b.1.total_cmp(&a.1));
        let valor_por_abate: f64 = v
            .iter()
            .map(|(i, taxa)| taxa * precos.get(i).copied().unwrap_or(0.0))
            .sum();
        (v, valor_por_abate)
    };
    let mut l: Vec<HuntMedida> = lista(h, "lista")
        .iter()
        .map(|x| {
            let (d, vpa) = drops_de(&txt(x, "hunt"));
            let valor_h = vpa * num(x, "kills").unwrap_or(0.0);
            (x.clone(), d, valor_h)
        })
        .collect();
    let chave = |x: &HuntMedida| match metrica {
        "XP/h" => num(&x.0, "xp").unwrap_or(0.0),
        "Abates/h" => num(&x.0, "kills").unwrap_or(0.0),
        "Lucro/h" => num(&x.0, "ouro").unwrap_or(0.0) + x.2,
        _ => num(&x.0, "ouro").unwrap_or(0.0),
    };
    l.sort_by(|a, b| chave(b).total_cmp(&chave(a)));
    let melhor = l.first().cloned();
    let da_atual = l.iter().find(|x| txt(&x.0, "hunt") == atual).map(chave);
    let ganho = da_atual.zip(melhor.as_ref().map(chave)).map(|(a, m)| m - a);
    let nome_melhor = melhor
        .as_ref()
        .map(|m| nome_hunt(&txt(&m.0, "hunt")))
        .unwrap_or_default();
    let trocar =
        ganho.filter(|g| *g > 0.0 && melhor.as_ref().is_some_and(|m| txt(&m.0, "hunt") != atual));
    let mut grade: std::collections::BTreeMap<String, Vec<Option<f64>>> = Default::default();
    for x in lista(h, "calor") {
        let linha = grade
            .entry(nome_hunt(&txt(x, "hunt")))
            .or_insert_with(|| vec![None; 24]);
        if let Some(hr) = num(x, "hora").map(|h| h as usize).filter(|h| *h < 24) {
            linha[hr] = num(x, "ouro");
        }
    }
    let max_ouro = l
        .iter()
        .filter_map(|x| num(&x.0, "p90"))
        .fold(1.0_f64, f64::max);
    view! {
        <div class="kpis">
            <Kpi titulo="Melhor hunt (7 dias)" valor=nome_melhor.clone() comparacao=melhor.as_ref().map_or(String::new(), |m| format!("{} {metrica}", curto(chave(m)))) />
            <Kpi titulo="Sua hunt atual" valor=nome_hunt(&atual) comparacao=da_atual.map_or("sem medida nos 7 dias".into(), |v| format!("{} {metrica}", curto(v))) />
            <Kpi titulo="Ganho potencial" valor=ganho.map_or("—".into(), |g| format!("+{}", curto(g.max(0.0)))) unidade=metrica bom=ganho.map(|g| g <= 0.0) />
        </div>
        {trocar.map(|g| view! {
            <Veredito titulo=format!("Trocar para {nome_melhor}")
                motivo=format!("+{} {metrica} medidos nos últimos 7 dias (todas as contas, sessões de 30 min ou mais).", curto(g))
                acao="O motor troca de hunt sozinho quando a regra “Troca de hunt” está ligada (Automação).".to_owned() tom="bom" />
        })}
        <div class="hunts">
            {l.into_iter().map(|(x, drops, valor_h)| {
                let slug = txt(&x, "hunt");
                let nome = nome_hunt(&slug);
                let ouro = num(&x, "ouro").unwrap_or(0.0);
                let (p10, p90) = (num(&x, "p10").unwrap_or(ouro), num(&x, "p90").unwrap_or(ouro));
                let faixa = format!("left:{:.1}%;width:{:.1}%", p10 / max_ouro * 100.0, ((p90 - p10) / max_ouro * 100.0).max(1.5));
                let marca = format!("left:{:.1}%", ouro / max_ouro * 100.0);
                let e_atual = slug == atual;
                let e_melhor = nome == nome_melhor;
                let img = sprite_de(&nome);
                view! {
                    <div class="hunt-card" class:atual=e_atual class:melhor=e_melhor>
                        <div class="hunt-topo">
                            {img.map(|s| view! { <img src=s alt="" loading="lazy" /> })}
                            <div>
                                <div class="hunt-n">{nome.clone()}</div>
                                <div class="dica">{format!("{} h medidas · {}", inteiro(num(&x, "horas").unwrap_or(0.0)), ha(num(&x, "ultima").unwrap_or(0.0)))}</div>
                            </div>
                            {e_atual.then(|| view! { <span class="selo ok">"atual"</span> })}
                            {(e_melhor && !e_atual).then(|| view! { <span class="selo">"melhor"</span> })}
                        </div>
                        <div class="hunt-nums">
                            <div><b>{curto(ouro)}</b><span>"ouro/h"</span></div>
                            <div><b>{curto(valor_h)}</b><span>"drops/h (gold)"</span></div>
                            <div><b>{curto(ouro + valor_h)}</b><span>"lucro/h"</span></div>
                            <div><b>{curto(num(&x, "xp").unwrap_or(0.0))}</b><span>"xp/h"</span></div>
                            <div><b>{inteiro(num(&x, "kills").unwrap_or(0.0))}</b><span>"abates/h"</span></div>
                        </div>
                        <div class="faixa-ouro" title=format!("ouro/h: pior 10% {} · melhor 10% {}", curto(p10), curto(p90))>
                            <span class="faixa" style=faixa></span><span class="marca" style=marca></span>
                        </div>
                        <div class="hunt-drops">
                            {drops.into_iter().take(4).map(|(i, taxa)| view! {
                                <span class="chip" title=format!("{:.2} por 1000 abates", taxa * 1000.0)>{format!("{i} · 1/{}", inteiro(1.0 / taxa.max(1e-9)))}</span>
                            }).collect_view()}
                        </div>
                    </div>
                }
            }).collect_view()}
        </div>
        <div class="caixa" style="margin-top:16px">
            <div class="caixa-t">"Mapa de calor · hunt × hora (ouro/h)"</div>
            <Calor linhas=grade.into_iter().collect() />
        </div>
        <div class="dica">"Drops/h: itens que caem a cada abate (eventos de 7 dias) × abates/h × valor justo em gold no Mercado. Faixa: o pior e o melhor 10% das horas medidas."</div>
    }
    .into_any()
}

#[component]
pub fn Cacada(e: Estado) -> impl IntoView {
    let versao = RwSignal::new(0u32);
    let aba = RwSignal::new("Sessão ao vivo");
    let metrica = RwSignal::new("Lucro/h");
    let cacada = recurso(e, versao, move || {
        Some(format!(
            "/api/cacada?conta={}",
            rotulo_ativo(e).unwrap_or_default()
        ))
    });
    let hist = recurso(e, versao, || Some("/api/historico".into()));
    let shiny = recurso(e, versao, || Some("/api/shiny/servidor?dias=7".into()));
    let mercado = recurso(e, versao, || Some("/api/bolsa/mercado".into()));
    view! {
        {cabecalho("Caçada", "Onde caçar, e a caçada atual está rendendo o esperado?", versao)}
        <Abas opcoes=ABAS ativa=aba />
        {move || match aba.get() {
            "Sessão ao vivo" => carregado(cacada.get(), move |c| {
                let h = hist.get().and_then(|h| h.and_then(Result::ok));
                sessao(&c, h, &nick_ativo(e).unwrap_or_default())
            }),
            "Mapa de hunts" => view! {
                <Abas opcoes=METRICAS ativa=metrica />
                {move || carregado(cacada.get(), move |c| {
                    let t = match mercado.get() { Some(Some(Ok(m))) => lista(&m, "tickers").to_vec(), _ => Vec::new() };
                    mapa(&c, metrica.get(), &t)
                })}
            }.into_any(),
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
                        <Kpi titulo="Sua chance" valor=chance.map_or("—".into(), |c| format!("1 em {}", inteiro(1.0 / c.max(1e-12))))
                            ajuda="Chance de shiny por encontro com os bônus atuais da conta." />
                        <Kpi titulo="Shiny no servidor" valor=inteiro(num(&s, "total").unwrap_or(0.0)) unidade="7 dias"
                            ajuda="Avisos globais de shiny capturado, cada pokémon contado uma vez." />
                        <Kpi titulo="Ritmo do servidor" valor=format!("{:.1}", num(&s, "por_hora").unwrap_or(0.0)) unidade="shiny/h" />
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

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn nomes_e_comparacao() {
        assert_eq!(nome_hunt("furious_scizor"), "Furious Scizor");
        assert_eq!(
            vs(110.0, Some(100.0)),
            ("+10% vs. média de 7 dias".into(), Some(true))
        );
        assert_eq!(vs(1.0, None).1, None);
        assert_eq!(hh(1_794_682_800_000.0), "16h");
    }
}
