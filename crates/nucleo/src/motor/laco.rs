//! Um ciclo do laço do motor (`motor()` do legado, a cada 30 s): para cada conta conectada,
//! as regras na mesma ordem do legado. Com `D.pausa`, nada roda.

use serde_json::Value;

use std::collections::HashMap;

use super::regras::{self, compra, equipe, hunt, simples, vender, ContaViva, Decisao, EstadoLaco};
use super::veredito::Especies;
use super::Relogio;
use crate::combate::especie::Pokedex;
use crate::js;

/// Decisão de um ciclo, com a conta e a regra que a tomou.
#[derive(Debug, Clone, PartialEq)]
pub struct DecisaoDoCiclo {
    pub nick: String,
    pub regra: &'static str,
    pub decisao: Decisao,
}

/// O que o ciclo precisa além do estado: a pokédex e o preço de NPC de cada espécie.
pub struct ContextoCiclo<'a> {
    pub dex: &'a Pokedex,
    pub precos_npc: &'a HashMap<i64, f64>,
}

/// Roda um ciclo. `d` é o `D` já carregado (`config::carregar`). Em sombra, o estado do laço
/// de cada conta vem do legado (`ContaViva::campos` e `objetos`) e vale para este ciclo.
pub fn ciclo(
    d: &Value,
    laco: &mut EstadoLaco,
    r: &Relogio,
    contas: &[ContaViva],
    ctx: &ContextoCiclo,
) -> Vec<DecisaoDoCiclo> {
    let mut saida = Vec::new();
    if js::verdadeiro(d.get("pausa")) {
        return saida;
    }
    let esp = Especies::com_precos(ctx.dex, ctx.precos_npc);
    for conta in contas.iter().filter(|c| !c.nick.is_empty()) {
        if !conta.campos.is_empty() {
            laco.adotar_campos(&conta.nick, &conta.campos);
        }
        // O maior ouro visto é do legado (`conta.ouroMax`): parte do valor dele.
        if conta.ouro_max > laco.marca(&conta.nick, "ouroMax") {
            laco.marcar(&conta.nick, "ouroMax", conta.ouro_max);
        }
        let mut st = equipe::EstadoEquipe::do_legado(&conta.objetos);
        let sessao_tomada = js::verdadeiro(conta.ultimo.get("sessaoTomada"));
        let mut de = |regra: &'static str, v: Vec<Decisao>| {
            saida.extend(v.into_iter().map(|decisao| DecisaoDoCiclo {
                nick: conta.nick.clone(),
                regra,
                decisao,
            }));
        };
        // Ordem do legado: motorPocao, motorCompra, motorHunt, motorVender, motorEquipe,
        // motorBola, motorRodizio, motorShinyBolas, motorMedias, motorPedras, motorRadar,
        // motorExtras.
        de(
            "motorPocao",
            compra::motor_pocao_com(d, laco, r, conta, &conta.consumo),
        );
        de(
            "motorCompra",
            compra::motor_compra_com(d, laco, r, conta, &conta.consumo),
        );
        let (plano_compra, _) = compra::plano_compra_com(
            d,
            r,
            conta,
            &conta.consumo,
            laco.marca(&conta.nick, "ouroMax"),
        );
        let k = hunt::constantes_da_conta(conta);
        let ctx_hunt = hunt::ContextoHunt {
            dex: ctx.dex,
            k: &k,
            precos: hunt::Precos {
                pot: super::num_ou_0(plano_compra.get("pocaoPreco")),
                ball: super::num_ou_0(plano_compra.get("bolaPreco")),
            },
            limite_rodizio: equipe::limite_rodizio(d, laco, r, conta, ctx.dex, &st),
            equipe_no_comando: equipe::equipe_no_comando(laco, r, &conta.nick, &st),
            rodizio_volta: st.rodizio_volta.is_some(),
            hunt_desde: conta.hunt_desde as f64,
        };
        // As gravações no `D` que a hunt faria ficam com o legado: em sombra o `D` é o dele.
        de("motorHunt", hunt::motor_hunt(d, laco, r, conta, &ctx_hunt));
        let desejos = equipe::lista_desejos(d, conta, ctx.dex);
        de(
            "motorVender",
            vender::motor_vender(d, laco, r, conta, &esp, Some(&desejos)),
        );
        de(
            "motorEquipe",
            equipe::motor_equipe(d, laco, r, conta, ctx.dex, &mut st, sessao_tomada),
        );
        de("motorBola", simples::bola(d, laco, r, conta));
        de(
            "motorRodizio",
            equipe::motor_rodizio(d, laco, r, conta, ctx.dex, &mut st),
        );
        de("motorShinyBolas", regras::shiny_bolas(d, laco, r, conta));
        de("motorMedias", simples::medias(d, laco, r, conta));
        de(
            "motorPedras",
            vender::motor_pedras(d, laco, r, conta, contas, &esp),
        );
        de("motorRadar", simples::radar(d, laco, r, contas));
        de("motorExtras", simples::extras(d, laco, r, conta));
    }
    saida
}

