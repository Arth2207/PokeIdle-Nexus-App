//! Uma função por rota de leitura: snapshot + parâmetros → resposta do contrato.
//!
//! Os handlers HTTP e o comparador de paridade chamam estas mesmas funções, então o que é
//! comparado com o legado é exatamente o que a API responde.

use std::collections::BTreeMap;

use contrato::bolsa::{self as dto, Params};
use nucleo::bolsa::estat::Liquidez;
use nucleo::bolsa::mercado::{Mercado, Ticker, OFERTA_FRESCA_MS, OFERTA_FRESCA_POKEMON_MS};
use nucleo::bolsa::ofertas;
use nucleo::bolsa::repo::{ErroRepo, FiltroOfertas, OfertaBruta, RepoMercado};
use nucleo::bolsa::sinal::{Acao, Sinal};
use nucleo::bolsa::ticker::{ChaveItem, TickerId};
use nucleo::bolsa::JANELA_DIAS;

use super::agora_ms;
use super::servico::{Feedback, ServicoBolsa, Snapshot};
use crate::conversao;

/// `+texto` do JS: vazio vale 0, texto inválido vale NaN.
pub fn numero(texto: Option<&str>) -> f64 {
    match texto.map(str::trim) {
        None | Some("") => 0.0,
        Some(t) => t.parse().unwrap_or(f64::NAN),
    }
}

/// `x || padrao` para números (0 e NaN contam como falso).
fn ou(x: f64, padrao: f64) -> f64 {
    if x == 0.0 || x.is_nan() {
        padrao
    } else {
        x
    }
}

/// `Array.prototype.slice(inicio, fim)` com índices relativos (negativos contam do fim).
fn fatia<T: Clone>(lista: &[T], inicio: f64, fim: f64) -> Vec<T> {
    let n = lista.len() as f64;
    let ajusta = |x: f64| {
        let x = if x.is_nan() { 0.0 } else { x.trunc() };
        if x < 0.0 {
            (n + x).max(0.0)
        } else {
            x.min(n)
        }
    };
    let (i, f) = (ajusta(inicio), ajusta(fim));
    if f <= i {
        return vec![];
    }
    lista[i as usize..f as usize].to_vec()
}

/// Filtro opcional (`tipo`, `moeda`): texto vazio conta como ausente.
fn filtro(texto: &Option<String>) -> Option<&str> {
    texto.as_deref().filter(|t| !t.is_empty())
}

/// Ticker pelo id exato em texto (o legado busca a string, não o valor interpretado).
pub fn ticker<'m>(m: &'m Mercado, id: Option<&str>) -> Option<&'m Ticker> {
    let texto = id?;
    let t = m.ticker(&texto.parse::<TickerId>().ok()?)?;
    (t.texto == texto).then_some(t)
}

fn passa(t: &Ticker, tipo: Option<&str>, moeda: Option<&str>) -> bool {
    tipo.is_none_or(|x| x == if t.pokemon { "pokemon" } else { "item" })
        && moeda.is_none_or(|x| x == t.moeda.como_str())
}

pub fn mercado(s: &Snapshot, p: &Params) -> dto::MercadoResposta {
    let m = &s.mercado;
    let l: Vec<&Ticker> = m
        .tickers
        .iter()
        .filter(|t| passa(t, filtro(&p.tipo), filtro(&p.moeda)))
        .collect();
    let conta = |sinal: Sinal| l.iter().filter(|t| t.sinal.sinal == sinal).count() as u64;
    let mut ordenados = l.clone();
    ordenados.sort_by_key(|t| std::cmp::Reverse(t.a.vol30d));
    dto::MercadoResposta {
        gerado_em: m.gerado_em,
        janela_dias: JANELA_DIAS,
        total: l.len() as u64,
        resumo: dto::ResumoSinais {
            comprar: conta(Sinal::Comprar),
            vender: conta(Sinal::Vender),
            observar: conta(Sinal::Observar),
            manter: conta(Sinal::Manter),
            sem_base: conta(Sinal::SemBase),
            vendas: m.total_vendas as u64,
        },
        tickers: ordenados.into_iter().map(conversao::resumo).collect(),
    }
}

