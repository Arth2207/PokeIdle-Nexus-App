//! Combate: como venço este boss (ou esta partida de PvP), e o que me falta para isso?

use leptos::prelude::*;
use serde_json::Value;

use super::cabecalho;
use crate::comp::{carregado, Abas};
use crate::dados::{curto, inteiro, lista, num, recurso, sprite, txt};
use crate::Estado;

const ABAS: &[&str] = &["Bosses", "PvP", "Treinamento", "Raid"];

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
            "PvP" => super::pvp::aba_pvp(e, versao).into_any(),
            "Treinamento" => super::pvp::aba_treino(e, versao).into_any(),
            _ => aba_raid(e, versao).into_any(),
        }}
    }
}
