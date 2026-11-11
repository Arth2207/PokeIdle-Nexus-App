//! Fotografia do mercado: vendas da janela normalizadas, tickers, indicadores e vereditos.
//!
//! Porte do `montar()` + `enriquecer()` do legado. Tudo aqui é cálculo puro sobre as
//! [`Fontes`]; quem busca os dados é o [`super::carregador`].

use std::collections::HashMap;
use std::sync::Arc;

use super::catalogo::Catalogo;
use super::estat::{self, Analise, Historico};
use super::indicadores::{indicadores, Indicadores};
use super::repo::{Agregado, Agregados};
use super::sinal::{self, ContextoSinal, EstatOfertas, ResultadoSinal, Veredito};
use super::ticker::{ChaveItem, Moeda, TickerId};
use super::venda::{nome_sem_quantidade, VendaBruta};
use super::{DIA_MS, JANELA_DIAS, SERIE_MAX_PONTOS};
use crate::js;
use crate::precificador::cotacao::{Cotacoes, PrecoItemDia};
use crate::precificador::especie::Central;
use crate::precificador::ficha::Ficha;
use crate::precificador::motor::{self, LinhaMotor, Referencia, VendaFicha};
use crate::precificador::vizinhos::{self, Indice};

/// Tudo o que a montagem precisa. Os campos `Arc` mudam pouco e são compartilhados entre
/// montagens sem copiar.
#[derive(Debug, Clone, Default)]
pub struct Fontes {
    /// Vendas da janela, em ordem de `em` e `id`.
    pub vendas: Vec<VendaBruta>,
    pub base: Arc<Vec<LinhaMotor>>,
    pub itens_base: Arc<Vec<PrecoItemDia>>,
    pub central: Arc<Central>,
    pub catalogo: Arc<Catalogo>,
    pub nomes_dim_item: Arc<HashMap<i32, String>>,
    pub pesos_confianca: Option<[f64; 5]>,
    pub vies: Option<f64>,
    pub agregados: Agregados,
}

/// Estatística histórica de vendas do ticker.
#[derive(Debug, Clone, PartialEq)]
pub struct Hist {
    pub n: i64,
    pub min: f64,
    pub max: f64,
    pub p05: f64,
    pub p95: f64,
    pub desde: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Ticker {
    pub id: TickerId,
    pub texto: String,
    pub pokemon: bool,
    pub moeda: Moeda,
    pub nome: String,
    pub especie: Option<i32>,
    pub item_id: Option<i32>,
    pub shiny: bool,
    pub normalizado: bool,
    pub referencia: Option<Referencia>,
    pub a: Analise,
    pub sinal: ResultadoSinal,
    pub sprite: Option<String>,
    pub ask_min: Option<f64>,
    pub ask_anuncios: Option<i64>,
    pub ind: Indicadores,
    pub of_stat: Option<EstatOfertas>,
    pub hist: Option<Hist>,
    pub vd: Veredito,
}

/// Uma venda depois da normalização.
#[derive(Debug, Clone, PartialEq)]
pub struct VendaT {
    pub em: i64,
    pub unidade: f64,
    pub nivel: Option<f64>,
    pub pve: Option<f64>,
}

/// Informação da venda usada para identificar o ticker (o legado lê da primeira venda).
#[derive(Debug, Clone)]
struct Origem {
    id: TickerId,
    nome: Option<String>,
    especie: Option<i32>,
    shiny: bool,
    normalizado: bool,
    referencia: Option<Referencia>,
}

#[derive(Debug, Default)]
pub struct Mercado {
    pub gerado_em: i64,
    /// Total de vendas (normalizadas) na janela.
    pub total_vendas: usize,
    pub tickers: Vec<Ticker>,
    por_id: HashMap<TickerId, usize>,
    /// Vendas de cada grupo, em ordem de tempo.
    vendas_por_grupo: HashMap<TickerId, Vec<VendaT>>,
    pub central: Arc<Central>,
    pub catalogo: Arc<Catalogo>,
    pub cotacoes: Cotacoes,
    /// Menor anúncio atual em orbs, por item.
    pub itens_atuais: HashMap<i32, f64>,
    pub indices: [Option<Indice>; 2],
    pub pesos_confianca: [f64; 5],
    pub vies: f64,
}

/// ((dia, item), preços unitários das vendas daquele dia).
type PrecosDoDia = ((i64, i32), Vec<f64>);

/// Frescor das ofertas: itens são relidos a cada ciclo; pokémon em rodadas mais longas.
pub const OFERTA_FRESCA_MS: i64 = 45 * 60_000;
pub const OFERTA_FRESCA_POKEMON_MS: i64 = 90 * 60_000;
pub const RESUMO_FRESCO_MS: i64 = 3 * 3_600_000;
/// Mínimo de vendas em coins normalizadas para trocar as brutas pelas normalizadas.
const MIN_NORMALIZADAS_GOLD: usize = 30;

impl Mercado {
    pub fn ticker(&self, id: &TickerId) -> Option<&Ticker> {
        self.por_id.get(id).map(|&i| &self.tickers[i])
    }