impl Decisao {
    /// Nome curto do tipo (coluna `decisao` de `nexus.motor_decisao`).
    pub fn tipo(&self) -> &'static str {
        match self {
            Decisao::Alerta { .. } => "alerta",
            Decisao::Comando { .. } => "comando",
            Decisao::Registro { .. } => "registro",
            Decisao::RecarregarPainel { .. } => "recarregarPainel",
            Decisao::TrocarHunt { .. } => "trocarHunt",
        }
    }

    /// Ações que o legado grava em `acoes` quando toma esta decisão: `(tipo, texto)` para o
    /// caso de sucesso e, quando há, o começo do texto de falha (antes de `{r}`).
    pub fn acoes_esperadas(&self) -> Vec<(String, String, Option<String>)> {
        let do_registro = |reg: &Option<regras::Registro>| {
            reg.iter()
                .map(|g| {
                    let falha = g.texto_falha.split("{r}").next().unwrap_or_default();
                    (g.tipo.clone(), g.texto_ok.clone(), Some(falha.to_owned()))
                })
                .collect::<Vec<_>>()
        };
        match self {
            Decisao::Alerta { titulo, texto, .. } => {
                vec![("alerta".into(), format!("{titulo}: {texto}"), None)]
            }
            Decisao::Registro { tipo, texto, .. } => vec![(tipo.clone(), texto.clone(), None)],
            Decisao::Comando { registro, .. } | Decisao::TrocarHunt { registro, .. } => {
                do_registro(registro)
            }
            Decisao::RecarregarPainel { .. } => Vec::new(),
        }
    }
}

/// Comando para a fila da fase 3 (`nexus.comando`) que o app executa, com a flag
/// `NEXUS_ESCRITA_MOTOR`: `(tipo, params)`. Alerta e registro não vão para o jogo.
impl Decisao {
    pub fn comando_da_fila(&self) -> Option<(String, Value)> {
        match self {
            Decisao::Comando { comando, .. } => Some((
                comando
                    .get("t")
                    .and_then(Value::as_str)
                    .map_or_else(|| "motor".to_owned(), |t| format!("motor.{t}")),
                comando.clone(),
            )),
            Decisao::TrocarHunt {
                slug, nome, nivel, ..
            } => Some((
                "motor.hunt.trocar".into(),
                serde_json::json!({ "slug": slug, "nome": nome, "nivel": nivel }),
            )),
            Decisao::RecarregarPainel { .. } => {
                Some(("motor.painel.recarregar".into(), serde_json::json!({})))
            }
            Decisao::Alerta { .. } | Decisao::Registro { .. } => None,
        }
    }
}

