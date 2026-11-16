//! Safari e Top Catch semanal: estimativa local de espécies únicas e marcos.
use leptos::prelude::*;
use serde_json::Value;

use super::rotulo_ativo;
use crate::comp::{carregado, Barras, Colunas, Kpi, Tabela};
use crate::dados::{inteiro, lista, num, recurso, txt};
use crate::Estado;

fn data(ms: f64) -> String {
    String::from(js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(ms))
        .to_locale_string("pt-BR", &js_sys::JSON::parse(
            r#"{"timeZone":"America/Sao_Paulo","day":"2-digit","month":"2-digit","hour":"2-digit","minute":"2-digit"}"#
        ).unwrap_or(wasm_bindgen::JsValue::UNDEFINED)))
}
fn dia_curto(dia: &str) -> String {
    dia.get(5..)
        .map(|s| s.split('-').rev().collect::<Vec<_>>().join("/"))
        .unwrap_or_else(|| dia.into())
}

fn painel(v: Value, conta: String) -> AnyView {
    let tc = &v["top_catch"];
    if !tc.is_object() || tc["catalogo_disponivel"] != true {
        return view! { <div class="vazio">"Aguardando histórico e Pokédex para classificar as capturas do Top Catch."</div> }.into_any();
    }
    let total = num(&v, "total").unwrap_or(0.0);
    let especies = num(tc, "especies").unwrap_or(0.0);
    let horas = num(&v, "horas_observadas").unwrap_or(0.0);
    let restantes =
        ((num(&v, "fim").unwrap_or(0.0) - num(&v, "agora").unwrap_or(0.0)) / 3.6e6).max(0.0);
    let ritmo = num(tc, "por_hora");
    let projecao = num(tc, "projecao");
    let proximo = &tc["proximo"];
    let meta = num(proximo, "meta");
    let previsao_marco = match num(proximo, "horas") {
        Some(h) if h <= restantes => format!("Neste ritmo: cerca de {h:.1} h para o próximo marco."),
        Some(h) => format!("Neste ritmo: {h:.1} h até o próximo marco, além do reset. Priorize espécies que faltam."),
        None => "O prazo para o próximo marco aparece quando houver ritmo de espécies novas.".into(),
    };
    let marcos = lista(tc, "marcos")
        .iter()
        .map(|m| {
            vec![
                inteiro(num(m, "meta").unwrap_or(0.0)),
                txt(m, "premio"),
                if m["atingido"] == true {
                    "Atingido no histórico".into()
                } else {
                    format!(
                        "Faltam {} espécies",
                        inteiro(num(m, "faltam").unwrap_or(0.0))
                    )
                },
            ]
        })
        .collect::<Vec<_>>();
    let dias = lista(tc, "dias")
        .iter()
        .map(|d| {
            let n = num(d, "total").unwrap_or(0.0);
            (dia_curto(&txt(d, "dia")), n, inteiro(n))
        })
        .collect::<Vec<_>>();
    let por_especie = lista(&v, "por_especie")
        .iter()
        .map(|d| {
            let n = num(d, "total").unwrap_or(0.0);
            (txt(d, "nome"), n, inteiro(n))
        })
        .collect::<Vec<_>>();
    let ultimas = lista(&v, "ultimas")
        .iter()
        .map(|d| {
            vec![
                data(num(d, "em").unwrap_or(0.0)),
                txt(d, "especie"),
                txt(d, "nivel"),
                if d["classificada"] != true {
                    "Sem classificação".into()
                } else if d["elegivel"] != true {
                    "Fora da estimativa".into()
                } else if d["nova"] == true {
                    "Nova na semana".into()
                } else {
                    "Repetida".into()
                },
            ]
        })
        .collect::<Vec<_>>();
    let dias_semana = lista(&v, "dias");
    let periodo = format!(
        "{} · {} a {} · reset {} (Brasília) · faltam {:.1} h",
        conta,
        dias_semana
            .first()
            .map(|d| dia_curto(&txt(d, "dia")))
            .unwrap_or_default(),
        dias_semana
            .last()
            .map(|d| dia_curto(&txt(d, "dia")))
            .unwrap_or_default(),
        data(num(&v, "fim").unwrap_or(0.0)),
        restantes
    );
    view! {
        <div class="secao">"Safari · Top Catch semanal"</div>
        <div class="dica">{periodo}</div>
        <div class="kpis">
            <Kpi titulo="Espécies únicas elegíveis" valor=inteiro(especies)
                comparacao="Estimativa pelo histórico do Nexus".to_owned()
                ajuda="Cada espécie elegível conta uma vez desde domingo, 00h de Brasília. Não é o placar oficial do jogo." />
            <Kpi titulo="Ritmo de espécies novas" valor=ritmo.map_or("—".into(), |n| format!("{n:.1}")) unidade="/h"
                comparacao=format!("{} novas em {:.1} h observadas", inteiro(num(tc, "recentes").unwrap_or(0.0)), horas)
                ajuda="Primeiras capturas elegíveis da semana nas últimas 24 h, divididas pelo tempo observado (até 24 h, mínimo 1 h). Repetidas não aumentam o ritmo." />
            <Kpi titulo="Previsão até o reset" valor=projecao.map_or("—".into(), |n| inteiro(n.floor())) unidade="espécies"
                comparacao=projecao.map_or("Aguardando dados".into(), |n| format!("+{} espécies estimadas", inteiro((n.floor()-especies).max(0.0))))
                ajuda="Espécies únicas já registradas + ritmo de espécies novas × horas até domingo, 00h. Limitada ao catálogo elegível; depende dos níveis e das espécies disponíveis." />
            <Kpi titulo="Total de capturas" valor=inteiro(total) unidade="na semana"
                ajuda="Todas as capturas recebidas na semana, incluindo repetidas e espécies fora do Top Catch estimado." />
        </div>
        <div class="caixa" style="margin-top:16px">
            <div class="caixa-t">"Próximo prêmio"</div>
            {meta.map(|m| view! {
                <div class="secao">{format!("{} / {} espécies · {}", inteiro(especies), inteiro(m), txt(proximo, "premio"))}</div>
                <progress max=m value=especies.min(m) style="width:100%;height:12px;accent-color:var(--neon)" aria-label="Progresso até o próximo marco"></progress>
                <div class="dica">{format!("Faltam {} espécies diferentes.", inteiro((m-especies).max(0.0)))}</div>
                <div class="dica">{previsao_marco.clone()}</div>
            })}
            {meta.is_none().then(|| view! { <div class="dica">"Todos os marcos aparecem atingidos no histórico local."</div> })}
            <Tabela cab=vec!["Espécies", "Recompensa anunciada", "Progresso estimado"] linhas=marcos />
            <div class="dica">"O jogo paga os marcos automaticamente. O Nexus não confirma recebimento nem ativa recompensas."</div>
        </div>
        <div class="caixa" style="margin-top:16px">
            <div class="caixa-t">"Novas espécies elegíveis por dia"</div>
            <Colunas linhas=dias />
            <div class="dica">"Domingo a sábado, em Brasília. A primeira captura elegível de cada espécie conta uma vez por semana."</div>
        </div>
        <div class="dica" style="margin-top:12px">
            {if ritmo.is_none() { "Aguardando pelo menos 1 hora de dados para projetar o ritmo." }
            else { "Estimativa se o ritmo continuar. Ao restarem espécies mais difíceis ou acima do nível atual, a projeção pode diminuir." }}
        </div>
        <div class="vg-base" style="margin-top:16px">
            <div class="caixa">
                <div class="caixa-t">"Capturas por espécie · inclui repetidas"</div>
                {if por_especie.is_empty() {
                    view! { <div class="vazio">"Nenhuma captura registrada nesta semana."</div> }.into_any()
                } else { view! { <Barras linhas=por_especie /> }.into_any() }}
            </div>
            <div class="caixa">
                <div class="caixa-t">"Últimas capturas"</div>
                <Tabela cab=vec!["Quando (Brasília)", "Espécie", "Nível", "Top Catch local"] linhas=ultimas />
            </div>
        </div>
        <div class="dica" style="margin-top:12px">
            {format!("Estimativa local: categoria Pokébola da Pokédex, excluindo Outlands e eventos identificados como Beast Ball. {} capturas sem classificação e {} fora da estimativa. O placar oficial pode diferir, principalmente se houve capturas com o Nexus desconectado.",
                inteiro(num(tc,"sem_classificacao").unwrap_or(0.0)), inteiro(num(tc,"descartadas").unwrap_or(0.0)))}
        </div>
    }.into_any()
}

#[allow(non_snake_case)]
pub fn Safari(e: Estado, versao: RwSignal<u32>) -> impl IntoView {
    let dados = recurso(e, versao, move || {
        rotulo_ativo(e).map(|c| format!("/api/cacada/capturas?conta={c}"))
    });
    view! { {move || carregado(dados.get(), move |v| painel(v, rotulo_ativo(e).unwrap_or_default()))} }
}
