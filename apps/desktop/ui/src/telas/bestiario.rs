//! Bestiário: o que esta criatura vale, onde ela rende e como eu a melhoro?

use leptos::prelude::*;
use serde_json::Value;

use super::{cabecalho, nick_ativo};
use crate::comp::{carregado, Kpi};
use crate::dados::{inteiro, lista, num, recurso, sprite, txt};
use crate::Estado;

const TIPOS: [&str; 18] = [
    "NORMAL", "FIRE", "WATER", "GRASS", "ELECTRIC", "ICE", "FIGHTING", "POISON", "GROUND",
    "FLYING", "PSYCHIC", "BUG", "ROCK", "GHOST", "DRAGON", "DARK", "STEEL", "FAIRY",
];

#[component]
pub fn Bestiario(e: Estado) -> impl IntoView {
    let versao = RwSignal::new(0u32);
    let col = recurso(e, versao, move || {
        nick_ativo(e).map(|n| format!("/api/equipes/colecao?nick={n}"))
    });
    let tipo = RwSignal::new(String::new());
    let busca = RwSignal::new(String::new());
    let ordem = RwSignal::new("poder".to_owned());
    view! {
        {cabecalho("Bestiário", "O que esta criatura vale, onde ela rende e como eu a melhoro?", versao)}
        <div class="filtros">
            <input placeholder="Buscar pelo nome" prop:value=move || busca.get() on:input=move |ev| busca.set(event_target_value(&ev)) />
            <select class="caixa" on:change=move |ev| tipo.set(event_target_value(&ev))>
                <option value="">"Todos os tipos"</option>
                {TIPOS.iter().map(|t| view! { <option value=*t>{*t}</option> }).collect_view()}
            </select>
            <select class="caixa" on:change=move |ev| ordem.set(event_target_value(&ev))>
                <option value="poder">"Mais fortes"</option>
                <option value="level">"Maior nível"</option>
                <option value="ivTotal">"Maior IV"</option>
                <option value="nota">"Maior nota"</option>
            </select>
        </div>
        {move || carregado(col.get(), move |v| {
            let todos = lista(&v, "pokemons").to_vec();
            let especies: std::collections::BTreeSet<i64> = todos.iter().filter_map(|p| p.get("speciesId")?.as_i64()).collect();
            let shiny = todos.iter().filter(|p| p.get("shiny").and_then(Value::as_bool) == Some(true)).count();
            let (t, q, o) = (tipo.get(), busca.get().to_lowercase(), ordem.get());
            let mut l: Vec<Value> = todos.iter().filter(|p| {
                (t.is_empty() || lista(p, "tipos").iter().any(|x| x.as_str() == Some(t.as_str())))
                    && (q.is_empty() || txt(p, "nome").to_lowercase().contains(&q))
            }).cloned().collect();
            l.sort_by(|a, b| num(b, &o).unwrap_or(0.0).total_cmp(&num(a, &o).unwrap_or(0.0)));
            let total = l.len();
            l.truncate(120);
            view! {
                <div class="kpis">
                    <Kpi titulo="Pokémon" valor=inteiro(todos.len() as f64) ajuda="Coleção da conta como o app mandou ao Nexus." />
                    <Kpi titulo="Espécies" valor=inteiro(especies.len() as f64) />
                    <Kpi titulo="Shiny" valor=inteiro(shiny as f64) />
                    <Kpi titulo="Nível do treinador" valor=inteiro(num(&v, "trLv").unwrap_or(0.0)) />
                </div>
                <div class="dica">{format!("Mostrando {} de {total}.", l.len())}</div>
                <div class="grade-cards">
                    {l.into_iter().map(|p| {
                        let sh = p.get("shiny").and_then(Value::as_bool) == Some(true);
                        view! {
                            <div class="mini">
                                <img src=sprite(num(&p, "speciesId").unwrap_or(0.0) as i64, sh) alt="" loading="lazy" />
                                <div class="n">{format!("{}{}", txt(&p, "nome"), if sh { " ✦" } else { "" })}</div>
                                <div class="s">{format!("nv {} · poder {}", inteiro(num(&p, "level").unwrap_or(0.0)), inteiro(num(&p, "poder").unwrap_or(0.0)))}</div>
                                <div class="s">{format!("IV {} · nota {:.2} · {}", inteiro(num(&p, "ivTotal").unwrap_or(0.0)), num(&p, "nota").unwrap_or(0.0),
                                    lista(&p, "tipos").iter().filter_map(Value::as_str).collect::<Vec<_>>().join("/"))}</div>
                            </div>
                        }
                    }).collect_view()}
                </div>
            }
        })}
    }
}
