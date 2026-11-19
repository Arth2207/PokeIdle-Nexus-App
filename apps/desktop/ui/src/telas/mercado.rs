//! Mercado: o que compro, o que vendo e o que guardo?

use leptos::prelude::*;
use serde_json::Value;

use super::cabecalho;
use crate::comp::{carregado, Abas, Kpi, Tabela, Veredito};
use crate::dados::{curto, inteiro, lista, num, pct, recurso, txt};
use crate::Estado;

const ABAS: &[&str] = &[
    "Pregão",
    "Oportunidades",
    "Radar de evolução",
    "Para o PvP",
    "Inventário",
];

const RADAR_ROTAS: &[(&str, &str)] = &[
    ("", "Todas"),
    ("gold-orb", "Coins → Gemas"),
    ("gold-gold", "Coins → Coins"),
    ("orb-orb", "Gemas → Gemas"),
];

const RADAR_COLUNAS: &[&str] = &[
    "Anúncio e evolução",
    "Nota final",
    "Compra",
    "Pedras e nível",
    "Revenda estimada",
    "Resultado líquido",
    "Sinal",
    "Visto às",
];

fn aba_radar_evolucao(e: Estado, versao: RwSignal<u32>) -> impl IntoView {
    let dados = recurso(e, versao, || Some("/api/bolsa/radar-evolucao".into()));
    let busca = RwSignal::new(String::new());
    let rota = RwSignal::new("");
    let prontas = RwSignal::new(false);

    view! {
        <section class="evo-radar" aria-label="Radar de evolução">
            <style>{RADAR_CSS}</style>
            <header class="evo-radar-intro">
                <div>
                    <h3>"Radar de evolução"</h3>
                    <p>"Encontre Pokémon comuns com potencial 4+ na evolução final e compare o custo com vendas recentes."</p>
                </div>
                <button class="btn" type="button" on:click=move |_| versao.update(|v| *v += 1)>
                    "Atualizar radar"
                </button>
            </header>
            <div class="evo-radar-filtros">
                <label class="evo-radar-busca">
                    <span>"Pokémon ou ID do anúncio"</span>
                    <input type="search" placeholder="Eevee ou 1478666" prop:value=move || busca.get()
                        on:input=move |ev| busca.set(event_target_value(&ev)) />
                </label>
                <div class="evo-radar-rotas">
                    <span id="evo-rotas-label">"Moedas da operação"</span>
                    <div class="evo-radar-botoes" role="group" aria-labelledby="evo-rotas-label">
                        {RADAR_ROTAS.iter().map(|&(chave, nome)| view! {
                            <button type="button" class:selecionada=move || rota.get() == chave
                                aria-pressed=move || if rota.get() == chave { "true" } else { "false" }
                                on:click=move |_| rota.set(chave)>{nome}</button>
                        }).collect_view()}
                    </div>
                </div>
                <label class="evo-radar-prontas">
                    <input type="checkbox" prop:checked=move || prontas.get()
                        on:change=move |ev| prontas.set(event_target_checked(&ev)) />
                    "Somente rotas prontas e com comparáveis"
                </label>
            </div>
            {move || match dados.get() {
                Some(Some(Ok(payload))) if payload.get("linhas").is_some_and(Value::is_array) => {
                    let filtro = busca.get().trim().to_lowercase();
                    let moeda = rota.get();
                    let so_prontas = prontas.get();
                    let todas = lista(&payload, "linhas");
                    let linhas = todas.iter()
                        .filter(|linha| moeda.is_empty() || txt(linha, "rota") == moeda)
                        .filter(|linha| {
                            !so_prontas
                                || matches!(txt(linha, "status").as_str(), "Margem positiva" | "Comparar custo por Gema")
                        })
                        .map(|linha| {
                            lista(linha, "celulas").iter()
                                .filter_map(Value::as_str).map(str::to_owned)
                                .collect::<Vec<_>>()
                        })
                        .filter(|linha| linha.len() == RADAR_COLUNAS.len())
                        .filter(|linha| {
                            filtro.is_empty() || linha.iter().any(|celula| celula.to_lowercase().contains(&filtro))
                        })
                        .collect::<Vec<_>>();
                    let qtd = linhas.len();
                    let total = todas.len();
                    let positivo = todas.iter().filter(|linha| txt(linha, "status") == "Margem positiva").count();
                    let atualizado = txt(&payload, "atualizado");
                    let anuncios = txt(&payload, "anuncios");
                    view! {
                        <div class="evo-radar-resumo" aria-live="polite">
                            <span><strong>{anuncios}</strong>" anúncios consultados"</span>
                            <span><strong>{format!("{qtd} de {total}")}</strong>" rotas exibidas"</span>
                            <span><strong>{positivo}</strong>" com margem positiva"</span>
                            <small>{format!("Coleta {atualizado} · horário de Brasília")}</small>
                        </div>
                        {if linhas.is_empty() {
                            let mensagem = if total == 0 {
                                "A coleta atual não encontrou pré-evoluções com potencial 4+."
                            } else {
                                "Nenhum anúncio atende aos filtros atuais."
                            };
                            view! { <div class="evo-radar-estado" role="status">{mensagem}</div> }.into_any()
                        } else {
                            view! { <TabelaRadar linhas=linhas /> }.into_any()
                        }}
                        <p class="evo-radar-rodape">"Os valores de revenda são estimativas. Confira o anúncio pelo ID no Mercado da Comunidade."</p>
                    }.into_any()
                }
                Some(Some(Err(erro))) => view! {
                    <div class="evo-radar-estado" role="alert">
                        <strong>"O radar não conseguiu carregar."</strong>
                        <p>"Confira a conexão do Nexus e tente novamente."</p>
                        <small>{erro}</small>
                        <button class="btn" type="button" on:click=move |_| versao.update(|v| *v += 1)>"Tentar novamente"</button>
                    </div>
                }.into_any(),
                Some(_) => view! {
                    <div class="evo-radar-estado" role="alert">
                        "O Nexus retornou dados incompletos. Use Atualizar radar para tentar novamente."
                    </div>
                }.into_any(),
                None => view! {
                    <div class="evo-radar-estado" role="status">"Carregando anúncios do Mercado da Comunidade…"</div>
                }.into_any(),
            }}
        </section>
    }
}