    /// Vendas (normalizadas) de um ticker, em ordem de tempo.
    pub fn vendas_de(&self, id: &TickerId) -> &[VendaT] {
        self.vendas_por_grupo.get(id).map_or(&[], Vec::as_slice)
    }

    pub fn total_tickers(&self) -> usize {
        self.tickers.len()
    }

    /// Avaliador de fichas (valor justo) com os dados deste mercado.
    pub fn avaliador(&self) -> vizinhos::Avaliador<'_> {
        vizinhos::Avaliador {
            central: &self.central,
            cotacoes: &self.cotacoes,
            itens_atuais: &self.itens_atuais,
            indices: &self.indices,
            pesos: self.pesos_confianca,
            vies: self.vies,
        }
    }

    /// `spriteEspecie`: arte pública da espécie pelo número da dex.
    pub fn sprite_especie(&self, especie: Option<i32>, shiny: bool) -> Option<String> {
        sprite_de(&self.central, especie, shiny)
    }

    /// Ícone de item do catálogo.
    pub fn icone_item(&self, item_id: Option<i32>) -> Option<String> {
        self.catalogo.icone(item_id?)
    }

    /// Monta o mercado no instante `agora`.
    pub fn montar(fontes: &Fontes, agora: i64) -> Self {
        let desde = agora - JANELA_DIAS * DIA_MS;
        let linhas: Vec<&VendaBruta> = fontes
            .vendas
            .iter()
            .filter(|r| r.em >= desde && r.em <= agora)
            .collect();
        let catalogo = &fontes.catalogo;
        let central = &fontes.central;

        // Corte: a base antiga só vale antes da primeira venda capturada com ficha.
        let corte = linhas
            .iter()
            .filter(|r| r.tipo == "pokemon" && r.ficha.is_some())
            .map(|r| r.em)
            .min()
            .unwrap_or(agora);

        // Vendas brutas: base antiga (pokémon em orbs) + linhas da janela.
        let mut brutas: Vec<(VendaT, Origem)> = fontes
            .base
            .iter()
            .filter(|v| v.em >= desde && v.em < corte)
            .map(|v| {
                (
                    VendaT {
                        em: v.em,
                        unidade: v.preco,
                        nivel: Some(v.nivel),
                        pve: Some(v.pve),
                    },
                    Origem {
                        id: TickerId::Pokemon {
                            especie: v.especie,
                            shiny: v.shiny,
                            moeda: Moeda::Orb,
                        },
                        nome: v.nome.clone(),
                        especie: Some(v.especie),
                        shiny: v.shiny,
                        normalizado: false,
                        referencia: None,
                    },
                )
            })
            .collect();
        // Cotação diária (mediana) dos itens vendidos, por moeda.
        let mut itens_dia: HashMap<Moeda, Vec<PrecosDoDia>> = HashMap::new();
        linhas.iter().for_each(|r| {
            if let Some(v) = classificar(r, catalogo, &fontes.nomes_dim_item) {
                if let (TickerId::Item { moeda, .. }, Some(item_id)) =
                    (&v.1.id, r.item_id.filter(|&i| i != 0))
                {
                    let lista = itens_dia.entry(*moeda).or_default();
                    let chave = (js::dia_sp(r.em), item_id);
                    match lista.iter_mut().find(|(k, _)| *k == chave) {
                        Some((_, l)) => l.push(v.0.unidade),
                        None => lista.push((chave, vec![v.0.unidade])),
                    }
                }
                brutas.push(v);
            }
        });
        let mediana_dia = |moeda: Moeda| -> Vec<PrecoItemDia> {
            itens_dia
                .get(&moeda)
                .map(|l| {
                    l.iter()
                        .map(|((dia, item_id), precos)| PrecoItemDia {
                            dia: *dia,
                            item_id: *item_id,
                            preco: js::mediana_meio(precos.iter().copied()).unwrap_or(f64::NAN),
                        })
                        .collect()
                })
                .unwrap_or_default()
        };
        let dia_orb = mediana_dia(Moeda::Orb);
        let itens_preco: Vec<PrecoItemDia> = fontes
            .itens_base
            .iter()
            .filter(|i| {
                !dia_orb
                    .iter()
                    .any(|d| d.dia == i.dia && d.item_id == i.item_id)
            })
            .copied()
            .chain(dia_orb.iter().copied())
            .collect();
        let cotacoes = Cotacoes::novo(itens_preco.iter().copied());
        // Em coins não há base: a cotação de coins é usada como se fosse a única moeda.
        let cotacoes_gold = Cotacoes::novo(mediana_dia(Moeda::Gold));

        // Normalização dos pokémon pelo precificador (se o catálogo de espécies existir).
        let fichas = |moeda: &str| -> Vec<VendaFicha> {
            linhas
                .iter()
                .filter(|r| r.tipo == "pokemon" && r.moeda == moeda)
                .filter_map(|r| {
                    let f = r.ficha.as_ref()?;
                    let ficha = Ficha::de_json(f);
                    Some(VendaFicha {
                        em: r.em,
                        lido_em: r.lido_em.filter(|l| *l != 0).unwrap_or(r.em),
                        preco: r.preco.unwrap_or(0) as f64,
                        especie: r.especie.or(ficha.especie),
                        ficha,
                    })
                })
                .collect()
        };
        let (vendas_t, linhas_motor) = if central.is_empty() {
            (brutas, Vec::new())
        } else {
            let novas = motor::preparar(central, &fichas("orb"), &cotacoes);
            let linhas_motor: Vec<LinhaMotor> = fontes
                .base
                .iter()
                .filter(|v| v.em >= desde && v.em < corte && v.base > 0.0)
                .cloned()
                .chain(novas)
                .collect();
            let norm_gold =
                motor::normalizar(&motor::preparar(central, &fichas("gold"), &cotacoes_gold));
            let usar_gold = norm_gold.len() >= MIN_NORMALIZADAS_GOLD;
            let para_venda = |moeda: Moeda| {
                move |n: motor::VendaNormalizada| {
                    (
                        VendaT {
                            em: n.em,
                            unidade: n.unidade,
                            nivel: Some(n.nivel),
                            pve: Some(n.pve),
                        },
                        Origem {
                            id: TickerId::Pokemon {
                                especie: n.especie,
                                shiny: n.shiny,
                                moeda,
                            },
                            nome: n.nome,
                            especie: Some(n.especie),
                            shiny: n.shiny,
                            normalizado: true,
                            referencia: n.referencia,
                        },
                    )
                }
            };
            let mut vendas_t: Vec<(VendaT, Origem)> = brutas
                .into_iter()
                .filter(|(_, o)| match o.id {
                    TickerId::Pokemon { moeda, .. } => {
                        !(moeda == Moeda::Orb || (usar_gold && moeda == Moeda::Gold))
                    }
                    TickerId::Item { .. } => true,
                })
                .chain(
                    motor::normalizar(&linhas_motor)
                        .into_iter()
                        .map(para_venda(Moeda::Orb)),
                )
                .chain(
                    norm_gold
                        .into_iter()
                        .filter(|_| usar_gold)
                        .map(para_venda(Moeda::Gold)),
                )
                .collect();
            vendas_t.sort_by_key(|(v, _)| v.em);
            (vendas_t, linhas_motor)
        };
        let total_vendas = vendas_t.len();

        // Agrupa por ticker, na ordem da primeira venda (como um Map do JS).
        let (ordem, mut grupos) = vendas_t.into_iter().fold(
            (
                Vec::<Origem>::new(),
                HashMap::<TickerId, Vec<VendaT>>::new(),
            ),
            |(mut ordem, mut grupos), (venda, origem)| {
                let lista = grupos.entry(origem.id.clone()).or_insert_with(|| {
                    ordem.push(origem);
                    Vec::new()
                });
                lista.push(venda);
                (ordem, grupos)
            },
        );
        grupos.values_mut().for_each(|l| l.sort_by_key(|v| v.em));

        let tickers: Vec<Ticker> = ordem
            .into_iter()
            .filter_map(|o| {
                let pontos: Vec<estat::Venda> = grupos
                    .get(&o.id)?
                    .iter()
                    .map(|v| estat::Venda {
                        em: v.em,
                        unidade: v.unidade,
                    })
                    .collect();
                let a = estat::analisar(&pontos, agora)?;
                Some(novo_ticker(o, a))
            })
            .collect();

        let mut mercado = Mercado {
            gerado_em: agora,
            total_vendas,
            por_id: tickers
                .iter()
                .enumerate()
                .map(|(i, t)| (t.id.clone(), i))
                .collect(),
            tickers,
            vendas_por_grupo: grupos,
            central: Arc::clone(central),
            catalogo: Arc::clone(catalogo),
            cotacoes,
            itens_atuais: fontes
                .agregados
                .asks
                .iter()
                .filter(|a| a.moeda == "orb")
                .map(|a| (a.item_id, a.preco_min))
                .collect(),
            indices: [None, None],
            pesos_confianca: fontes.pesos_confianca.unwrap_or(vizinhos::PESOS_CONFIANCA),
            vies: fontes
                .vies
                .filter(|v| v.is_finite())
                .unwrap_or_else(vizinhos::vies_padrao),
        };
        mercado.indices = [
            Indice::novo(&linhas_motor, false),
            Indice::novo(&linhas_motor, true),
        ];
        mercado.enriquecer(&fontes.agregados);
        mercado
    }