pub fn serie(s: &Snapshot, p: &Params) -> Option<dto::SerieResposta> {
    let t = ticker(&s.mercado, p.id.as_deref())?;
    let pontos = nucleo::bolsa::mercado::serie(&s.mercado, &t.id)?;
    Some(dto::SerieResposta {
        id: t.texto.clone(),
        vendas: pontos.collect(),
    })
}

pub async fn ticker_completo<R: RepoMercado + 'static>(
    servico: &ServicoBolsa<R>,
    s: &Snapshot,
    p: &Params,
) -> Result<Option<dto::TickerResposta>, ErroRepo> {
    let Some(t) = ticker(&s.mercado, p.id.as_deref()) else {
        return Ok(None);
    };
    let recentes = s.mercado.vendas_de(&t.id);
    let recentes: Vec<dto::VendaRecente> = recentes[recentes.len().saturating_sub(30)..]
        .iter()
        .rev()
        .map(|v| dto::VendaRecente {
            at: v.em,
            preco: v.unidade,
            level: v.nivel,
            pve: v.pve,
        })
        .collect();
    // Pokémon pela espécie; item pelo id. Item identificado só pelo nome não tem ofertas
    // (no legado essa consulta falhava com `item_id = NaN`).
    let alvo = match &t.id {
        TickerId::Pokemon { especie, .. } => Some((true, *especie, OFERTA_FRESCA_POKEMON_MS)),
        TickerId::Item {
            item: ChaveItem::Id(id),
            ..
        } => Some((false, *id, OFERTA_FRESCA_MS)),
        TickerId::Item { .. } => None,
    };
    let ofertas = match alvo {
        Some((pokemon, id, frescor)) => {
            servico
                .repo
                .ofertas_do_ticker(pokemon, id, t.moeda.como_str(), agora_ms() - frescor)
                .await?
        }
        None => vec![],
    };
    Ok(Some(dto::TickerResposta {
        resumo: conversao::resumo(t),
        ind: conversao::indicadores(&t.ind),
        vd: conversao::veredito(&t.vd),
        of_stat: t.of_stat.as_ref().map(conversao::estat_ofertas),
        hist: t.hist.as_ref().map(conversao::hist),
        min30: t.a.min30,
        max30: t.a.max30,
        vol7d: t.a.vol7d as u64,
        referencia: t.referencia.as_ref().map(conversao::referencia),
        candles: t
            .a
            .candles
            .iter()
            .map(|c| dto::Candle {
                d: c.d.clone(),
                o: c.o,
                h: c.h,
                l: c.l,
                c: c.c,
                m: c.m,
                v: c.v as u64,
                hb: c.hb,
            })
            .collect(),
        recentes,
        ofertas: ofertas
            .into_iter()
            .map(|o| dto::OfertaTicker {
                id: o.id,
                preco: o.preco.unwrap_or(0) as f64,
                quantidade: o.quantidade,
                nome: o.nome,
            })
            .collect(),
    }))
}

