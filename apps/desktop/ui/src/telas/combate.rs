//! Combate: como venço este boss (ou esta partida de PvP), e o que me falta para isso?

use leptos::prelude::*;
use serde_json::Value;

use super::{cabecalho, nick_ativo};
use crate::comp::{carregado, Abas, Kpi, Tabela, Veredito};
use crate::dados::{curto, inteiro, lista, num, recurso, sprite, txt};
use crate::Estado;

const ABAS: &[&str] = &["Bosses", "PvP", "Treinamento", "Raid"];

/// Tabuleiro 5×5 (casas 1–25) com os pokémon nas casas.
fn tabuleiro(pecas: Vec<(i64, i64, String)>) -> impl IntoView {
    view! {
        <div class="tabuleiro">
            {(1..=25).map(|casa| {
                let p = pecas.iter().find(|p| p.0 == casa).cloned();
                let ocupada = p.is_some();
                let titulo = p.as_ref().map(|p| p.2.clone()).unwrap_or_default();
                view! {
                    <div class="casa" class:ocupada=ocupada title=titulo>
                        {match p {
                            Some((_, esp, nome)) => view! { <img src=sprite(esp, false) alt=nome /> }.into_any(),
                            None => view! { <span>{casa}</span> }.into_any(),
                        }}
                    </div>
                }
            }).collect_view()}
        </div>
    }
}

fn aba_pvp(e: Estado, versao: RwSignal<u32>) -> impl IntoView {
    let sug = recurso(e, versao, move || {
        nick_ativo(e).map(|n| format!("/api/pvp/sugestao?nick={n}"))
    });
    view! {
        {move || carregado(sug.get(), move |p| {
            let s = p.get("sugestao").cloned().unwrap_or(Value::Null);
            let atual = p.get("atual").cloned().unwrap_or(Value::Null);
            let faixa = num(&s, "faixa");
            let meta = recurso(e, versao, move || Some(match faixa {
                Some(f) => format!("/api/pvp/meta?faixa={f}"),
                None => "/api/pvp/meta".into(),
            }));
            let pecas: Vec<(i64, i64, String)> = lista(&s, "time").iter().map(|x| (
                num(x, "casa").unwrap_or(0.0) as i64, num(x, "especie").unwrap_or(0.0) as i64, txt(x, "nome"))).collect();
            let linhas: Vec<Vec<String>> = lista(&s, "time").iter().map(|x| vec![
                txt(x, "nome"), inteiro(num(x, "nivel").unwrap_or(0.0)), inteiro(num(x, "alcance").unwrap_or(0.0)),
                inteiro(num(x, "casa").unwrap_or(0.0)), txt(x, "motivo"),
            ]).collect();
            view! {
                <div class="kpis">
                    <Kpi titulo="Elo" valor=format!("{} {}", txt(&atual, "tier"), txt(&atual, "divisao")) comparacao=format!("{} pontos", inteiro(num(&atual, "pontos").unwrap_or(0.0))) />
                    <Kpi titulo="Vitórias" valor=inteiro(num(&atual, "vitorias").unwrap_or(0.0)) comparacao=format!("{} derrotas", inteiro(num(&atual, "derrotas").unwrap_or(0.0))) />
                    <Kpi titulo="Partidas no meta" valor=inteiro(num(&s, "partidas_no_meta").unwrap_or(0.0))
                        ajuda="Partidas das suas contas usadas para medir o meta desta faixa de pontos." />
                </div>
                <div class="duas">
                    <div>
                        <div class="secao">"Time e casas sugeridos"</div>
                        {tabuleiro(pecas)}
                        <Veredito titulo="Aplicar no PvP".to_owned()
                            motivo="Com o modo Ativo e “Aprender PvP” ligados, o app testa e aplica sozinho; ou monte este time no jogo.".to_owned() />
                    </div>
                    <div>
                        <div class="secao">"Por quê"</div>
                        <Tabela cab=vec!["Pokémon", "Nível", "Alcance", "Casa", "Motivo"] linhas=linhas />
                    </div>
                </div>
                <div class="secao">"Meta da faixa: espécies"</div>
                {move || carregado(meta.get(), |m| {
                    let linhas: Vec<Vec<String>> = lista(&m, "especies").iter().take(25).map(|x| vec![
                        txt(x, "nome"), inteiro(num(x, "usos").unwrap_or(0.0)),
                        format!("{:.0}%", num(x, "taxa").unwrap_or(0.0) * 100.0),
                        inteiro(num(x, "alcance").unwrap_or(0.0)), inteiro(num(x, "nivel_medio").unwrap_or(0.0)),
                        x.get("casas").and_then(Value::as_array).and_then(|c| c.first()).and_then(|c| c.get(0)).map_or("—".into(), |c| c.to_string()),
                    ]).collect();
                    view! { <Tabela cab=vec!["Espécie", "Usos", "Vitória", "Alcance", "Nível médio", "Casa mais usada"] linhas=linhas /> }
                })}
            }
        })}
    }
}

