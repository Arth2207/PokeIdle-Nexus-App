//! "Escaneamento de campo": a criatura ativa de uma conta em destaque, com nível, qualidade,
//! IV, nota, HP, XP e a caçada atual. Os dados vêm de `/api/contas` (a janela do motor).

use leptos::prelude::*;
use serde_json::Value;

use crate::dados::{num, sprite, txt};

/// Números de um texto "26.280 / 26.280" ou "7.108.026 / 50.954.600 xp" → fração (0–1).
pub fn fracao(texto: &str) -> Option<f64> {
    let mut partes = texto.split('/').map(|p| {
        p.chars()
            .filter(char::is_ascii_digit)
            .collect::<String>()
            .parse::<f64>()
            .ok()
    });
    let (a, b) = (partes.next()??, partes.next()??);
    (b > 0.0).then(|| (a / b).clamp(0.0, 1.0))
}

/// Espécie para a imagem: formas especiais do jogo (ex.: 3006, mega) caem na forma base.
pub fn especie_da_imagem(sp: i64) -> i64 {
    if (1..=1025).contains(&sp) {
        sp
    } else {
        let base = sp % 1000;
        if (1..=1025).contains(&base) {
            base
        } else {
            0
        }
    }
}

/// Classe do brilho das formas do jogo (Furious, Ancient, Brave), que usam a imagem da
/// espécie base: sem ela ficam iguais à normal.
pub fn forma(nome: &str) -> &'static str {
    match nome.split_whitespace().next() {
        Some("Furious") => "forma-furious",
        Some("Ancient") => "forma-ancient",
        Some("Brave") => "forma-brave",
        _ => "",
    }
}

fn medidor(rotulo: &'static str, texto: String, classe: &'static str) -> impl IntoView {
    let w = format!("width:{:.1}%", fracao(&texto).unwrap_or(0.0) * 100.0);
    view! {
        <div class=format!("medidor {classe}")>
            <b>{rotulo}</b>
            <span class="trilho"><span class="val" style=w></span></span>
            <span>{texto}</span>
        </div>
    }
}

#[component]
pub fn Escaneamento(conta: Value) -> impl IntoView {
    let ativo = conta.get("ativo").cloned().unwrap_or(Value::Null);
    let sp = especie_da_imagem(num(&conta, "especie").unwrap_or(0.0) as i64);
    let shiny = conta.get("shiny").and_then(Value::as_bool).unwrap_or(false);
    let nome = txt(&ativo, "nome");
    let chips: Vec<String> = ["lv", "q", "iv", "nota", "pot"]
        .iter()
        .map(|k| txt(&ativo, k))
        .filter(|t| !t.is_empty())
        .collect();
    view! {
        <div class="scan">
            <div class="scan-img">
                {(sp > 0).then(|| view! { <img class=forma(&nome) src=sprite(sp, shiny) alt=nome.clone() loading="lazy" /> })}
            </div>
            <div>
                <div class="scan-sub">{format!("{} · treinador Nv {}", txt(&conta, "nick"), txt(&conta, "trLv"))}</div>
                <div class="scan-nome">{if nome.is_empty() { "—".to_owned() } else { nome }}{shiny.then_some(" ✦")}</div>
                <div class="chips">{chips.into_iter().map(|c| view! { <span class="chip">{c}</span> }).collect_view()}</div>
                {medidor("HP", txt(&ativo, "hp"), "hp")}
                {medidor("XP", txt(&ativo, "xp"), "xp")}
                <div class="pilula-neon">"Caçada: "{txt(&conta, "hunt")}</div>
            </div>
        </div>
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn fracao_dos_textos_da_tela() {
        assert_eq!(fracao("26280 / 26280"), Some(1.0));
        assert_eq!(
            fracao("7.108.026 / 50.954.600 xp").map(|f| (f * 1000.0).round()),
            Some(139.0)
        );
        assert_eq!(fracao("—"), None);
        assert_eq!(especie_da_imagem(3006), 6);
        assert_eq!(especie_da_imagem(9445), 445);
        assert_eq!(especie_da_imagem(25), 25);
        assert_eq!(forma("Furious Scizor"), "forma-furious");
        assert_eq!(forma("Scizor"), "");
    }
}