#[component]
fn TabelaRadar(linhas: Vec<Vec<String>>) -> impl IntoView {
    view! {
        <div class="evo-radar-tabela" tabindex="0" aria-label="Resultados do radar">
            <table>
                <thead><tr>{RADAR_COLUNAS.iter().map(|&nome| view! { <th scope="col">{nome}</th> }).collect_view()}</tr></thead>
                <tbody>
                    {linhas.into_iter().map(|linha| view! {
                        <tr>{linha.into_iter().zip(RADAR_COLUNAS.iter()).enumerate().map(|(i, (texto, &nome))| {
                            let conteudo = match i {
                                0 => {
                                    let (titulo, detalhes) = texto.split_once(" · ").unwrap_or((&texto, ""));
                                    view! { <strong>{titulo.to_owned()}</strong><small>{detalhes.to_owned()}</small> }.into_any()
                                }
                                6 => {
                                    let positivo = matches!(texto.as_str(), "Margem positiva" | "Comparar custo por Gema");
                                    view! { <span class="evo-radar-sinal" class:positivo=positivo>{texto}</span> }.into_any()
                                }
                                _ => texto.into_any(),
                            };
                            view! { <td data-coluna=nome>{conteudo}</td> }
                        }).collect_view()}</tr>
                    }).collect_view()}
                </tbody>
            </table>
        </div>
    }
}

