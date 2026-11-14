//! Bestiário: o que esta criatura vale, onde ela rende e como eu a melhoro?

use leptos::prelude::*;
use serde_json::Value;

use super::escaneamento::especie_da_imagem;
use super::{cabecalho, nick_ativo};
use crate::comp::{carregado, Abas, Barras, Kpi};
use crate::dados::{curto, inteiro, lista, num, recurso, sprite, txt};
use crate::Estado;

const TIPOS: [&str; 18] = [
    "NORMAL", "FIRE", "WATER", "GRASS", "ELECTRIC", "ICE", "FIGHTING", "POISON", "GROUND",
    "FLYING", "PSYCHIC", "BUG", "ROCK", "GHOST", "DRAGON", "DARK", "STEEL", "FAIRY",
];

const ABAS: &[&str] = &["Coleção", "Pokédex"];
/// shortcut: a Pokédex nacional (1–1025) no lugar da lista de espécies do jogo, que a API não
/// expõe; formas especiais (mega etc.) contam pela forma base. Expor a pokédex do Nexus se faltar.
const POKEDEX: i64 = 1025;

/// Espécie base para a imagem: a que o servidor resolveu (`dex`), senão a regra local.
fn dex(p: &Value) -> i64 {
    num(p, "dex").map_or_else(
        || especie_da_imagem(num(p, "speciesId").unwrap_or(0.0) as i64),
        |d| d as i64,
    )
}

fn img(p: &Value) -> String {
    let sh = p.get("shiny").and_then(Value::as_bool) == Some(true);
    sprite(dex(p), sh)
}

/// Ficha do exemplar escolhido: stats, IVs, nota e o valor na Bolsa.
fn ficha(p: &Value, mercado: Option<Value>) -> impl IntoView {
    let stats: Vec<(String, f64, String)> = ["hp", "atk", "def", "spAtk", "spDef", "speed"]
        .iter()
        .map(|k| {
            let v = p
                .pointer(&format!("/stats/{k}"))
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            let iv = p
                .pointer(&format!("/ivs/{k}"))
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            (
                k.to_uppercase(),
                v,
                format!("{} · IV {}", inteiro(v), inteiro(iv)),
            )
        })
        .collect();
    let sp = num(p, "speciesId").unwrap_or(0.0) as i64;
    let sh = p.get("shiny").and_then(Value::as_bool) == Some(true);
    let id = format!("p:{sp}:{}:orb", u8::from(sh));
    let t = mercado.and_then(|m| {
        lista(&m, "tickers")
            .iter()
            .find(|x| txt(x, "id") == id)
            .cloned()
    });
    view! {
        <div class="caixa" style="margin-bottom:16px">
            <div class="scan">
                <div class="scan-img"><img src=img(p) alt="" /></div>
                <div>
                    <div class="scan-sub">{format!("#{sp} · {}", lista(p, "tipos").iter().filter_map(Value::as_str).collect::<Vec<_>>().join(" / "))}</div>
                    <div class="scan-nome">{txt(p, "nome")}{sh.then_some(" ✦")}</div>
                    <div class="chips">
                        <span class="chip">{format!("Nv {}", inteiro(num(p, "level").unwrap_or(0.0)))}</span>
                        <span class="chip">{format!("Q {:.3}", num(p, "quality").unwrap_or(0.0))}</span>
                        <span class="chip">{format!("IV {}", inteiro(num(p, "ivTotal").unwrap_or(0.0)))}</span>
                        <span class="chip">{format!("nota {:.2}", num(p, "nota").unwrap_or(0.0))}</span>
                        <span class="chip">{format!("poder {}", inteiro(num(p, "poder").unwrap_or(0.0)))}</span>
                    </div>
                    <Barras linhas=stats />
                    <div class="dica">{match t {
                        Some(t) => format!("Na Bolsa: último {} orbs · valor justo {} · sinal {}", curto(num(&t, "ultimo").unwrap_or(0.0)), curto(num(&t, "base").unwrap_or(0.0)), txt(&t, "sinal").to_lowercase()),
                        None => "Sem negócios desta espécie na Bolsa.".to_owned(),
                    }}</div>
                </div>
            </div>
        </div>
    }
}