    /// Sprites, menor anúncio, indicadores, estatísticas de ofertas e veredito.
    fn enriquecer(&mut self, ag: &Agregados) {
        let asks: HashMap<String, (f64, Option<i64>)> = ag
            .asks
            .iter()
            .map(|a| {
                (
                    format!("i:{}:{}", a.item_id, a.moeda),
                    (a.preco_min, a.anuncios),
                )
            })
            .collect();
        let of_stat: HashMap<String, EstatOfertas> = ag
            .ofertas_itens
            .iter()
            .map(|r| (format!("i:{}:{}", r.chave, r.moeda), &r.agregado))
            .chain(ag.ofertas_pokemon.iter().map(|r| {
                (
                    format!("p:{}:{}:{}", r.especie, u8::from(r.shiny), r.moeda),
                    &r.agregado,
                )
            }))
            .map(|(k, a)| {
                let q = |i: usize| a.q.get(i).copied().unwrap_or(f64::NAN);
                (
                    k,
                    EstatOfertas {
                        n: a.n,
                        min: a.min,
                        p25: q(0),
                        med: q(1),
                        p75: q(2),
                        max: a.max,
                    },
                )
            })
            .collect();
        let hist = historico(ag, &self.catalogo);

        let catalogo = Arc::clone(&self.catalogo);
        let central = Arc::clone(&self.central);
        self.tickers.iter_mut().for_each(|t| {
            if t.pokemon {
                t.sprite = sprite_de(&central, t.especie, t.shiny);
            } else {
                t.sprite = t.item_id.and_then(|i| catalogo.icone(i));
                if let Some((min, anuncios)) = asks.get(&t.texto) {
                    if t.a.ma7 != 0.0 && t.a.historico == Historico::Ok {
                        t.ask_min = Some(*min);
                        t.ask_anuncios = *anuncios;
                        t.sinal = sinal::sinal_de(&t.a, &contexto(t));
                    }
                }
            }
            t.ind = indicadores(&t.a);
            t.of_stat = of_stat.get(&t.texto).cloned();
            t.hist = hist.get(&t.texto).cloned();
            if t.ask_min.is_none() && !t.pokemon {
                t.ask_min = t.of_stat.as_ref().map(|o| o.min);
            }
            t.vd = sinal::veredito(
                &t.a,
                &t.ind,
                t.pokemon,
                t.normalizado,
                t.of_stat.as_ref(),
                t.ask_min,
            );
        });
    }
}