/// Ação esperada no legado, gravada junto com a decisão (`dados.esperadas`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AcaoEsperada {
    pub tipo: String,
    pub texto: String,
    /// Começo do texto de falha (o legado acrescenta a resposta do jogo).
    pub falha: Option<String>,
}

impl DecisaoDoCiclo {
    /// JSON da coluna `dados`: a decisão e as ações que o legado gravaria.
    pub fn dados(&self) -> Value {
        let esperadas: Vec<AcaoEsperada> = self
            .decisao
            .acoes_esperadas()
            .into_iter()
            .map(|(tipo, texto, falha)| AcaoEsperada { tipo, texto, falha })
            .collect();
        serde_json::json!({ "decisao": self.decisao, "esperadas": esperadas })
    }
}

/// Resultado do confronto por tipo de ação.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Confronto {
    pub iguais: u64,
    pub so_nexus: u64,
    pub so_legado: u64,
    pub exemplos: Vec<String>,
}

/// Marcas de tempo que o legado e o Nexus põem quando uma regra age (`conta.xxxEm =
/// Date.now()`). Comandos como `market.medias` não deixam ação no `D`; a marca é o rastro
/// que dá para comparar.
pub const MARCAS_DE_TEMPO: [&str; 7] = [
    "mediasEm",
    "bolaReligadaEm",
    "recargaListaEm",
    "equipeEm",
    "pedrasEm",
    "rodizioEm",
    "venderEm",
];

/// Marcas de tempo que o laço do Nexus pôs neste ciclo: `(nick, campo)`. As adotadas do
/// legado são de ciclos anteriores; só as do Nexus valem `agora`.
pub fn marcas_do_ciclo(laco: &EstadoLaco, agora_ms: i64) -> Vec<(String, String)> {
    let agora = agora_ms as f64;
    let mut v: Vec<(String, String)> = laco
        .marcas
        .iter()
        .filter(|((_, campo), valor)| MARCAS_DE_TEMPO.contains(&campo.as_str()) && **valor == agora)
        .map(|((nick, campo), _)| (nick.clone(), campo.clone()))
        .collect();
    v.sort();
    v
}

/// Esperada de uma marca do Nexus (tipo `marca`, texto = nome da marca).
pub fn esperada_da_marca(campo: &str) -> Value {
    serde_json::json!({ "esperadas": [{ "tipo": "marca", "texto": campo, "falha": null }] })
}

/// Marcas de tempo do legado na foto do ciclo (`contas[].antes.estado`), como ações
/// (`t` = a própria marca) para a tabela das ações do legado. A chave primária descarta a
/// repetição de um ciclo para o outro.
pub fn marcas_do_legado(janela: &Value) -> Vec<Value> {
    let Some(contas) = janela.get("contas").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut v = Vec::new();
    for c in contas {
        let (Some(nick), Some(estado)) = (
            c.get("nick").and_then(Value::as_str),
            c.pointer("/antes/estado").and_then(Value::as_object),
        ) else {
            continue;
        };
        for campo in MARCAS_DE_TEMPO {
            if let Some(t) = estado
                .get(campo)
                .and_then(Value::as_f64)
                .filter(|t| *t > 0.0)
            {
                v.push(serde_json::json!({
                    "t": t as i64, "nick": nick, "tipo": "marca", "texto": campo, "ok": true
                }));
            }
        }
    }
    v
}