#[component]
pub fn Bestiario(e: Estado) -> impl IntoView {
    let aba = RwSignal::new("Coleção");
    let escolhido = RwSignal::new(None::<i64>);
    let mercado = recurso(e, RwSignal::new(0u32), || Some("/api/bolsa/mercado".into()));
    let versao = RwSignal::new(0u32);
    let col = recurso(e, versao, move || {
        nick_ativo(e).map(|n| format!("/api/equipes/colecao?nick={n}"))
    });
    let tipo = RwSignal::new(String::new());
    let busca = RwSignal::new(String::new());
    let ordem = RwSignal::new("poder".to_owned());
    view! {
        {cabecalho("Bestiário", "O que esta criatura vale, onde ela rende e como eu a melhoro?", versao)}
        <Abas opcoes=ABAS ativa=aba />
        {move || (aba.get() == "Pokédex").then(|| carregado(col.get(), |v| {
            let tem: std::collections::BTreeSet<i64> = lista(&v, "pokemons").iter()
                .map(dex).filter(|d| *d > 0).collect();
            let n = (1..=POKEDEX).filter(|i| tem.contains(i)).count();
            view! {
                <div class="medidor"><b></b><span class="trilho"><span class="val" style=format!("width:{:.1}%", n as f64 / POKEDEX as f64 * 100.0)></span></span>
                    <span>{format!("{n} de {POKEDEX} espécies ({:.0}%)", n as f64 / POKEDEX as f64 * 100.0)}</span></div>
                <div class="pokedex">
                    {(1..=POKEDEX).map(|i| {
                        let falta = !tem.contains(&i);
                        view! { <img class:falta=falta src=sprite(i, false) alt=format!("#{i}") title=format!("#{i}") loading="lazy" /> }
                    }).collect_view()}
                </div>
            }
        }))}
        <div class="filtros" style:display=move || if aba.get() == "Coleção" { "flex" } else { "none" }>
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
        {move || (aba.get() == "Coleção").then(|| carregado(col.get(), move |v| {
            let todos = lista(&v, "pokemons").to_vec();
            let sel = escolhido.get().and_then(|id| todos.iter().find(|p| num(p, "id") == Some(id as f64)).cloned());
            let m = mercado.get().and_then(|r| r.and_then(Result::ok));
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
                {sel.map(|p| ficha(&p, m))}
                <div class="dica">{format!("Mostrando {} de {total}. Clique num pokémon para ver a ficha.", l.len())}</div>
                <div class="grade-cards">
                    {l.into_iter().map(|p| {
                        let sh = p.get("shiny").and_then(Value::as_bool) == Some(true);
                        let id = num(&p, "id").unwrap_or(0.0) as i64;
                        view! {
                            <div class="mini" on:click=move |_| escolhido.set(Some(id))>
                                <img src=img(&p) alt="" loading="lazy" />
                                <div class="n">{format!("{}{}", txt(&p, "nome"), if sh { " ✦" } else { "" })}</div>
                                <div class="s">{format!("nv {} · poder {}", inteiro(num(&p, "level").unwrap_or(0.0)), inteiro(num(&p, "poder").unwrap_or(0.0)))}</div>
                                <div class="s">{format!("IV {} · nota {:.2} · {}", inteiro(num(&p, "ivTotal").unwrap_or(0.0)), num(&p, "nota").unwrap_or(0.0),
                                    lista(&p, "tipos").iter().filter_map(Value::as_str).collect::<Vec<_>>().join("/"))}</div>
                            </div>
                        }
                    }).collect_view()}
                </div>
            }
        }))}
    }
}