fn contexto(t: &Ticker) -> ContextoSinal {
    ContextoSinal {
        pokemon: t.pokemon,
        orb: t.moeda == Moeda::Orb,
        normalizado: t.normalizado,
        ask_min: t.ask_min,
        ask_anuncios: t.ask_anuncios,
    }
}

fn novo_ticker(o: Origem, a: Analise) -> Ticker {
    let (pokemon, moeda, item_id) = match &o.id {
        TickerId::Pokemon { moeda, .. } => (true, *moeda, None),
        TickerId::Item {
            item: ChaveItem::Id(id),
            moeda,
        } => (false, *moeda, Some(*id)),
        TickerId::Item { moeda, .. } => (false, *moeda, None),
    };
    // `first.name + ' ✨'`: nome ausente vira "undefined" no JS.
    let base = o.nome.clone().unwrap_or_else(|| "undefined".into());
    let nome = if pokemon && o.shiny {
        format!("{base} ✨")
    } else {
        base
    };
    let mut t = Ticker {
        texto: o.id.to_string(),
        id: o.id,
        pokemon,
        moeda,
        nome,
        especie: if pokemon { o.especie } else { None },
        item_id,
        shiny: o.shiny,
        normalizado: o.normalizado,
        referencia: o.referencia,
        ind: indicadores(&a),
        a,
        sinal: ResultadoSinal {
            sinal: sinal::Sinal::SemBase,
            score: 0.0,
            motivo: String::new(),
        },
        sprite: None,
        ask_min: None,
        ask_anuncios: None,
        of_stat: None,
        hist: None,
        vd: Veredito::sem_base(""),
    };
    t.sinal = sinal::sinal_de(&t.a, &contexto(&t));
    t
}