/// Confronta as decisões do Nexus `(ciclo, nick, regra, dados)` com as ações do legado
/// `(t, nick, tipo, texto)` no mesmo período. Uma decisão bate com uma ação da mesma conta e
/// tipo, com o mesmo texto (ou o começo do de falha), a até `tolerancia_ms`; cada ação vale
/// uma vez. Só entram os tipos que o Nexus já decide: as ações das regras ainda não portadas
/// não contam como falta.
pub fn confrontar(
    decisoes: &[(i64, String, String, Value)],
    acoes: &[(i64, String, String, String)],
    tolerancia_ms: i64,
) -> std::collections::BTreeMap<String, Confronto> {
    use std::collections::{BTreeMap, BTreeSet};
    let mut por_tipo: BTreeMap<String, Confronto> = BTreeMap::new();
    let mut usada = vec![false; acoes.len()];
    let mut tipos: BTreeSet<String> = BTreeSet::new();
    let exemplo = |c: &mut Confronto, e: String| {
        if c.exemplos.len() < 5 {
            c.exemplos.push(e);
        }
    };
    for (ciclo, nick, regra, dados) in decisoes {
        let esperadas: Vec<AcaoEsperada> = dados
            .get("esperadas")
            .cloned()
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();
        for e in esperadas {
            tipos.insert(e.tipo.clone());
            let achou = acoes
                .iter()
                .enumerate()
                .position(|(i, (t, n, tipo, texto))| {
                    !usada[i]
                        && n == nick
                        && *tipo == e.tipo
                        && (t - ciclo).abs() <= tolerancia_ms
                        && (*texto == e.texto
                            || e.falha
                                .as_deref()
                                .is_some_and(|f| !f.is_empty() && texto.starts_with(f)))
                });
            let c = por_tipo.entry(e.tipo.clone()).or_default();
            match achou {
                Some(i) => {
                    usada[i] = true;
                    c.iguais += 1;
                }
                None => {
                    c.so_nexus += 1;
                    exemplo(c, format!("só Nexus {nick} {regra} @{ciclo}: {}", e.texto));
                }
            }
        }
    }
    for (i, (t, nick, tipo, texto)) in acoes.iter().enumerate() {
        if !usada[i] && tipos.contains(tipo) {
            let c = por_tipo.entry(tipo.clone()).or_default();
            c.so_legado += 1;
            exemplo(c, format!("só legado {nick} @{t}: {texto}"));
        }
    }
    por_tipo
}

#[cfg(test)]
mod testes {
    use super::*;
    use crate::motor::config;
    use serde_json::json;

    fn ctx_vazio() -> (Pokedex, HashMap<i64, f64>) {
        (Pokedex::default(), HashMap::new())
    }

    #[test]
    fn pausa_nao_roda_nada_e_ordem_por_conta() {
        let (dex, precos) = ctx_vazio();
        let ctx = ContextoCiclo {
            dex: &dex,
            precos_npc: &precos,
        };
        let r = Relogio {
            agora_ms: 10_000_000,
            fuso_ms: 0,
        };
        let conta = |n: &str| ContaViva {
            nick: n.into(),
            ultimo: json!({"hdx": {"eu": {"balls": {"1": 5}}}}),
            ..Default::default()
        };
        let cfg = json!({"config": {"A": {"papel": {"tipo": "shiny"}}, "B": {"papel": {"tipo": "shiny"}}}});
        let mut laco = EstadoLaco::default();
        let todas = ciclo(
            &config::carregar(&cfg),
            &mut laco,
            &r,
            &[conta("A"), conta("B")],
            &ctx,
        );
        // Todas as decisões de A antes das de B.
        let nicks: Vec<&str> = todas.iter().map(|x| x.nick.as_str()).collect();
        assert!(nicks.windows(2).all(|w| w[0] <= w[1]), "{nicks:?}");
        let v: Vec<_> = todas
            .iter()
            .filter(|x| x.regra == "motorShinyBolas")
            .collect();
        assert_eq!(
            v.iter().map(|x| x.nick.as_str()).collect::<Vec<_>>(),
            ["A", "B"]
        );
        assert_eq!(
            v[0].decisao.acoes_esperadas(),
            [(
                "alerta".to_owned(),
                "Bolas acabando na conta Shiny: Só 5 bolas. Sem bola boa o shiny que aparecer pode escapar."
                    .to_owned(),
                None
            )]
        );
        let mut pausado = cfg.clone();
        pausado["pausa"] = json!(true);
        let mut laco = EstadoLaco::default();
        assert!(ciclo(
            &config::carregar(&pausado),
            &mut laco,
            &r,
            &[conta("A")],
            &ctx
        )
        .is_empty());
    }