pub fn balao(s: &Snapshot, p: &Params) -> Option<dto::BalaoResposta> {
    let t = ticker(&s.mercado, p.id.as_deref())?;
    let (a, i) = (&t.a, &t.ind);
    Some(dto::BalaoResposta {
        id: t.texto.clone(),
        nome: t.nome.clone(),
        tipo: if t.pokemon { "pokemon" } else { "item" }.into(),
        moeda: t.moeda.como_str().into(),
        sprite: t.sprite.clone(),
        ultimo: a.ultimo,
        ma7: a.ma7,
        ma30: a.ma30,
        min30: a.min30,
        max30: a.max30,
        hist: t.hist.as_ref().map(conversao::hist),
        var24h: a.var24h,
        var7d: a.var7d,
        tendencia: a.tendencia,
        tend_z: a.tend_z,
        liquidez: a.liquidez.como_str().into(),
        vol24h: a.vol24h as u64,
        spd: a.spd,
        dias: a.dias as u64,
        historico: a.historico.como_str().into(),
        normalizado: t.normalizado,
        ask_min: t.ask_min,
        of_stat: t.of_stat.as_ref().map(conversao::estat_ofertas),
        rsi: i.rsi,
        pb: i.boll.as_ref().and_then(|b| b.pb),
        largura: i.boll.as_ref().and_then(|b| b.largura),
        macd: i.macd.as_ref().map(|m| dto::MacdResumo {
            hist: m.hist,
            adaptado: m.adaptado,
        }),
        vol_dia: i.vol,
        mom: i.mom,
        mom_per: i.mom_per,
        vwap: i.vwap,
        drawdown: i.drawdown,
        suporte: i.suporte,
        resistencia: i.resistencia,
        vd: conversao::veredito(&t.vd),
    })
}

pub fn feedback<R: RepoMercado + 'static>(
    servico: &ServicoBolsa<R>,
    s: &Snapshot,
    p: &Params,
) -> dto::FeedbackResposta {
    let m = &s.mercado;
    let limite = ou(numero(p.limite.as_deref()), 60.0).clamp(10.0, 200.0);
    let pagina = ou(numero(p.pagina.as_deref()), 0.0).max(0.0);
    let q = p.q.clone().unwrap_or_default().to_lowercase();
    let (tipo, moeda, acao) = (filtro(&p.tipo), filtro(&p.moeda), filtro(&p.acao));
    let chave = [tipo, moeda, acao, Some(q.as_str())]
        .map(|x| x.unwrap_or(""))
        .join("|");

    let calcular = || {
        let base: Vec<usize> = m
            .tickers
            .iter()
            .enumerate()
            .filter(|(_, t)| {
                passa(t, tipo, moeda) && (q.is_empty() || t.nome.to_lowercase().contains(&q))
            })
            .map(|(i, _)| i)
            .collect();
        let contagem = base.iter().fold(BTreeMap::new(), |mut c, &i| {
            *c.entry(m.tickers[i].vd.acao.como_str().to_owned())
                .or_insert(0u64) += 1;
            c
        });
        let grupo: Option<&[Acao]> = match acao {
            Some("comprar") => Some(&[Acao::Comprar]),
            Some("vender") => Some(&[Acao::Vender]),
            Some("aguardar") => Some(&[Acao::AguardarParaComprar, Acao::AguardarParaVender]),
            _ => None,
        };
        let mut indices: Vec<usize> = base
            .into_iter()
            .filter(|&i| match grupo {
                Some(g) => g.contains(&m.tickers[i].vd.acao),
                None => m.tickers[i].vd.acao != Acao::SemBase,
            })
            .collect();
        let peso = |i: usize| {
            let vd = &m.tickers[i].vd;
            vd.score.abs() * (vd.confianca + 20.0)
        };
        indices.sort_by(|&a, &b| peso(b).total_cmp(&peso(a)));
        std::sync::Arc::new(Feedback {
            gerado_em: m.gerado_em,
            indices,
            contagem,
        })
    };
    let h = servico
        .feedback
        .get(&chave)
        .filter(|h| h.gerado_em == m.gerado_em)
        .unwrap_or_else(|| {
            let h = calcular();
            servico.feedback.insert(chave, std::sync::Arc::clone(&h));
            h
        });
    let pagina_itens = fatia(&h.indices, pagina * limite, pagina * limite + limite);
    dto::FeedbackResposta {
        gerado_em: m.gerado_em,
        total: h.indices.len() as u64,
        contagem: h.contagem.clone(),
        pagina,
        limite,
        itens: pagina_itens
            .into_iter()
            .map(|i| conversao::resumo(&m.tickers[i]))
            .collect(),
    }
}