/// Classifica uma venda bruta da janela. Pokémon sem espécie fica de fora (no legado
/// virava o ticker `p:null:...`).
fn classificar(
    r: &VendaBruta,
    catalogo: &Catalogo,
    nomes_dim_item: &HashMap<i32, String>,
) -> Option<(VendaT, Origem)> {
    let moeda: Moeda = r.moeda.parse().ok()?;
    let preco = r.preco.unwrap_or(0) as f64;
    if r.tipo == "pokemon" {
        let ficha = r.ficha.as_ref().map(Ficha::de_json).unwrap_or_default();
        let especie = r.especie.or(ficha.especie)?;
        let nome = ficha
            .nome
            .clone()
            .filter(|n| !n.is_empty())
            .or_else(|| r.descricao.as_deref().map(|d| sem_nivel(d).to_owned()));
        let shiny = ficha.shiny;
        return Some((
            VendaT {
                em: r.em,
                unidade: preco,
                nivel: ficha.nivel,
                pve: None,
            },
            Origem {
                id: TickerId::Pokemon {
                    especie,
                    shiny,
                    moeda,
                },
                nome: Some(nome.unwrap_or_default()),
                especie: Some(especie),
                shiny,
                normalizado: false,
                referencia: None,
            },
        ));
    }
    let quantidade = r.quantidade.unwrap_or(1).max(1);
    let descricao = r.descricao.as_deref().unwrap_or("");
    let nome = nome_sem_quantidade(descricao).unwrap_or(descricao).trim();
    let minusculo = nome.to_lowercase();
    let cid = r
        .item_id
        .filter(|&i| i != 0)
        .or_else(|| catalogo.id_por_nome(&minusculo));
    let item = cid.map_or(ChaveItem::Nome(minusculo), ChaveItem::Id);
    let nome_final = cid
        .and_then(|c| catalogo.item(c).map(|i| i.nome.clone()))
        .filter(|n| !n.is_empty())
        .or_else(|| {
            r.item_id
                .and_then(|i| nomes_dim_item.get(&i).cloned())
                .filter(|n| !n.is_empty())
        })
        .unwrap_or_else(|| nome.to_owned());
    Some((
        VendaT {
            em: r.em,
            unidade: preco / f64::from(quantidade),
            nivel: None,
            pve: None,
        },
        Origem {
            id: TickerId::Item { item, moeda },
            nome: Some(nome_final),
            especie: None,
            shiny: false,
            normalizado: false,
            referencia: None,
        },
    ))
}

