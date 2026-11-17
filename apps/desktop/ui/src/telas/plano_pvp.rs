//! Próximo corte do ranking, preparação do time e anúncios para testar no PvP.

use leptos::prelude::*;
use serde_json::Value;

use super::{nick_ativo, pvp};
use crate::comp::carregado;
use crate::dados::{inteiro, lista, num, recurso, sprite, txt};
use crate::Estado;

const DEGRAUS: [&str; 4] = ["500", "100", "50", "10"];

fn corte(p: &Value, chave: &str, pos: &str) -> Option<f64> {
    p.pointer(&format!("/{chave}/cortes/{pos}"))?.as_f64()
}

fn metas(p: &Value, mensal: bool) -> AnyView {
    let (eu, cortes) = if mensal {
        (&p["mensal"]["eu"]["eu"], &p["mensal"]["cortes"])
    } else {
        (&p["atual"], &p["cortes"])
    };
    let Some(pontos) = num(eu, "pontos") else {
        return view! { <p class="dica">"Falta a leitura do seu ranking neste período. Abra o ranking correspondente no jogo."</p> }.into_any();
    };
    let posicao = num(eu, if mensal { "pos" } else { "posicao" }).filter(|p| *p > 0.0);
    let fim = if mensal {
        num(&p["mensal"]["eu"], "fimEm")
    } else {
        num(p, "fimSemana")
    };
    let dias_restantes = fim.map(|f| ((f - js_sys::Date::now()) / 86_400_000.0).max(0.0));
    let s = &p["semana"];
    let v = num(s, "vitorias").unwrap_or(0.0);
    let d = num(s, "derrotas").unwrap_or(0.0);
    let pp = pvp::por_partida(
        v,
        d,
        num(s, "ganho").unwrap_or(0.0),
        num(s, "perda").unwrap_or(0.0),
    );
    let ritmo = num(s, "ultimas6h").unwrap_or(0.0) / 6.0;
    let saldo_dia = num(&p["mensal"], "saldoDia");
    let proximo = DEGRAUS.iter().copied().find(|n| {
        let pos = n.parse::<f64>().unwrap_or_default();
        posicao.is_none_or(|p| p > pos)
    });
    view! {
        <div class="caixa">
            <div class="secao">{proximo.map_or("Manter o Top 10".to_owned(), |n| format!("Próximo degrau: Top {n}"))}</div>
            <p>{format!("{} pontos{}",inteiro(pontos),posicao.map_or(String::new(),|p| format!(" · posição #{}",inteiro(p))))}</p>
            <div class="cortes">
                {DEGRAUS.into_iter().map(|pos| {
                    let atual = corte(cortes,"atual",pos);
                    let anterior = corte(cortes,"anterior",pos);
                    let alvo = atual.zip(anterior).map_or(atual.or(anterior), |(a,b)| Some(a.max(b)));
                    let detalhe = alvo.map_or("Corte ainda não coletado.".to_owned(), |alvo| {
                        let falta = (alvo.ceil() + 1.0 - pontos).max(0.0);
                        if falta == 0.0 { return "Acima da referência de pontos.".to_owned(); }
                        let base = format!("Faltam {} pts",inteiro(falta));
                        if mensal {
                            return match (saldo_dia,dias_restantes) {
                                (Some(saldo),Some(dias)) if saldo > 0.0 && dias > 0.0 =>
                                    format!("{base} · {:.1} pts/dia necessários · {:.1} pts/dia observados{}",falta/dias,saldo,
                                        if falta/dias <= saldo { " · no ritmo" } else { " · acima do ritmo" }),
                                (Some(saldo),_) if saldo <= 0.0 => format!("{base} · saldo mensal recente de {saldo:+.1} pts/dia"),
                                _ => format!("{base} · faltam 24 h entre leituras mensais para projetar"),
                            };
                        }
                        match pp {
                            Some(saldo) if saldo > 0.0 && v + d >= 10.0 => {
                                let partidas = (falta / saldo).ceil();
                                let cabe = dias_restantes.filter(|_| ritmo > 0.0).map(|dias| partidas <= dias * 24.0 * ritmo);
                                format!("{base} · ≈ {} partidas{}",inteiro(partidas),
                                    match cabe { Some(true) => " · no ritmo", Some(false) => " · acima do ritmo", None => "" })
                            }
                            Some(saldo) if v + d >= 10.0 => format!("{base} · saldo atual de {saldo:+.1} pts/partida"),
                            _ => format!("{base} · precisa de 10 partidas observadas para projetar"),
                        }
                    });
                    view! { <div class="corte">
                        <div class="corte-t">{format!("Top {pos}")}</div>
                        <div class="corte-v">{alvo.map_or("—".into(), |p| format!("{} pts",inteiro(p.ceil()+1.0)))}</div>
                        <div class="dica">{detalhe}</div>
                        {anterior.is_none().then(|| view! { <small class="dica">"Sem corte de fechamento anterior."</small> })}
                    </div> }
                }).collect_view()}
            </div>
            {(mensal && num(eu,"validas").is_some_and(|v| v < 10.0)).then(|| view! {
                <p class="dica">{format!("O mensal exige 10 partidas válidas. O jogo registrou {}.",inteiro(num(eu,"validas").unwrap_or_default()))}</p>
            })}
            <div class="dica">{if mensal {
                "O corte pode subir até o fim do mês. O ritmo usa a variação dos seus pontos mensais nos últimos 7 dias."
            } else {
                "A referência usa o maior corte disponível entre o atual e o fechamento anterior. O ritmo semanal usa suas partidas desta semana."
            }}</div>
        </div>
    }.into_any()
}

