//! Safari do Top Catch: sorteia espécies elegíveis que ainda faltam na semana.
use super::{ContaViva, Decisao, EstadoLaco, Registro};
use crate::combate::especie::Pokedex;
use crate::motor::{
    config,
    jogo::{hunt_atual, num, slug_de},
    Relogio,
};
use crate::top_catch;
use serde_json::Value;
use std::collections::{hash_map::RandomState, HashSet};
use std::hash::BuildHasher;

fn aviso(laco: &mut EstadoLaco, r: &Relogio, nick: &str, chave: &str, texto: &str) -> Vec<Decisao> {
    if laco.marca(nick, chave) > 0.0 && r.agora_ms as f64 - laco.marca(nick, chave) < 300_000.0 {
        return vec![];
    }
    laco.marcar(nick, chave, r.agora_ms as f64);
    vec![Decisao::Registro {
        nick: nick.into(),
        tipo: "safari".into(),
        texto: texto.into(),
        ok: true,
    }]
}

pub fn motor(
    d: &Value,
    laco: &mut EstadoLaco,
    r: &Relogio,
    conta: &ContaViva,
    dex: &Pokedex,
) -> Vec<Decisao> {
    if config::efetiva(d, &conta.nick).pointer("/safari/ativo") != Some(&Value::Bool(true)) {
        return vec![];
    }
    let nick = &conta.nick;
    if conta.ultimo.get("sessaoTomada") == Some(&Value::Bool(true)) {
        return vec![];
    }
    let inicio = top_catch::inicio_semana(r.agora_ms);
    let Some(tc) = conta.top_catch.as_ref().filter(|tc| {
        tc["inicio"].as_f64() == Some(inicio as f64)
            && tc["catalogo_disponivel"] == true
            && tc["capturadas_ids"].is_array()
    }) else {
        return aviso(
            laco,
            r,
            nick,
            "safariSemHistorico",
            "Safari: aguardando histórico e Pokédex da semana para evitar espécies repetidas.",
        );
    };
    let capturadas: HashSet<i64> = tc["capturadas_ids"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_i64)
        .collect();
    // A captura precisa ter chegado ao histórico classificado. Assim o motor não
    // confunde linhas sem informação da bola com confirmações elegíveis.
    let Some((nome, nivel)) = hunt_atual(conta) else {
        return vec![];
    };
    let atual = dex.por_nome(&nome);
    // Uma hunt não elegível também dá lugar a uma espécie que falta, ao ligar Safari.
    if atual.is_some_and(|sp| top_catch::elegivel(sp) && !capturadas.contains(&sp.id)) {
        return vec![];
    }
    let Some(limite) = conta
        .ultimo
        .get("ativo")
        .and_then(|a| num(a.get("lv")))
        .filter(|n| n.is_finite() && *n > 0.0)
    else {
        return vec![];
    };
    let Some(hunts) = conta.welcome.get("hunts").and_then(Value::as_array) else {
        return vec![];
    };
    let slug_atual = slug_de(conta, &nome, nivel);
    // Uma entrada por espécie evita favorecer espécies que têm mais de uma hunt.
    let mut vistas = HashSet::new();
    let destinos: Vec<(&Value, i64)> = hunts
        .iter()
        .filter_map(|h| {
            let nome_h = h["nome"].as_str()?;
            let slug = h["slug"].as_str().filter(|s| !s.is_empty())?;
            let sp = dex.por_nome(nome_h)?;
            let h_nivel = num(h.get("nivel"))?;
            (top_catch::elegivel(sp)
                && !capturadas.contains(&sp.id)
                && atual.is_none_or(|a| a.id != sp.id)
                && slug_atual.as_deref() != Some(slug)
                && h["area"].as_str() != Some("outland")
                && h_nivel > 0.0
                && h_nivel <= limite
                && vistas.insert(sp.id))
            .then_some((h, sp.id))
        })
        .collect();
    if destinos.is_empty() {
        return aviso(laco, r, nick, "safariSemAlvos", "Safari: nenhuma espécie nova elegível disponível até o nível do Pokémon ativo. Aguardo novos destinos ou o reset semanal.");
    }
    // Dá tempo de a fila confirmar a troca. Se falhar, permite nova tentativa após 60 s.
    let ultima = laco.marca(nick, "safariTrocaEm");
    if ultima > 0.0 && r.agora_ms as f64 - ultima < 60_000.0 {
        return vec![];
    }
    let sorteio = RandomState::new().hash_one((nick, r.agora_ms, capturadas.len()));
    let (h, _) = destinos[(sorteio % destinos.len() as u64) as usize];
    laco.marcar(nick, "safariTrocaEm", r.agora_ms as f64);
    vec![Decisao::TrocarHunt {
        nick: nick.clone(), slug: h["slug"].as_str().unwrap_or_default().into(),
        nome: h["nome"].as_str().unwrap_or_default().into(), nivel: h["nivel"].clone(),
        registro: Some(Registro { tipo: "hunt".into(),
            texto_ok: format!("Safari Top Catch: {} já registrada ou fora do evento; seguindo para {} (espécie que falta na semana).", nome, h["nome"].as_str().unwrap_or_default()),
            texto_falha: "Safari: falha ao trocar hunt: {r}".into(), custo: None }),
    }]
}