/// `descricao.replace(/ Nv \d+.*$/, '')`.
fn sem_nivel(descricao: &str) -> &str {
    descricao
        .match_indices(" Nv ")
        .find(|(i, _)| {
            descricao[i + 4..]
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_digit())
        })
        .map_or(descricao, |(i, _)| &descricao[..i])
}

/// Histórico agregado por ticker. Itens identificados por nome são resolvidos pelo
/// catálogo e somados ao id (n, mín. e máx.; quantis e "desde" ficam os do primeiro grupo).
fn historico(ag: &Agregados, catalogo: &Catalogo) -> HashMap<String, Hist> {
    let para_hist = |a: &Agregado| Hist {
        n: a.n,
        min: a.min,
        max: a.max,
        p05: a.q.first().copied().unwrap_or(f64::NAN),
        p95: a.q.get(1).copied().unwrap_or(f64::NAN),
        desde: a.desde.unwrap_or(0),
    };
    let mut mapa: HashMap<String, Hist> = HashMap::new();
    ag.hist_itens.iter().for_each(|r| {
        let numerico = !r.chave.is_empty() && r.chave.bytes().all(|b| b.is_ascii_digit());
        let cid = if numerico {
            r.chave.parse::<i32>().ok()
        } else {
            catalogo.id_por_nome(&r.chave)
        };
        let k = match cid.filter(|&c| c != 0) {
            Some(c) => format!("i:{c}:{}", r.moeda),
            None => format!("i:n:{}:{}", r.chave, r.moeda),
        };
        match mapa.get_mut(&k) {
            None => {
                mapa.insert(k, para_hist(&r.agregado));
            }
            Some(o) => {
                o.n += r.agregado.n;
                o.min = o.min.min(r.agregado.min);
                o.max = o.max.max(r.agregado.max);
            }
        }
    });
    ag.hist_pokemon.iter().for_each(|r| {
        mapa.insert(
            format!("p:{}:{}:{}", r.especie, u8::from(r.shiny), r.moeda),
            para_hist(&r.agregado),
        );
    });
    mapa
}

/// `dexDe`: número da dex para a arte da espécie.
fn dex_de(central: &Central, sp: i32) -> Option<i32> {
    if (13000..14000).contains(&sp) {
        return Some(sp - 13000);
    }
    let Some(e) = central.especie(sp) else {
        return Some(sp);
    };
    if let Some(x) = e.sprite_id.or(e.mega_base).filter(|x| *x != 0) {
        return Some(x);
    }
    if e.id < 1100 {
        return Some(e.id);
    }
    // Formas sem arte própria: procura o nome base nas palavras finais ("Alolan Vulpix").
    let palavras: Vec<String> = e
        .nome
        .to_lowercase()
        .split(' ')
        .map(str::to_owned)
        .collect();
    (1..palavras.len()).find_map(|i| central.dex_por_nome(&palavras[i..].join(" ")))
}

fn sprite_de(central: &Central, especie: Option<i32>, shiny: bool) -> Option<String> {
    let dex = dex_de(central, especie?)?;
    (dex > 0 && dex < 1100).then(|| {
        format!(
            "https://raw.githubusercontent.com/PokeAPI/sprites/master/sprites/pokemon/{}{dex}.png",
            if shiny { "shiny/" } else { "" }
        )
    })
}