fn aba_treino(e: Estado, versao: RwSignal<u32>) -> impl IntoView {
    let t = recurso(e, versao, move || {
        nick_ativo(e).map(|n| format!("/api/pvp/treinos?nick={n}"))
    });
    view! {
        {move || carregado(t.get(), |v| {
            let lutas = lista(&v, "treinos");
            let vitorias = lutas.iter().filter(|x| x.pointer("/resultado/venceuA").and_then(Value::as_bool) == Some(false)).count();
            let linhas: Vec<Vec<String>> = lutas.iter().map(|x| {
                let r = x.get("resultado").cloned().unwrap_or(Value::Null);
                let casas = |lado: i64| lista(&r, "placar").iter()
                    .find(|l| num(l, "lado") == Some(lado as f64))
                    .map(|l| lista(l, "pokemons").iter().map(|p| format!("{}@{}", txt(p, "nome"), txt(p, "casa"))).collect::<Vec<_>>().join(" "))
                    .unwrap_or_default();
                vec![
                    if r.get("venceuA").and_then(Value::as_bool) == Some(false) { "vitória".into() } else { "derrota".into() },
                    txt(&r, "motivo"), casas(2), casas(1),
                ]
            }).collect();
            view! {
                <div class="kpis">
                    <Kpi titulo="Lutas no treino" valor=inteiro(lutas.len() as f64) unidade="últimas" />
                    <Kpi titulo="Vitórias" valor=inteiro(vitorias as f64) comparacao=if lutas.is_empty() { "—".into() } else { format!("{:.0}%", vitorias as f64 * 100.0 / lutas.len() as f64) } />
                </div>
                <Tabela cab=vec!["Resultado", "Motivo", "Seu time (casa)", "Adversário teste"] linhas=linhas />
                <div class="dica">"Liga em Configurações › Aprender PvP (modo Ativo): uma luta a cada 5 min na primeira conta."</div>
            }
        })}
    }
}

fn aba_raid(e: Estado, versao: RwSignal<u32>) -> impl IntoView {
    let f = recurso(e, versao, || Some("/api/boss/fichas".into()));
    view! {
        {move || carregado(f.get(), |v| {
            let fichas = lista(&v, "fichas").to_vec();
            view! {
                <div class="grade-cards">
                    {fichas.into_iter().map(|b| {
                        let golpes = b.get("golpes").and_then(Value::as_object).map(|g| g.values()
                            .map(|x| format!("{} ({})", txt(x, "nome"), txt(x, "tipo"))).collect::<Vec<_>>().join(" · ")).unwrap_or_default();
                        view! {
                            <div class="mini" title=golpes>
                                <img src=sprite(num(&b, "dex").unwrap_or(0.0) as i64, false) alt="" />
                                <div class="n">{txt(&b, "nome")}</div>
                                <div class="s">{format!("nível {} · {}", inteiro(num(&b, "nivel").unwrap_or(0.0)), lista(&b, "tipos").iter().filter_map(Value::as_str).collect::<Vec<_>>().join("/"))}</div>
                                <div class="s">{format!("HP {} · ataque ×{} · defesa ×{}", curto(num(&b, "hpMax").unwrap_or(0.0)), num(&b, "multAtaque").unwrap_or(1.0), num(&b, "multDefesa").unwrap_or(1.0))}</div>
                            </div>
                        }
                    }).collect_view()}
                </div>
            }
        })}
    }
}

#[component]
pub fn Combate(e: Estado) -> impl IntoView {
    let versao = RwSignal::new(0u32);
    let aba = RwSignal::new("Bosses");
    view! {
        {cabecalho("Combate", "Como venço este boss ou esta partida, e o que me falta para isso?", versao)}
        <Abas opcoes=ABAS ativa=aba />
        {move || match aba.get() {
            "Bosses" => super::bosses::aba(e, versao).into_any(),
            "PvP" => aba_pvp(e, versao).into_any(),
            "Treinamento" => aba_treino(e, versao).into_any(),
            _ => aba_raid(e, versao).into_any(),
        }}
    }
}