#[cfg(test)]
mod testes {
    use super::*;
    use serde_json::json;
    const AGORA: i64 = 1_800_000_000_000;
    fn dex() -> Result<Pokedex, crate::combate::especie::ErroPokedex> {
        Pokedex::de_valor(json!([
            {"d":63,"n":"Abra","t":["PSYCHIC"],"s":[25,20,15,105,55,90],"h":5,"c":"pokebola"},
            {"d":16,"n":"Pidgey","t":["NORMAL"],"s":[40,45,40,35,35,56],"h":10,"c":"pokebola"},
            {"d":19,"n":"Rattata","t":["NORMAL"],"s":[30,56,35,25,35,72],"h":20,"c":"pokebola"},
            {"d":25,"n":"Pikachu","t":["ELECTRIC"],"s":[35,55,40,50,50,90],"h":100,"c":"pokebola"},
            {"d":151,"n":"Mew","t":["PSYCHIC"],"s":[100,100,100,100,100,100],"h":10,"c":"arena_mistica"},
            {"d":2001,"n":"Brave Blastoise","t":["WATER"],"s":[100,100,100,100,100,100],"h":10,"c":"pokebola"}
        ]))
    }
    fn conta(ids: &[i64]) -> ContaViva {
        ContaViva {
            nick: "Ana".into(),
            top_catch: Some(
                json!({"inicio":top_catch::inicio_semana(AGORA),"catalogo_disponivel":true,"capturadas_ids":ids}),
            ),
            ultimo: json!({"hunt":"Abra · nível 5","trLv":100,"ativo":{"lv":"Nv 20"}}),
            welcome: json!({"hunts":[
                {"slug":"abra","nome":"Abra","nivel":5},
                {"slug":"pidgey","nome":"Pidgey","nivel":10},
                {"slug":"rattata","nome":"Rattata","nivel":20},
                {"slug":"pikachu","nome":"Pikachu","nivel":100},
                {"slug":"mew","nome":"Mew","nivel":10},
                {"slug":"brave","nome":"Brave Blastoise","nivel":10,"area":"outland"}]}),
            ..Default::default()
        }
    }
    fn config() -> Value {
        json!({"config":{"Ana":{"safari":{"ativo":true}}}})
    }
    fn destino(d: &[Decisao]) -> Option<&str> {
        d.iter().find_map(|d| match d {
            Decisao::TrocarHunt { slug, .. } => Some(slug.as_str()),
            _ => None,
        })
    }
    #[test]
    fn so_sorteia_especies_novas_elegiveis_ate_nivel_do_ativo(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let dex = dex()?;
        let mut c = conta(&[16]);
        let mut l = EstadoLaco::default();
        let r = Relogio {
            agora_ms: AGORA,
            fuso_ms: 0,
        };
        assert!(motor(&config(), &mut l, &r, &c, &dex).is_empty());
        if let Some(tc) = c.top_catch.as_mut() {
            tc["capturadas_ids"] = json!([63, 16]);
        }
        assert_eq!(
            destino(&motor(&config(), &mut l, &r, &c, &dex)),
            Some("rattata")
        );
        assert!(motor(&config(), &mut l, &r, &c, &dex).is_empty());
        c.ultimo["hunt"] = json!("Rattata · nível 20");
        let r = Relogio {
            agora_ms: AGORA + 70_000,
            ..r
        };
        assert!(motor(&config(), &mut l, &r, &c, &dex).is_empty());
        if let Some(tc) = c.top_catch.as_mut() {
            tc["capturadas_ids"] = json!([63, 16, 19]);
        }
        assert!(destino(&motor(&config(), &mut l, &r, &c, &dex)).is_none());
        Ok(())
    }
    #[test]
    fn reinicio_e_reset_domingo_preservam_so_a_semana_correta(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let dex = dex()?;
        let mut c = conta(&[63, 16, 19]);
        let mut l = EstadoLaco::default();
        let r = Relogio {
            agora_ms: AGORA,
            fuso_ms: 0,
        };
        assert!(destino(&motor(&config(), &mut l, &r, &c, &dex)).is_none());
        // Mesmo sem estado do processo, o histórico do banco evita repetir espécies.
        let mut restaurado = EstadoLaco::default();
        assert!(destino(&motor(&config(), &mut restaurado, &r, &c, &dex)).is_none());
        let r = Relogio {
            agora_ms: top_catch::inicio_semana(AGORA) + top_catch::SEMANA,
            ..r
        };
        assert!(destino(&motor(&config(), &mut restaurado, &r, &c, &dex)).is_none());
        c.top_catch =
            Some(json!({"inicio":r.agora_ms,"catalogo_disponivel":true,"capturadas_ids":[63,16]}));
        assert_eq!(
            destino(&motor(&config(), &mut restaurado, &r, &c, &dex)),
            Some("rattata")
        );
        Ok(())
    }
    #[test]
    fn nao_troca_sem_historico_nivel_ou_quando_desligado() -> Result<(), Box<dyn std::error::Error>>
    {
        let dex = dex()?;
        let mut c = conta(&[63]);
        let mut l = EstadoLaco::default();
        let r = Relogio {
            agora_ms: AGORA,
            fuso_ms: 0,
        };
        c.ultimo["ativo"] = Value::Null;
        assert!(destino(&motor(&config(), &mut l, &r, &c, &dex)).is_none());
        c.top_catch = None;
        assert!(destino(&motor(&config(), &mut l, &r, &c, &dex)).is_none());
        assert!(motor(&json!({}), &mut l, &r, &c, &dex).is_empty());
        Ok(())
    }
}
