//! Outlands: catálogo do jogo, medições por contexto e valor dos drops.
use super::rotulo_ativo;
use crate::comp::{carregado, Kpi, Tabela};
use crate::dados::{inteiro, lista, num, recurso, txt};
use crate::Estado;
use leptos::prelude::*;
use serde_json::Value;
fn decimal(v: Option<f64>) -> String {
    v.map_or("—".into(), |n| format!("{n:.2}"))
}
fn bonus(v: &Value) -> String {
    let boost = match v.get("lootBoost").and_then(Value::as_bool) {
        Some(true) => "Boost de loot ligado",
        Some(false) => "Boost de loot desligado",
        None => "Boost de loot não informado",
    };
    match num(v, "guildBonusPct") {
        Some(n) => format!("{boost} · guild {n:.1}% informado"),
        None => boost.into(),
    }
}
fn painel(v: Value) -> AnyView {
    let atual = &v["atual"];
    let prox = &v["proxima"];
    let nivel = num(&v, "nivel_treinador").unwrap_or(0.0);
    let regioes = lista(&v, "tiers")
        .iter()
        .map(|t| {
            vec![
                txt(t, "rotulo"),
                inteiro(num(t, "nivel").unwrap_or(0.0)),
                format!("{}×", decimal(num(t, "mult"))),
                if t["tier"] == atual["tier"] {
                    "Atual".into()
                } else if num(t, "nivel").unwrap_or(f64::INFINITY) <= nivel {
                    "Liberada".into()
                } else {
                    "Bloqueada".into()
                },
            ]
        })
        .collect::<Vec<_>>();
    let medidas = lista(&v, "linhas")
        .iter()
        .map(|l| {
            vec![
                txt(l, "especie"),
                txt(&l["tier"], "rotulo"),
                format!("{} · nv {}", txt(l, "ativo"), txt(l, "ativo_nivel")),
                decimal(num(l, "horas")),
                inteiro(num(l, "kills").unwrap_or(0.0)),
                decimal(num(l, "kph")),
                decimal(num(l, "tokens_h_sem_bonus")),
                decimal(num(l, "gemas_h_parcial")),
                decimal(num(l, "valor_pos_comissao")),
                if l["regra_atual"] == true {
                    "Atual".into()
                } else {
                    "Histórica".into()
                },
                bonus(&l["loot"]),
            ]
        })
        .collect::<Vec<_>>();
    let itens = lista(&v, "linhas")
        .iter()
        .flat_map(|l| {
            lista(l, "itens").iter().map(move |d| {
                vec![
                    txt(l, "especie"),
                    txt(&l["tier"], "rotulo"),
                    txt(d, "nome"),
                    inteiro(num(d, "qtd").unwrap_or(0.0)),
                    decimal(num(&d["cotacao"], "preco")),
                    txt(&d["cotacao"], "liquidez"),
                ]
            })
        })
        .collect::<Vec<_>>();
    view! {
        <div class="secao">"Outlands · Regras e rendimento"</div>
        <div class="dica">"Catálogo recebido do jogo. A região selecionada é preservada ao trocar de espécie."</div>
        <div class="kpis">
            <Kpi titulo="Outland selecionada" valor=if atual.is_null(){"Não identificada".into()}else{txt(atual,"rotulo")}
                comparacao=format!("Bônus {}× · nível exigido {}",decimal(num(atual,"mult")),inteiro(num(atual,"nivel").unwrap_or(0.0))) />
            <Kpi titulo="Próxima liberação" valor=if prox.is_null(){"—".into()}else{txt(prox,"rotulo")}
                comparacao=if prox.is_null(){"Sem próxima região no catálogo".into()}else{format!("Nv {} · faltam {} níveis",inteiro(num(prox,"nivel").unwrap_or(0.0)),inteiro((num(prox,"nivel").unwrap_or(0.0)-nivel).max(0.0)))} />
            <Kpi titulo="Chance-base do token" valor=num(&v,"chance_token").map_or("—".into(),|p|format!("{:.4}%",p*100.0))
                comparacao="Antes do tier e dos bônus adicionais".to_owned() />
            <Kpi titulo="Recebido em vendas" valor=decimal(num(&v,"vendas_recebidas_24h")) unidade="gemas · 24 h"
                ajuda="Valor recebido nas vendas da conta segundo o extrato disponível. Não atribui essas vendas às Outlands nem ao inventário da sessão." />
        </div>
        <div class="dica" style="margin-top:12px">{format!("{} abates antigos sem contexto, fora das comparações. {} itens sem cotação em gemas.",inteiro(num(&v,"abates_sem_contexto").unwrap_or(0.0)),inteiro(num(&v,"itens_sem_cotacao").unwrap_or(0.0)))}</div>
        <div class="caixa" style="margin-top:16px">
            <div class="caixa-t">"Medições das últimas 24 horas"</div>
            <Tabela cab=vec!["Espécie","Região","Pokémon ativo","Horas observadas","Abates","Abates/h","Tokens/h sem bônus","Gemas brutas/h · parcial","Inventário pós comissão","Regra","Bônus informados"] linhas=medidas />
            <div class="dica">"Cada linha separa região, espécie, regra, Pokémon, nível e bônus. O ritmo exige 15 minutos observados; pausas longas não contam como tempo de caça. Não compare condições diferentes."</div>
            <div class="dica">"Tokens/h é uma expectativa pela chance-base e pelo tier, sem bônus adicionais. Fragmentos sem chance atual publicada não entram nessa previsão."</div>
        </div>
        <div class="vg-base" style="margin-top:16px">
            <div class="caixa"><div class="caixa-t">"Regiões do catálogo atual"</div><Tabela cab=vec!["Outland","Nível exigido","Multiplicador","Acesso"] linhas=regioes /></div>
            <div class="caixa"><div class="caixa-t">"Drops recebidos e cotações de vendas"</div><Tabela cab=vec!["Espécie","Região","Item","Qtd","Gemas por unidade","Liquidez"] linhas=itens /></div>
        </div>
        <div class="dica" style="margin-top:12px">"O valor de inventário é estimado por vendas recentes do mercado, não por anúncios. Cotação ausente não significa item sem valor. A comissão informada é descontada na simulação; poções, bolas e demais custos em Coins não são convertidos em gemas. Não representa lucro líquido completo."</div>
        <div class="dica">"Escolha XP, equilíbrio, Coins ou gemas em Automação → Troca de hunt → Objetivo da conta. Dados de rendimento anteriores à atualização ficam separados dos novos."</div>
    }.into_any()
}
#[allow(non_snake_case)]
pub fn Outlands(e: Estado, versao: RwSignal<u32>) -> impl IntoView {
    let taxa = RwSignal::new(15.0_f64);
    let dados = recurso(e, versao, move || {
        rotulo_ativo(e).map(|c| format!("/api/outlands?conta={c}&taxa={}", taxa.get() / 100.0))
    });
    view! {
        <label class="op"><span>"Comissão usada na simulação (%)"</span>
            <input class="num" type="number" min="0" max="100" step="0.1" prop:value=move ||taxa.get().to_string()
                on:change={move |ev| { if let Ok(n)=event_target_value(&ev).parse::<f64>() { if n.is_finite() && (0.0..=100.0).contains(&n) { taxa.set(n); } } }} />
        </label>
        <div class="dica">"Valor inicial de 15% para a simulação. Ajuste à comissão mostrada pelo jogo antes de comparar valores pós comissão."</div>
        {move ||carregado(dados.get(),painel)}
    }
}