fn estatisticas(sp: &Value) -> String {
    format!(
        "{} usos · {:.0}% de vitórias",
        inteiro(num(sp, "usos").unwrap_or_default()),
        num(sp, "taxa").unwrap_or_default() * 100.0
    )
}

fn mercado(
    ofertas: &Value,
    meta: &Value,
    colecao: &Value,
    mensal: bool,
    recente: Option<&Value>,
    moeda_filtro: &str,
    teto: Option<f64>,
) -> AnyView {
    let todos = lista(meta, "especies");
    let mut vistos = std::collections::HashSet::new();
    let elegivel = |sp: &&Value| {
        num(sp, "usos").unwrap_or_default() >= 20.0 && num(sp, "taxa").unwrap_or_default() >= 0.5
    };
    let idade =
        num(ofertas, "ultimaCaptura").map(|em| ((js_sys::Date::now() - em) / 3_600_000.0).max(0.0));
    if idade.is_some_and(|h| h > 24.0) {
        return view! { <p class="dica">"A captura da Bolsa tem mais de 24 h. Aguarde uma coleta recente para avaliar anúncios."</p> }.into_any();
    }
    let ofertas = lista(ofertas, "ofertas");
    let tem_especie = |nome: &str| {
        lista(colecao, "pokemons")
            .iter()
            .any(|p| txt(p, "nome").eq_ignore_ascii_case(nome))
    };
    let mut candidatos: Vec<_> = ofertas
        .iter()
        .filter_map(|o| {
            let nome = txt(o, "nome");
            let moeda = txt(o, "moeda");
            let desconto = num(o, "desconto")?;
            let preco = num(o, "preco").filter(|p| *p > 0.0)?;
            let sp = todos
                .iter()
                .filter(elegivel)
                .find(|sp| txt(sp, "nome").eq_ignore_ascii_case(&nome))?;
            if moeda != moeda_filtro
                || teto.is_some_and(|maximo| preco > maximo)
                || desconto > -0.1
                || num(o, "comparaveis").unwrap_or_default() < 5.0
                || (mensal
                    && !recente.is_some_and(|r| {
                        lista(r, "especies")
                            .iter()
                            .filter(elegivel)
                            .any(|x| txt(x, "nome").eq_ignore_ascii_case(&nome))
                    }))
            {
                return None;
            }
            Some((o, sp, preco, desconto))
        })
        .collect();
    candidatos.sort_by(|a, b| {
        tem_especie(&txt(a.0, "nome"))
            .cmp(&tem_especie(&txt(b.0, "nome")))
            .then_with(|| a.2.total_cmp(&b.2))
            .then_with(|| a.3.total_cmp(&b.3))
    });
    let top: Vec<_> = candidatos
        .into_iter()
        .filter(|(o, _, _, _)| vistos.insert(txt(o, "nome").to_lowercase()))
        .take(6)
        .collect();
    if top.is_empty() {
        return view! { <p class="dica">{if ofertas.is_empty() { "Sem anúncios avaliados na Bolsa." } else {
            "Nenhum anúncio da amostra passou pelos filtros de preço e desempenho do período."
        }}</p> }.into_any();
    }
    view! {
        <div class="meta-grade">
            {top.into_iter().map(|(o,sp,preco,desconto)| {
                let nome = txt(o,"nome");
                let tem = tem_especie(&nome);
                let moeda = if txt(o,"moeda") == "gold" { "moedas" } else { "gemas" };
                let ficha = &o["ficha"];
                view! { <div class="meta-card">
                    <img src=txt(o,"sprite") alt="" loading="lazy" />
                    <div class="meta-info">
                        <b>{nome}</b>
                        <span>{format!("{} {} · {:.0}% abaixo do valor estimado",inteiro(preco),moeda,-desconto*100.0)}</span>
                        <span class="dica">{format!("{} · Nv {} · anúncio #{}",estatisticas(sp),
                            inteiro(num(ficha,"level").unwrap_or_default()), txt(o,"ofertaId"))}</span>
                        {tem.then(|| view! { <span class="dica">"Você já tem esta espécie. Teste a sua antes de comprar."</span> })}
                    </div>
                </div> }
            }).collect_view()}
        </div>
        <p class="dica">"Anúncios capturados pela Bolsa, sujeitos a venda. A taxa é da espécie nas partidas observadas. Compare a ficha e teste a troca antes de gastar."</p>
    }.into_any()
}

