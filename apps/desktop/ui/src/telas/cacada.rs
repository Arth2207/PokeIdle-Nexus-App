//! Caçada: onde caçar, e a caçada atual está rendendo o esperado? (mockups 03, 04 e 05)
//! Sessão e mapa de hunts vêm de `/api/cacada` (Postgres: `fato_kill`, `fato_hunt_eficiencia`).

use leptos::prelude::*;
use serde_json::Value;

use super::{cabecalho, nick_ativo, rotulo_ativo};
use crate::comp::{carregado, Abas, Barras, Calor, Kpi, Linha, Tabela, Veredito};
use crate::dados::{curto, inteiro, lista, num, recurso, txt};
use crate::Estado;

const ABAS: &[&str] = &["Sessão ao vivo", "Mapa de hunts", "Shiny"];
const METRICAS: &[&str] = &["Ouro/h", "XP/h", "Abates/h"];

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
    let drops: Vec<Vec<String>> = lista(c, "drops")
        .iter()
        .map(|d| {
            vec![
                txt(d, "item"),
                inteiro(num(d, "qtd").unwrap_or(0.0)),
                txt(d, "de"),
            ]
        })
        .collect();
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
                <div class="caixa-t">"Drops da sessão (últimas 3 h)"</div>
                <Tabela cab=vec!["Item", "Qtd", "De"] linhas=drops />
            </div>
            <div class="caixa">
                <div class="caixa-t">"Linha do tempo"</div>
                <Tabela cab=vec!["Hora", "O que houve"] linhas=eventos />
            </div>
        </div>
    }
    .into_any()
}

fn mapa(c: &Value, metrica: &'static str) -> AnyView {
    let h = &c["hunts"];
    let k = match metrica {
        "XP/h" => "xp",
        "Abates/h" => "kills",
        _ => "ouro",
    };
    let atual = txt(&c["sessao"], "atual");
    let mut l: Vec<Value> = lista(h, "lista").to_vec();
    l.sort_by(|a, b| {
        num(b, k)
            .unwrap_or(0.0)
            .total_cmp(&num(a, k).unwrap_or(0.0))
    });
    let melhor = l.first().cloned().unwrap_or(Value::Null);
    let da_atual = l
        .iter()
        .find(|x| txt(x, "hunt") == atual)
        .and_then(|x| num(x, k));
    let ganho = da_atual.map(|a| num(&melhor, k).unwrap_or(0.0) - a);
    let barras: Vec<(String, f64, String)> = l
        .iter()
        .map(|x| {
            let v = num(x, k).unwrap_or(0.0);
            (
                nome_hunt(&txt(x, "hunt")),
                v,
                format!(
                    "{} · {} h medidas",
                    curto(v),
                    inteiro(num(x, "horas").unwrap_or(0.0))
                ),
            )
        })
        .collect();
    let mut grade: std::collections::BTreeMap<String, Vec<Option<f64>>> = Default::default();
    for x in lista(h, "calor") {
        let linha = grade
            .entry(nome_hunt(&txt(x, "hunt")))
            .or_insert_with(|| vec![None; 24]);
        if let Some(hr) = num(x, "hora").map(|h| h as usize).filter(|h| *h < 24) {
            linha[hr] = num(x, "ouro");
        }
    }
    let trocar = ganho.filter(|g| *g > 0.0 && txt(&melhor, "hunt") != atual);
    view! {
        <div class="kpis">
            <Kpi titulo="Melhor hunt (7 dias)" valor=nome_hunt(&txt(&melhor, "hunt")) comparacao=format!("{} {metrica}", curto(num(&melhor, k).unwrap_or(0.0))) />
            <Kpi titulo="Sua hunt atual" valor=nome_hunt(&atual) comparacao=da_atual.map_or("sem medida nos 7 dias".into(), |v| format!("{} {metrica}", curto(v))) />
            <Kpi titulo="Ganho potencial" valor=ganho.map_or("—".into(), |g| format!("{g:+.0}")) unidade=metrica bom=ganho.map(|g| g <= 0.0) />
        </div>
        {trocar.map(|g| view! {
            <Veredito titulo=format!("Trocar para {}", nome_hunt(&txt(&melhor, "hunt")))
                motivo=format!("+{} {metrica} medidos nos últimos 7 dias (todas as contas, sessões de 30 min ou mais).", curto(g))
                acao="O motor troca de hunt sozinho quando a regra da conta permite.".to_owned() tom="bom" />
        })}
        <div class="vg-base" style="margin-top:16px">
            <div class="caixa">
                <div class="caixa-t">{format!("Hunts por {metrica}")}</div>
                <Barras linhas=barras destaque=nome_hunt(&atual) />
            </div>
            <div class="caixa">
                <div class="caixa-t">"Mapa de calor · hunt × hora (ouro/h)"</div>
                <Calor linhas=grade.into_iter().collect() />
            </div>
        </div>
    }
    .into_any()
}

#[component]
pub fn Cacada(e: Estado) -> impl IntoView {
    let versao = RwSignal::new(0u32);
    let aba = RwSignal::new("Sessão ao vivo");
    let metrica = RwSignal::new("Ouro/h");
    let cacada = recurso(e, versao, move || {
        Some(format!(
            "/api/cacada?conta={}",
            rotulo_ativo(e).unwrap_or_default()
        ))
    });
    let hist = recurso(e, versao, || Some("/api/historico".into()));
    let shiny = recurso(e, versao, || Some("/api/shiny/servidor?dias=7".into()));
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
                {move || carregado(cacada.get(), move |c| mapa(&c, metrica.get()))}
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