async fn ofertas_ativas<R: RepoMercado>(
    repo: &R,
    tipo: Option<&str>,
    moeda: Option<&str>,
) -> Result<Vec<OfertaBruta>, ErroRepo> {
    let agora = agora_ms();
    repo.ofertas_ativas(&FiltroOfertas {
        corte_item: agora - OFERTA_FRESCA_MS,
        corte_pokemon: agora - OFERTA_FRESCA_POKEMON_MS,
        tipo: tipo.map(str::to_owned),
        moeda: moeda.map(str::to_owned),
        limite: 6000,
    })
    .await
}

const AVISO_SEM_OFERTAS: &str = "Nenhuma oferta ativa capturada ainda: as dicas de compra usam só o histórico de vendas. Para listar anúncios abaixo do valor justo, o motor precisa enviar as ofertas (POST /api/bolsa/ofertas).";
const DESC_COMPRA: f64 = -0.15;

pub async fn dicas<R: RepoMercado + 'static>(
    servico: &ServicoBolsa<R>,
    s: &Snapshot,
    p: &Params,
) -> Result<dto::DicasResposta, ErroRepo> {
    let m = &s.mercado;
    let (tipo, moeda) = (filtro(&p.tipo), filtro(&p.moeda));
    let topo = |sinal: Sinal, n: usize| {
        let mut l: Vec<&Ticker> = m
            .tickers
            .iter()
            .filter(|t| {
                t.sinal.sinal == sinal && t.a.liquidez != Liquidez::Baixa && passa(t, tipo, moeda)
            })
            .collect();
        l.sort_by(|a, b| b.sinal.score.total_cmp(&a.sinal.score));
        l.into_iter().take(n).map(conversao::resumo).collect()
    };
    let ofertas = ofertas_ativas(&servico.repo, tipo, moeda).await?;
    let oportunidades = if ofertas.is_empty() {
        vec![]
    } else {
        ofertas::avaliar(m, &ofertas, &s.valores)
            .into_iter()
            .filter(|o| o.desconto <= DESC_COMPRA && o.faixa.is_none_or(|f| o.unidade <= f[0]))
            .take(40)
            .map(|o| conversao::oferta(&o))
            .collect()
    };
    Ok(dto::DicasResposta {
        gerado_em: m.gerado_em,
        ofertas_ativas: ofertas.len() as u64,
        aviso: ofertas.is_empty().then(|| AVISO_SEM_OFERTAS.to_owned()),
        comprar: topo(Sinal::Comprar, 15),
        vender: topo(Sinal::Vender, 15),
        oportunidades,
    })
}

pub async fn ofertas<R: RepoMercado + 'static>(
    servico: &ServicoBolsa<R>,
    s: &Snapshot,
    p: &Params,
) -> Result<dto::OfertasResposta, ErroRepo> {
    let (tipo, moeda) = (filtro(&p.tipo), filtro(&p.moeda));
    let limite = ou(numero(p.limite.as_deref()), 200.0).min(500.0);
    let lista = ofertas_ativas(&servico.repo, tipo, moeda).await?;
    let ultima = lista.iter().map(|o| o.ts_visto).max();
    let avaliadas = ofertas::avaliar(&s.mercado, &lista, &s.valores);
    Ok(dto::OfertasResposta {
        gerado_em: agora_ms(),
        ultima_captura: ultima,
        total: lista.len() as u64,
        avaliadas: avaliadas.len() as u64,
        ofertas: fatia(&avaliadas, 0.0, limite)
            .iter()
            .map(conversao::oferta)
            .collect(),
    })
}

const AVISO_SEM_ITENS: &str = "Nenhum item cadastrado. O inventário ainda não chega do app; adicione itens manualmente na aba Feedback › Meus itens.";