pub fn painel(e: Estado, versao: RwSignal<u32>, modo: RwSignal<&'static str>) -> impl IntoView {
    let periodo = move || {
        if modo.get() == "Mensal" {
            "mensal"
        } else {
            "semanal"
        }
    };
    let ranking = recurso(e, versao, move || {
        nick_ativo(e).map(|n| format!("/api/pvp/painel?nick={n}"))
    });
    let meta = recurso(e, versao, move || {
        Some(format!("/api/pvp/meta?periodo={}", periodo()))
    });
    let recente = recurso(e, versao, move || {
        (periodo() == "mensal").then(|| "/api/pvp/meta?dias=7".into())
    });
    let sugestao = recurso(e, versao, move || {
        nick_ativo(e).map(|n| format!("/api/pvp/sugestao?nick={n}&periodo={}", periodo()))
    });
    let colecao = recurso(e, versao, move || {
        nick_ativo(e).map(|n| format!("/api/equipes/colecao?nick={n}"))
    });
    let moeda = RwSignal::new("orb".to_owned());
    let teto = RwSignal::new(String::new());
    let ofertas = recurso(e, versao, move || {
        Some(format!(
            "/api/bolsa/ofertas?tipo=pokemon&moeda={}&limite=500",
            moeda.get()
        ))
    });
    view! {
        <div class="secao">"Plano de PvP"</div>
        {move || carregado(ranking.get(), |p| metas(&p,periodo()=="mensal"))}
        {move || carregado(meta.get(), |m| {
            let n = num(&m,"partidas").unwrap_or(0.0);
            let m2 = m.clone();
            view! {
                <div class="caixa">
                    <div class="secao">"Seu time contra o meta"</div>
                    <p class="dica">{format!("{} partidas observadas neste período.",inteiro(n))}</p>
                    {move || carregado(sugestao.get(), |s| {
                        let col = colecao.get().flatten().and_then(Result::ok).unwrap_or(Value::Null);
                        let atual = s.pointer("/atual/casas").and_then(Value::as_object);
                        let mut time: Vec<_> = lista(&col,"pokemons").iter().filter(|sp| {
                            let id = txt(sp,"id");
                            atual.is_some_and(|a| a.contains_key(&id))
                        }).collect();
                        time.sort_by_key(|sp| atual.and_then(|a| a.get(&txt(sp,"id"))).and_then(Value::as_i64));
                        let sp_meta = lista(&m2,"especies");
                        let ameacas = sp_meta.iter().take(5).map(|sp| txt(sp,"nome")).collect::<Vec<_>>().join(", ");
                        let primeiro_teste = time.iter().filter_map(|sp| {
                            let id = num(sp,"speciesId").or_else(|| num(sp,"dex"))?;
                            let dados = sp_meta.iter().find(|m| num(m,"especie") == Some(id))?;
                            (num(dados,"usos")? >= 20.0).then_some((txt(sp,"nome"), num(dados,"taxa")?))
                        }).min_by(|a,b| a.1.total_cmp(&b.1))
                            .filter(|(_, taxa)| *taxa < 0.5).map(|(nome, _)| nome);
                        let comparar = time.into_iter().map(|sp| {
                            let id = num(sp,"speciesId").or_else(|| num(sp,"dex")).unwrap_or(0.0);
                            let stats = sp_meta.iter().find(|m| num(m,"especie") == Some(id));
                            let id_arte = super::escaneamento::especie_da_imagem(id as i64);
                            view! { <div class="meta-card">
                                <img src=sprite(id_arte,false) alt="" loading="lazy" />
                                <div class="meta-info"><b>{txt(sp,"nome")}</b><span class="dica">{stats.map_or("Sem amostra no período".into(),estatisticas)}</span></div>
                            </div> }
                        }).collect_view();
                        let trocas = lista(&s["sugestao"],"time").iter().filter(|sp| {
                            let mesmo = atual.and_then(|a| a.get(&txt(sp,"id"))).and_then(Value::as_f64);
                            mesmo.is_none() || mesmo != num(sp,"casa")
                        }).map(|sp| view! { <li>{format!("{} → casa {} · {}",txt(sp,"nome"),
                            inteiro(num(sp,"casa").unwrap_or_default()+1.0),txt(sp,"motivo"))}</li> }).collect_view();
                        view! {
                            {(!ameacas.is_empty()).then(|| view! { <p>{format!("Prepare-se para: {ameacas}.")}</p> })}
                            {atual.is_none().then(|| view! { <p class="dica">"Faltam as casas do time atual. Abra a formação PvP no jogo."</p> })}
                            <div class="meta-grade">{comparar}</div>
                            <div class="secao">"Melhorias para testar com sua coleção"</div>
                            {(n >= 20.0).then(|| view! {
                                <p class="dica">{primeiro_teste.map_or(
                                    "Teste uma troca por vez no Treinamento contra os mesmos adversários.".into(),
                                    |nome| format!("Comece pelo espaço de {nome}, que tem o menor aproveitamento entre suas espécies com amostra. Teste uma mudança por vez.")
                                )}</p>
                                <ul>{trocas}</ul>
                            })}
                            {(n < 20.0).then(|| view! { <p class="dica">"Amostra pequena para sugerir trocas com confiança. Jogue mais partidas neste período."</p> })}
                        }
                    })}
                </div>
                <div class="caixa">
                    <div class="secao">"Compras com bom custo-benefício"</div>
                    <div class="filtros">
                        <label>"Moeda " <select class="caixa" on:change=move |ev| moeda.set(event_target_value(&ev))>
                            <option value="orb">"Gemas"</option><option value="gold">"Moedas"</option>
                        </select></label>
                        <label>"Preço máximo " <input type="number" min="1" placeholder="Sem limite" on:input=move |ev| teto.set(event_target_value(&ev)) /></label>
                    </div>
                    {move || carregado(ofertas.get(), |o| {
                        let r = recente.get().flatten().and_then(Result::ok);
                        let c = colecao.get().flatten().and_then(Result::ok).unwrap_or(Value::Null);
                        mercado(&o,&m,&c,periodo()=="mensal",r.as_ref(),&moeda.get(),teto.get().parse().ok())
                    })}
                    {move || (periodo()=="mensal").then(|| view! { <p class="dica">"No mensal, a espécie também precisa aparecer com bom desempenho nos últimos 7 dias. Reavalie quando o meta ou o mercado mudar."</p> })}
                </div>
            }
        })}
    }
}