    #[test]
    fn comandos_da_fila() {
        let c = Decisao::Comando {
            nick: "A".into(),
            comando: json!({"t": "auto.set", "autoBallSemParar": true}),
            registro: None,
        };
        assert_eq!(
            c.comando_da_fila(),
            Some((
                "motor.auto.set".into(),
                json!({"t": "auto.set", "autoBallSemParar": true})
            ))
        );
        let a = Decisao::Alerta {
            nick: "A".into(),
            chave: "x".into(),
            titulo: "t".into(),
            texto: "x".into(),
            minutos: 1,
        };
        assert_eq!(a.comando_da_fila(), None);
    }

    #[test]
    fn confronto_casa_por_texto_e_janela() {
        let dec = |c: i64, texto: &str| {
            (
                c,
                "A".to_owned(),
                "motorBola".to_owned(),
                json!({"esperadas": [
                    {"tipo": "bola", "texto": texto, "falha": "Não consegui religar a bola automática ("}
                ]}),
            )
        };
        let decisoes = [
            dec(1_000, "religuei"),
            dec(500_000, "religuei"),
            dec(900_000, "religuei"),
        ];
        let acoes = [
            (
                40_000,
                "A".to_owned(),
                "bola".to_owned(),
                "religuei".to_owned(),
            ),
            (
                510_000,
                "A".to_owned(),
                "bola".to_owned(),
                "Não consegui religar a bola automática (sem-coletor).".to_owned(),
            ),
            (
                700_000,
                "A".to_owned(),
                "bola".to_owned(),
                "religuei".to_owned(),
            ),
            (
                5,
                "A".to_owned(),
                "hunt".to_owned(),
                "fora do escopo".to_owned(),
            ),
        ];
        let r = confrontar(&decisoes, &acoes, 90_000);
        let b = &r["bola"];
        assert_eq!((b.iguais, b.so_nexus, b.so_legado), (2, 1, 1));
        assert!(
            !r.contains_key("hunt"),
            "tipo que o Nexus não decide não conta"
        );
    }

    #[test]
    fn marcas_de_tempo_dos_dois_lados_se_confrontam() {
        let mut laco = EstadoLaco::default();
        laco.marcar("Ana", "mediasEm", 1_000.0); // do Nexus, neste ciclo
        laco.marcar("Ana", "pedrasEm", 500.0); // adotada do legado, ciclo velho
        laco.marcar("Ana", "ouroMax", 1_000.0); // não é marca de tempo
        let m = marcas_do_ciclo(&laco, 1_000);
        assert_eq!(m, vec![("Ana".to_owned(), "mediasEm".to_owned())]);

        let janela = json!({"contas": [
            {"nick": "Ana", "antes": {"estado": {"mediasEm": 1_004, "pedrasEm": 500, "venderEm": 0}}},
            {"nick": "Bia"}
        ]});
        let legado = marcas_do_legado(&janela);
        assert_eq!(legado.len(), 2);

        let decisoes = vec![(
            1_000,
            "Ana".to_owned(),
            "mediasEm".to_owned(),
            esperada_da_marca("mediasEm"),
        )];
        let acoes: Vec<(i64, String, String, String)> = legado
            .iter()
            .map(|a| {
                (
                    a["t"].as_i64().unwrap_or(0),
                    "Ana".to_owned(),
                    "marca".to_owned(),
                    a["texto"].as_str().unwrap_or("").to_owned(),
                )
            })
            .filter(|a| a.0 >= 1_000)
            .collect();
        let r = confrontar(&decisoes, &acoes, 120_000);
        assert_eq!(r["marca"].iguais, 1);
        assert_eq!(r["marca"].so_nexus + r["marca"].so_legado, 0);
    }
}