/// Os últimos 5000 pontos de um ticker, como `(momento, preço unitário)`.
/// `None` se o ticker não existe.
///
/// Lifetime explícito `'m`: o iterador devolvido aponta para dentro do `mercado`, sem copiar,
/// então só pode ser usado enquanto o mercado existir. Como a função recebe duas
/// referências, o compilador não adivinha de qual delas o resultado depende; o `'m` diz
/// "do mercado, não do `id`". Por isso quem chama pode descartar o `id` logo depois.
///
/// `impl Iterator<...>` no retorno: "devolvo algum iterador desses itens", sem expor o tipo
/// concreto (que seria enorme). Quem chama decide se faz `collect`, conta, etc.
pub fn serie<'m>(
    mercado: &'m Mercado,
    id: &TickerId,
) -> Option<impl Iterator<Item = (i64, f64)> + 'm> {
    mercado.ticker(id)?;
    let pontos = mercado.vendas_de(id);
    let inicio = pontos.len().saturating_sub(SERIE_MAX_PONTOS);
    Some(pontos[inicio..].iter().map(|p| (p.em, p.unidade)))
}

#[cfg(test)]
mod testes {
    use std::sync::Mutex;

    use proptest::prelude::*;

    use super::*;
    use crate::bolsa::carregador::Carregador;
    use crate::bolsa::catalogo::ItemCatalogo;
    use crate::bolsa::repo::{ErroRepo, Estaticos, FiltroOfertas, OfertaBruta, RepoMercado};

    const AGORA: i64 = 1_800_000_000_000;

    fn venda(id: i64, em: i64, item_id: Option<i32>, preco: i64) -> VendaBruta {
        VendaBruta {
            id,
            em,
            lido_em: Some(em),
            ficha: None,
            tipo: "item".into(),
            moeda: "orb".into(),
            preco: Some(preco),
            quantidade: Some(1),
            descricao: Some("Coisa".into()),
            item_id,
            especie: None,
            shiny: false,
        }
    }

    fn id(texto: &str) -> TickerId {
        texto.parse().unwrap()
    }

    fn fontes(vendas: Vec<VendaBruta>) -> Fontes {
        Fontes {
            vendas,
            ..Default::default()
        }
    }

    fn serie_vec(m: &Mercado, texto: &str) -> Option<Vec<(i64, f64)>> {
        serie(m, &id(texto)).map(Iterator::collect)
    }

    #[test]
    fn vazio_e_um_ponto_nao_viram_ticker() {
        let m = Mercado::montar(&fontes(vec![]), AGORA);
        assert_eq!(m.total_tickers(), 0);
        assert!(serie_vec(&m, "i:1:orb").is_none());

        let m = Mercado::montar(&fontes(vec![venda(1, AGORA, Some(1), 10)]), AGORA);
        assert!(serie_vec(&m, "i:1:orb").is_none());
    }

    #[test]
    fn oito_vendas_viram_ticker_sete_nao() {
        let vendas: Vec<_> = (0..8)
            .map(|i| venda(i, AGORA - i * 1000, Some(1), 10 + i))
            .collect();
        let m = Mercado::montar(&fontes(vendas[..7].to_vec()), AGORA);
        assert!(m.ticker(&id("i:1:orb")).is_none());

        let m = Mercado::montar(&fontes(vendas), AGORA);
        let s = serie_vec(&m, "i:1:orb").unwrap();
        assert_eq!(s.len(), 8);
        assert!(s.windows(2).all(|par| par[0].0 <= par[1].0));
    }

    #[test]
    fn serie_corta_nos_ultimos_5000() {
        let vendas = (0..6000)
            .map(|i| venda(i, AGORA - 6000 + i, Some(1), 5))
            .collect();
        let m = Mercado::montar(&fontes(vendas), AGORA);
        let s = serie_vec(&m, "i:1:orb").unwrap();
        assert_eq!(s.len(), SERIE_MAX_PONTOS);
        assert_eq!(s.last().map(|p| p.0), Some(AGORA - 1));
    }

    /// Repositório em memória: a outra implementação da porta. O teste exercita o
    /// carregamento inteiro sem banco.
    struct RepoMemoria {
        vendas: Mutex<Vec<VendaBruta>>,
        consultas: Mutex<Vec<String>>,
    }