const RADAR_CSS: &str = r#"
.evo-radar { --evo-accent:#286652; display:grid; gap:14px; min-width:0 }
body.noite .evo-radar { --evo-accent:#8bd4b1 }
.evo-radar-intro { display:flex; justify-content:space-between; gap:18px; align-items:start; padding:20px 22px;
  border:1px solid var(--vidro-borda); border-radius:18px; background:var(--vidro); }
.evo-radar-intro h3 { margin:0; font-size:clamp(20px,2.3vw,29px); letter-spacing:-.035em; }
.evo-radar p { margin:6px 0 0; color:var(--texto-2); line-height:1.5; }
.evo-radar-intro button { flex:none; }
.evo-radar-filtros { display:flex; flex-wrap:wrap; gap:12px 18px; align-items:end; }
.evo-radar-busca,.evo-radar-rotas { display:grid; gap:5px; font-size:12px; font-weight:700; color:var(--texto-2); }
.evo-radar-busca { flex:1 1 210px; max-width:360px; }
.evo-radar-busca input,.evo-radar-botoes button { min-height:36px; color:var(--texto); background:var(--vidro);
  border:1px solid var(--vidro-borda); border-radius:8px; font:inherit; padding:7px 11px; }
.evo-radar-busca input { width:100%; font-size:14px; font-weight:500; }
.evo-radar-botoes { display:flex; gap:4px; flex-wrap:wrap; }
.evo-radar-botoes button { cursor:pointer; white-space:nowrap; }
.evo-radar-botoes button.selecionada { color:var(--evo-accent); border-color:var(--evo-accent);
  background:color-mix(in srgb,var(--evo-accent) 12%,transparent); }
.evo-radar :focus-visible { outline:2px solid var(--evo-accent); outline-offset:2px; }
.conteudo:has(.evo-radar) > .resumo > .btn { display:none; }
.evo-radar-prontas { display:flex; align-items:center; gap:6px; min-height:36px; font-size:12px; color:var(--texto-2); }
.evo-radar-prontas input[type=checkbox] { width:auto; margin:0; padding:0; flex:none; accent-color:var(--evo-accent); }
.evo-radar-resumo { display:flex; flex-wrap:wrap; align-items:baseline; gap:6px 18px; font-size:13px; }
.evo-radar small { color:var(--texto-2); }
.evo-radar-resumo small { margin-left:auto; }
.evo-radar-tabela,.evo-radar-estado { border:1px solid var(--vidro-borda); border-radius:14px; background:var(--vidro); }
.evo-radar-tabela { overflow:auto; max-height:58vh; }
.evo-radar-tabela table { border-collapse:separate; border-spacing:0; width:100%; font-size:13px; text-align:left; }
.evo-radar-tabela th,.evo-radar-tabela td { padding:11px 10px; vertical-align:top; border-bottom:1px solid var(--vidro-borda); }
.evo-radar-tabela th { position:sticky; top:0; z-index:1; font-size:11px; background:#e8efea; color:#20362d; }
body.noite .evo-radar-tabela th { background:#1e3029; color:#ebf4ef; }
.evo-radar-tabela td:first-child { min-width:155px; }
.evo-radar-tabela td:nth-child(2),.evo-radar-tabela td:last-child { white-space:nowrap; font-variant-numeric:tabular-nums; }
.evo-radar-tabela td small { display:block; margin-top:3px; }
.evo-radar-sinal { display:inline-block; border-radius:6px; padding:2px 6px; background:var(--trilho); color:var(--texto-2); }
.evo-radar-sinal.positivo { color:var(--evo-accent); background:color-mix(in srgb,var(--evo-accent) 13%,transparent); }
.evo-radar-estado { padding:20px; }
.evo-radar-estado small,.evo-radar-estado button { display:block; margin-top:10px; }
.evo-radar-rodape { font-size:12px; }
@media (max-width:850px) {
  .evo-radar-tabela table { min-width:880px; }
}
@media (max-width:700px) {
  .casca:has(.evo-radar) { flex-direction:column; gap:8px; padding:8px; }
  .casca:has(.evo-radar) .barra { width:100%; flex-direction:row; align-items:center; gap:4px; padding:7px; }
  .casca:has(.evo-radar) .barra .grupo,.casca:has(.evo-radar) .barra .rodape-conta { display:none; }
  .casca:has(.evo-radar) .barra .logo { padding:0 8px; font-size:17px; }
  .casca:has(.evo-radar) .barra .item { width:auto; white-space:nowrap; padding:6px 8px; }
  .casca:has(.evo-radar) .palco { gap:8px; min-height:0; }
  .casca:has(.evo-radar) .topo,.conteudo:has(.evo-radar) .abas { max-width:100%; overflow-x:auto; }
  .conteudo:has(.evo-radar) { padding:12px; }
  .evo-radar-intro { flex-direction:column; padding:15px; }
  .evo-radar-resumo small { margin-left:0; flex-basis:100%; }
  .evo-radar-tabela { max-height:none; }
  .evo-radar-tabela table,.evo-radar-tabela tbody { display:block; min-width:0; }
  .evo-radar-tabela th { position:absolute; width:1px; height:1px; clip-path:inset(50%); padding:0; overflow:hidden; }
  .evo-radar-tabela tr { display:grid; grid-template-columns:repeat(2,minmax(0,1fr)); padding:12px; gap:8px; }
  .evo-radar-tabela tbody tr + tr { border-top:1px solid var(--trilho); }
  .evo-radar-tabela td,.evo-radar-tabela td:nth-child(2),.evo-radar-tabela td:last-child {
    padding:0; border:0; min-width:0; white-space:normal; }
  .evo-radar-tabela td:first-child,.evo-radar-tabela td:nth-child(4) { grid-column:1/-1; }
  .evo-radar-tabela td:not(:first-child)::before {
    content:attr(data-coluna); display:block; font-size:11px; color:var(--texto-2);
  }
}
"#;

fn acao_pt(a: &str) -> String {
    match a {
        "COMPRAR" => "comprar",
        "VENDER" => "vender",
        "AGUARDAR_PARA_COMPRAR" => "esperar para comprar",
        "AGUARDAR_PARA_VENDER" => "esperar para vender",
        "MANTER" => "manter",
        "OBSERVAR" => "observar",
        _ => a,
    }
    .to_owned()
}

fn aba_oportunidades(e: Estado, versao: RwSignal<u32>) -> impl IntoView {
    let d = recurso(e, versao, || Some("/api/bolsa/dicas".into()));
    view! {
        {move || carregado(d.get(), |v| {
            let ops = lista(&v, "oportunidades");
            let top: Vec<Value> = ops.iter().take(3).cloned().collect();
            let linhas: Vec<Vec<String>> = ops.iter().map(|o| vec![
                txt(o, "nome"), txt(o, "moeda"), curto(num(o, "unidade").unwrap_or(0.0)),
                curto(num(o, "justo").unwrap_or(0.0)), num(o, "desconto").map_or("—".into(), pct),
                inteiro(num(o, "quantidade").unwrap_or(0.0)), txt(o, "motivo"),
            ]).collect();
            view! {
                <div class="vereditos">
                    {top.into_iter().map(|o| view! {
                        <Veredito titulo=format!("Comprar {}", txt(&o, "nome")) motivo=txt(&o, "motivo")
                            acao=format!("{} {} cada", curto(num(&o, "unidade").unwrap_or(0.0)), txt(&o, "moeda")) tom="bom" />
                    }).collect_view()}
                </div>
                <div class="dica">{format!("{} ofertas ativas analisadas.", inteiro(num(&v, "ofertasAtivas").unwrap_or(0.0)))}</div>
                <Tabela cab=vec!["Oferta", "Moeda", "Unidade", "Justo", "Desconto", "Qtd.", "Motivo"] linhas=linhas />
            }
        })}
    }
}

/// Um item do inventário avaliado na moeda em que ele de fato gira.
#[derive(Clone, Debug, PartialEq)]
pub struct Avaliado {
    pub nome: String,
    pub qtd: f64,
    /// "gold" ou "orb".
    pub moeda: &'static str,
    pub unit: f64,
    pub valor: f64,
    pub acao: String,
    pub motivo: String,
    pub sprite: String,
}

/// Avalia os itens de `/api/bolsa/meus-itens` pelos tickers de `/api/bolsa/mercado`: cada item
/// na moeda com mais vendas em 30 dias, ao valor justo (`base`, ou o último preço). Sem ticker,
/// fica o valor que a API deu (em orbs). Somar tudo numa moeda só misturava gold e orbs.
pub fn avaliar(itens: &[Value], tickers: &[Value]) -> Vec<Avaliado> {
    itens
        .iter()
        .map(|x| {
            let nome = txt(x, "nome");
            let qtd = num(x, "quantidade").unwrap_or(0.0);
            let t = tickers
                .iter()
                .filter(|t| txt(t, "nome") == nome)
                .max_by(|a, b| {
                    num(a, "vol30d")
                        .unwrap_or(0.0)
                        .total_cmp(&num(b, "vol30d").unwrap_or(0.0))
                });
            let (moeda, unit, sprite) = match t {
                Some(t) => (
                    if txt(t, "moeda") == "gold" {
                        "gold"
                    } else {
                        "orb"
                    },
                    num(t, "base").or_else(|| num(t, "ultimo")).unwrap_or(0.0),
                    txt(t, "sprite"),
                ),
                None => (
                    "orb",
                    if qtd > 0.0 {
                        num(x, "valor").unwrap_or(0.0) / qtd
                    } else {
                        0.0
                    },
                    String::new(),
                ),
            };
            Avaliado {
                nome,
                qtd,
                moeda,
                unit,
                valor: unit * qtd,
                acao: txt(x, "acao"),
                motivo: txt(x, "vmot"),
                sprite,
            }
        })
        .collect()
}

/// (total em gold, total em orbs) do inventário avaliado.
pub fn totais(v: &[Avaliado]) -> (f64, f64) {
    v.iter().fold((0.0, 0.0), |(g, o), a| {
        if a.moeda == "gold" {
            (g + a.valor, o)
        } else {
            (g, o + a.valor)
        }
    })
}

fn aba_inventario(e: Estado, versao: RwSignal<u32>) -> impl IntoView {
    let i = recurso(e, versao, || Some("/api/bolsa/meus-itens".into()));
    let m = recurso(e, versao, || Some("/api/bolsa/mercado".into()));
    view! {
        {move || carregado(i.get(), move |v| carregado(m.get(), move |mk| {
            let mut av = avaliar(lista(&v, "itens"), lista(&mk, "tickers"));
            av.sort_by(|a, b| (b.moeda == "gold").cmp(&(a.moeda == "gold")).then(b.valor.total_cmp(&a.valor)));
            let (g, o) = totais(&av);
            let vender: Vec<&Avaliado> = av.iter().filter(|a| a.acao == "VENDER").collect();
            let (vg, vo) = vender.iter().fold((0.0, 0.0), |(g, o), a| if a.moeda == "gold" { (g + a.valor, o) } else { (g, o + a.valor) });
            let linhas: Vec<Vec<String>> = av.iter().map(|a| vec![
                a.nome.clone(), inteiro(a.qtd), if a.moeda == "gold" { "gold".into() } else { "orbs".into() },
                curto(a.unit), curto(a.valor), acao_pt(&a.acao), a.motivo.clone(),
            ]).collect();
            view! {
                <div class="kpis">
                    <Kpi titulo="Itens em gold" valor=curto(g) unidade="gold" ajuda="Itens que giram em gold, ao valor justo da Bolsa." />
                    <Kpi titulo="Itens em orbs" valor=curto(o) unidade="orbs" ajuda="Itens que giram em orbs, ao valor justo da Bolsa." />
                    <Kpi titulo="Para vender" valor=format!("{} gold · {} orbs", curto(vg), curto(vo)) comparacao=format!("{} itens com sinal de venda", vender.len()) />
                </div>
                <Tabela cab=vec!["Item", "Qtd.", "Moeda", "Valor justo (un.)", "Total", "Sinal", "Motivo"] linhas=linhas />
                <div class="dica">"Cada item é avaliado na moeda em que mais vende (vendas de 30 dias); gold e orbs nunca se somam."</div>
            }
        }))}
    }
}

/// Força de uma espécie no meta, com a taxa puxada para 50% quando há poucas partidas.
pub fn forca_meta(vitorias: f64, usos: f64) -> f64 {
    (vitorias + 5.0) / (usos + 10.0)
}

/// Qualidade do indivíduo (0–1): IV total (até 186), qualidade (até ~2) e potência (até 5).
pub fn qualidade(iv: f64, q: f64, pot: f64) -> f64 {
    (iv / 186.0).clamp(0.0, 1.0) * 0.5
        + (q / 2.0).clamp(0.0, 1.0) * 0.3
        + (pot / 5.0).clamp(0.0, 1.0) * 0.2
}

/// Para o PvP: pokémon à venda bons no meta da faixa da conta, baratos e que melhoram a coleção.
fn aba_pvp(e: Estado, versao: RwSignal<u32>) -> impl IntoView {
    let of = recurso(e, versao, || {
        Some("/api/bolsa/ofertas?tipo=pokemon&limite=500".into())
    });
    let sug = recurso(e, versao, move || {
        super::nick_ativo(e).map(|n| format!("/api/pvp/sugestao?nick={n}"))
    });
    let col = recurso(e, versao, move || {
        super::nick_ativo(e).map(|n| format!("/api/pvp/colecao?nick={n}"))
    });
    view! {
        {move || carregado(sug.get(), move |sg| {
            let faixa = num(&sg["sugestao"], "faixa");
            let meta_faixa = recurso(e, versao, move || Some(match faixa { Some(f) => format!("/api/pvp/meta?faixa={f}"), None => "/api/pvp/meta".into() }));
            let meta_geral = recurso(e, versao, || Some("/api/pvp/meta".into()));
            // Faixa com poucas partidas (ex.: ranking zerado no domingo): vale o meta geral.
            let meta = move || match meta_faixa.get() {
                Some(Some(Ok(m))) if num(&m, "partidas").unwrap_or(0.0) >= 300.0 => Some(Some(Ok(m))),
                Some(Some(Ok(_))) => meta_geral.get(),
                outro => outro,
            };
            view! {
                {move || carregado(meta(), move |m| carregado(of.get(), move |o| {
                    let minha: Vec<Value> = match col.get() { Some(Some(Ok(c))) => lista(&c, "pokemons").to_vec(), _ => Vec::new() };
                    let nivel_min = 150.0;
                    let mut l: Vec<(f64, Value, Value, Option<Value>)> = lista(&o, "ofertas").iter().filter_map(|x| {
                        let nome = txt(x, "nome");
                        let me = lista(&m, "especies").iter().find(|s| txt(s, "nome") == nome)?.clone();
                        let usos = num(&me, "usos").unwrap_or(0.0);
                        let fm = forca_meta(num(&me, "vitorias").unwrap_or(0.0), usos);
                        let f = &x["ficha"];
                        if usos < 8.0 || fm < 0.47 || num(f, "level").unwrap_or(0.0) < nivel_min {
                            return None;
                        }
                        let ql = qualidade(num(f, "ivTotal").unwrap_or(0.0), num(f, "quality").unwrap_or(0.0), num(f, "potencia").unwrap_or(0.0));
                        let meu = minha.iter().filter(|p| txt(p, "nome") == nome)
                            .max_by(|a, b| qualidade(num(a, "ivTotal").unwrap_or(0.0), num(a, "quality").unwrap_or(0.0), num(a, "potencia").unwrap_or(0.0))
                                .total_cmp(&qualidade(num(b, "ivTotal").unwrap_or(0.0), num(b, "quality").unwrap_or(0.0), num(b, "potencia").unwrap_or(0.0))))
                            .cloned();
                        let ql_meu = meu.as_ref().map(|p| qualidade(num(p, "ivTotal").unwrap_or(0.0), num(p, "quality").unwrap_or(0.0), num(p, "potencia").unwrap_or(0.0)));
                        let melhora = ql_meu.is_none_or(|q| ql > q + 0.03);
                        let desconto = (-num(x, "desconto").unwrap_or(0.0)).clamp(-0.5, 0.9);
                        let nota = fm * (0.6 + 0.4 * ql) * (1.0 + desconto) * if melhora { 1.0 } else { 0.5 };
                        Some((nota, x.clone(), me, meu))
                    }).collect();
                    l.sort_by(|a, b| b.0.total_cmp(&a.0));
                    // No máximo 2 ofertas por espécie: variedade para montar o time.
                    let mut vistas: std::collections::HashMap<String, usize> = Default::default();
                    l.retain(|x| { let c = vistas.entry(txt(&x.1, "nome")).or_default(); *c += 1; *c <= 2 });
                    let n = l.len();
                    let base = format!("{} partidas", inteiro(num(&m, "partidas").unwrap_or(0.0)));
                    view! {
                        <div class="dica">{format!("{n} ofertas boas no meta ({base}), do nível {nivel_min} para cima, até 2 por espécie. Ordem: força no meta × qualidade × desconto, priorizando o que melhora sua coleção.")}</div>
                        <div class="ofertas-pvp">
                            {l.into_iter().take(30).map(|(_, x, me, meu)| {
                                let f = x["ficha"].clone();
                                let taxa = forca_meta(num(&me, "vitorias").unwrap_or(0.0), num(&me, "usos").unwrap_or(0.0));
                                let w = format!("width:{:.0}%", taxa * 100.0);
                                let desc = num(&x, "desconto").unwrap_or(0.0);
                                let classe_desc = if desc < 0.0 { "melhora" } else { "piora" };
                                let moeda = if txt(&x, "moeda") == "gold" { "gold" } else { "orbs" };
                                let situacao = match &meu {
                                    None => "Você não tem esta espécie".to_owned(),
                                    Some(p) => format!("O seu: nv {} · IV {} · P{}", inteiro(num(p, "level").unwrap_or(0.0)), inteiro(num(p, "ivTotal").unwrap_or(0.0)), inteiro(num(p, "potencia").unwrap_or(0.0))),
                                };
                                view! {
                                    <div class="oferta-pvp">
                                        <div class="op-topo">
                                            <img src=txt(&x, "sprite") alt="" loading="lazy" />
                                            <div>
                                                <b>{txt(&x, "nome")}</b>
                                                <div class="op-preco">{format!("{} {moeda}", curto(num(&x, "preco").unwrap_or(0.0)))}<span class=classe_desc>{format!(" {} vs justo", pct(desc))}</span></div>
                                            </div>
                                        </div>
                                        <div class="chips">
                                            <span class="chip">{format!("Nv {}", inteiro(num(&f, "level").unwrap_or(0.0)))}</span>
                                            <span class="chip">{format!("IV {}", inteiro(num(&f, "ivTotal").unwrap_or(0.0)))}</span>
                                            <span class="chip">{format!("Q {:.2}", num(&f, "quality").unwrap_or(0.0))}</span>
                                            <span class="chip">{format!("P{}", inteiro(num(&f, "potencia").unwrap_or(0.0)))}</span>
                                            {f.get("shiny").and_then(Value::as_bool).unwrap_or(false).then(|| view! { <span class="chip">"✦ shiny"</span> })}
                                        </div>
                                        <div class="barra-taxa bom"><span style=w></span></div>
                                        <div class="dica">{format!("{:.0}% de vitória no meta · {} usos · alcance {}", taxa * 100.0, inteiro(num(&me, "usos").unwrap_or(0.0)), inteiro(num(&me, "alcance").unwrap_or(0.0)))}</div>
                                        <div class="dica">{situacao}</div>
                                    </div>
                                }
                            }).collect_view()}
                        </div>
                    }
                }))}
            }
        })}
    }
}

#[component]
pub fn Mercado(e: Estado, moeda: RwSignal<String>) -> impl IntoView {
    let versao = RwSignal::new(0u32);
    let aba = RwSignal::new("Pregão");
    view! {
        {cabecalho("Mercado", "O que compro, o que vendo e o que guardo?", versao)}
        <Abas opcoes=ABAS ativa=aba />
        {move || match aba.get() {
            "Pregão" => super::pregao::aba(e, versao, moeda).into_any(),
            "Oportunidades" => aba_oportunidades(e, versao).into_any(),
            "Radar de evolução" => aba_radar_evolucao(e, versao).into_any(),
            "Para o PvP" => aba_pvp(e, versao).into_any(),
            _ => aba_inventario(e, versao).into_any(),
        }}
    }
}

#[cfg(test)]
mod testes {
    use super::*;
    use serde_json::json;

    #[test]
    fn item_vai_na_moeda_que_mais_vende() {
        let itens = json!([{"nome": "Leaf Stone", "quantidade": 2, "valor": 1026}, {"nome": "Raro", "quantidade": 4, "valor": 40}]);
        let t = json!([
            {"nome": "Leaf Stone", "moeda": "gold", "base": 87000.0, "vol30d": 2359},
            {"nome": "Leaf Stone", "moeda": "orb", "base": 96.5, "vol30d": 18}]);
        let av = avaliar(itens.as_array().unwrap(), t.as_array().unwrap());
        assert_eq!((av[0].moeda, av[0].valor), ("gold", 174_000.0));
        assert_eq!((av[1].moeda, av[1].unit), ("orb", 10.0));
        assert_eq!(totais(&av), (174_000.0, 40.0));
        // Poucas partidas puxam a taxa para 50%; qualidade máxima dá 1.
        assert!((forca_meta(10.0, 10.0) - 0.75).abs() < 1e-9);
        assert!((qualidade(186.0, 2.0, 5.0) - 1.0).abs() < 1e-9);
    }
}