pub fn meus_itens(
    s: &Snapshot,
    p: &Params,
    inventario: &nucleo::bolsa::ingestao::MeusItens,
    raros: &std::collections::HashMap<i32, String>,
) -> dto::MeusItensResposta {
    let m = &s.mercado;
    let moeda = if p.moeda.as_deref() == Some("gold") {
        "gold"
    } else {
        "orb"
    };
    let mut itens: Vec<dto::MeuItem> = inventario
        .itens
        .iter()
        .map(|(&id, &q)| {
            // Como o legado: sem ticker pelo id, procura pelo nome que o painel de raros
            // conhece (itens que o catálogo não tem, como os fragmentos).
            let nr = raros.get(&id);
            let t = ticker(m, Some(&format!("i:{id}:{moeda}"))).or_else(|| {
                let nr = nr?.to_lowercase();
                m.tickers.iter().find(|t| {
                    !t.pokemon && t.moeda.como_str() == moeda && t.nome.to_lowercase() == nr
                })
            });
            let vd = t.map(|t| &t.vd);
            let qf = q as f64;
            let valor = t.map(|t| t.a.ultimo * qf);
            let alvo = vd.and_then(|v| v.preco_alvo_venda);
            dto::MeuItem {
                item_id: id,
                nome: t
                    .map(|t| t.nome.clone())
                    .filter(|n| !n.is_empty())
                    .or_else(|| m.catalogo.item(id).map(|c| c.nome.clone()))
                    .or_else(|| nr.cloned())
                    .unwrap_or_else(|| format!("#{id}")),
                quantidade: q,
                sprite: t
                    .and_then(|t| t.sprite.clone())
                    .or_else(|| m.icone_item(Some(id))),
                id: t.map(|t| t.texto.clone()),
                ultimo: t.map(|t| t.a.ultimo),
                valor,
                acao: vd.map_or("SEM_BASE", |v| v.acao.como_str()).into(),
                vscore: vd.map_or(0.0, |v| v.score),
                vconf: vd.map_or(0.0, |v| v.confianca),
                vprov: vd.is_some_and(|v| v.provisorio),
                alvo_v: alvo,
                valor_alvo: alvo.filter(|a| *a != 0.0).map(|a| a * qf),
                vmot: vd
                    .and_then(|v| v.motivos.first().cloned())
                    .unwrap_or_else(|| "sem histórico de vendas nesta moeda".into()),
            }
        })
        .collect();
    let total = itens
        .iter()
        .filter_map(|i| i.valor)
        .filter(|v| *v != 0.0 && !v.is_nan())
        .fold(0.0, |s, v| s + v);
    itens.sort_by(|a, b| {
        let primeiro = a.vscore - b.vscore;
        let ordem = if primeiro != 0.0 && !primeiro.is_nan() {
            primeiro
        } else {
            b.valor.unwrap_or(0.0) - a.valor.unwrap_or(0.0)
        };
        ordem.total_cmp(&0.0)
    });
    dto::MeusItensResposta {
        moeda: moeda.into(),
        ts: inventario.ts,
        total,
        aviso: itens.is_empty().then(|| AVISO_SEM_ITENS.to_owned()),
        itens,
    }
}

/// Chave em `nexus.parametro` dos nomes de itens raros (do `raros-estado.json` do legado).
pub const CHAVE_RAROS_NOMES: &str = "raros.nomes";

/// Nomes dos itens raros por id. Sem a importação, um mapa vazio.
pub async fn raros_nomes(db: &sqlx::PgPool) -> std::collections::HashMap<i32, String> {
    persistencia::equipes::ler_parametro(db, CHAVE_RAROS_NOMES)
        .await
        .ok()
        .flatten()
        .and_then(|v| {
            v.as_object().map(|m| {
                m.iter()
                    .filter_map(|(k, v)| Some((k.parse().ok()?, v.as_str()?.to_owned())))
                    .collect()
            })
        })
        .unwrap_or_default()
}