    impl RepoMercado for RepoMemoria {
        async fn vendas(&self, desde: i64, ate: i64) -> Result<Vec<VendaBruta>, ErroRepo> {
            self.consultas
                .lock()
                .unwrap()
                .push(format!("janela {desde} {ate}"));
            Ok(self
                .vendas
                .lock()
                .unwrap()
                .iter()
                .filter(|v| v.em >= desde && v.em <= ate)
                .cloned()
                .collect())
        }

        async fn vendas_recentes(&self, em_desde: i64) -> Result<Vec<VendaBruta>, ErroRepo> {
            self.consultas
                .lock()
                .unwrap()
                .push(format!("recentes {em_desde}"));
            Ok(self
                .vendas
                .lock()
                .unwrap()
                .iter()
                .filter(|v| v.em >= em_desde)
                .cloned()
                .collect())
        }

        async fn estaticos(&self) -> Result<Estaticos, ErroRepo> {
            Ok(Estaticos {
                catalogo: vec![ItemCatalogo {
                    id: 9,
                    nome: "Coisa".into(),
                    categoria: None,
                    raro: false,
                    preco_npc: None,
                    icone: Some("icone.png".into()),
                }],
                ..Default::default()
            })
        }

        async fn agregados(&self, _: i64, _: i64, _: i64, _: bool) -> Result<Agregados, ErroRepo> {
            Ok(Agregados::default())
        }

        async fn ofertas_ativas(&self, _: &FiltroOfertas) -> Result<Vec<OfertaBruta>, ErroRepo> {
            Ok(vec![])
        }

        async fn ofertas_do_ticker(
            &self,
            _: bool,
            _: i32,
            _: &str,
            _: i64,
        ) -> Result<Vec<OfertaBruta>, ErroRepo> {
            Ok(vec![])
        }
    }

    #[tokio::test]
    async fn carga_incremental_pega_vendas_novas_e_usa_o_catalogo() {
        let vendas: Vec<_> = (0..8)
            .map(|i| venda(i, AGORA - i * 60_000, None, 10))
            .collect();
        let repo = RepoMemoria {
            vendas: Mutex::new(vendas),
            consultas: Mutex::new(vec![]),
        };
        let mut c = Carregador::default();

        let m = Mercado::montar(&c.fontes(&repo, AGORA).await.unwrap(), AGORA);
        // Sem item_id: o nome "Coisa" resolve para o id 9 pelo catálogo.
        let t = m.ticker(&id("i:9:orb")).unwrap();
        assert_eq!(t.sprite.as_deref(), Some("icone.png"));
        assert_eq!(serie_vec(&m, "i:9:orb").unwrap().len(), 8);

        repo.vendas
            .lock()
            .unwrap()
            .push(venda(99, AGORA + 1000, None, 12));
        let depois = AGORA + 60_000;
        let m = Mercado::montar(&c.fontes(&repo, depois).await.unwrap(), depois);
        assert_eq!(serie_vec(&m, "i:9:orb").unwrap().len(), 9);

        let consultas = repo.consultas.lock().unwrap();
        assert!(consultas[0].starts_with("janela"));
        assert!(consultas[1].starts_with("recentes"), "{consultas:?}");
    }

    proptest! {
        #[test]
        fn serie_nunca_tem_nan_nem_passa_do_limite(
            pontos in proptest::collection::vec((0i64..60 * DIA_MS, any::<i64>(), -2i32..50), 0..300)
        ) {
            let vendas = pontos
                .into_iter()
                .enumerate()
                .map(|(i, (atraso, preco, qtd))| VendaBruta { quantidade: Some(qtd), ..venda(i as i64, AGORA - atraso, Some(1), preco) })
                .collect();
            let m = Mercado::montar(&fontes(vendas), AGORA);
            if let Some(s) = serie_vec(&m, "i:1:orb") {
                prop_assert!(s.len() >= estat::MIN_VENDAS && s.len() <= SERIE_MAX_PONTOS);
                prop_assert!(s.iter().all(|(_, v)| v.is_finite()));
            }
            for t in &m.tickers {
                prop_assert!(t.a.ultimo.is_finite() && t.vd.score.is_finite());
            }
        }
    }
}
