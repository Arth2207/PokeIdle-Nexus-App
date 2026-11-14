//! nexus-api: servidor HTTP e WebSocket do Nexus.
//!
//! Uso:
//!   nexus-api                                  sobe o servidor
//!   nexus-api migrar                           aplica as migrations pendentes
//!   nexus-api token criar <nome> <escopos> [conta]  cria um token (escopos separados por vírgula; com conta, só enxerga essa conta)
//!   nexus-api importar itens < itens.json      catálogo de itens (items-catalogo.json do legado)
//!   nexus-api importar especies < esp.json     catálogo de espécies (PI_ESPECIES em JSON)
//!   nexus-api importar especies-js < a.js b.js  o mesmo, direto dos .js do precificador (sem Node)
//!   nexus-api importar base < base.json        base do precificador (vendas antigas, cotações)
//!   nexus-api importar meus-itens < inv.json   inventário (.bolsa-meus-itens.json do legado)
//!   nexus-api importar pokedex < pokedex.json  pokédex do motor de combate
//!   nexus-api importar equipes < estado.json   estado de equipes (equipes-estado.json do legado)
//!   nexus-api importar wiki < wiki.txt         bosses da wiki do jogo (POKEIDLE-wiki.txt)
//!   nexus-api importar faltas < faltas.json    faltas de coleta (.bolsa-faltas.json do legado)
//!   nexus-api importar raros < raros.json      nomes dos itens raros (raros-estado.json do legado)
//!   nexus-api importar welcome < payload.json  constantes do servidor (tabela de tipos etc.)
//!   nexus-api pokedex-divergencias             diagnóstico: pokédex do motor × catálogo do precificador
//!   nexus-api paridade [url]                   uma rodada de comparação com o legado
//!   nexus-api paridade-continua                comparador em loop, gravando em nexus.paridade
//!   nexus-api paridade-motor <url>             compara só as rotas GET do motor (motor.*)
//!   nexus-api motor-sombra <url> [--ciclos n]   roda n ciclos do motor em sombra e sai
//!   nexus-api motor-sombra-relatorio [--desde h] decisões do motor em sombra × ações do legado
//!   nexus-api avaliar-fichas <agora_ms>        diagnóstico: valor justo de fichas (stdin)
//!   nexus-api sombra                           cliente sombra do WebSocket (NEXUS_SOMBRA_URL, NEXUS_SOMBRA_TOKEN_FILE)
//!   nexus-api sombra-relatorio [--desde <h>] [--origem espelho|sintetico|todas]   relatório da sombra (só SELECT; padrão 168 h, todas)
//!   nexus-api healthcheck                      checa o /health/ready local (usado pelo Docker)

mod auth;
mod bolsa;
mod comandos;
mod configuracao;
mod conversao;
mod desligamento;
mod dossie;
mod equipes;
mod estado;
mod etag;
mod limite;
mod metricas;
mod motor;
mod motor_envio;
mod motor_sombra;
mod raros;
mod rotas;
mod sombra;
mod telemetria;
mod webhook;
mod ws;

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, ensure, Context};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_util::sync::CancellationToken;

use crate::auth::Escopo;
use crate::configuracao::Configuracao;
use crate::estado::Estado;

const USO: &str = "uso: nexus-api [migrar | token criar <nome> <escopos> [conta] | \
importar itens|especies|especies-js|base|meus-itens|pokedex|equipes|wiki|faltas|welcome|raros|motor|motor-contas | \
comparar-motor motor|motor-contas | paridade [url] | \
paridade-continua | paridade-motor <url> | motor-sombra <url> [--ciclos <n>] | motor-sombra-relatorio [--desde <horas>] | sombra | sombra-relatorio [--desde <horas>] [--origem espelho|sintetico|todas] | avaliar-fichas <agora_ms> | pokedex-divergencias | healthcheck]";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // O reqwest usa rustls sem provedor embutido: instala o `ring` antes de criar qualquer
    // cliente HTTP (comparador, webhook). `Err` só quer dizer que já havia um instalado.
    let _ = rustls::crypto::ring::default_provider().install_default();

    // Em desenvolvimento lê o arquivo .env. Em produção ele não existe, e tudo bem.
    dotenvy::dotenv().ok();

    let cfg = Configuracao::carregar().context("configuração inválida")?;
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();

    match args.as_slice() {
        [] => servir(cfg).await,
        ["migrar"] => migrar(&cfg).await,
        ["token", "criar", nome, escopos] => criar_token(&cfg, nome, escopos, None).await,
        ["token", "criar", nome, escopos, conta] => {
            criar_token(&cfg, nome, escopos, Some(conta)).await
        }
        ["importar", tipo] => importar(&cfg, tipo).await,
        ["pokedex-divergencias"] => pokedex_divergencias(&cfg).await,
        ["avaliar-fichas", agora] => avaliar_fichas(&cfg, agora).await,
        ["paridade-continua"] => paridade_continua(cfg).await,
        ["paridade-motor", url] => paridade_motor(&cfg, url).await,
        ["paridade"] => paridade(&cfg, None).await,
        ["paridade", url] => paridade(&cfg, Some(url)).await,
        ["sombra"] => {
            telemetria::iniciar(cfg.log_formato).context("não consegui iniciar os logs")?;
            let estado = Estado::novo(&cfg).context("NEXUS_DATABASE_URL inválida")?;
            sombra::executar(estado.db.clone()).await
        }
        ["motor-sombra", url, args @ ..] => {
            let ciclos: u64 = match args {
                ["--ciclos", n] => n.parse().context("--ciclos precisa de um número")?,
                [] => 1,
                _ => anyhow::bail!("uso: motor-sombra <url> [--ciclos <n>]"),
            };
            let estado = Estado::novo(&cfg).context("NEXUS_DATABASE_URL inválida")?;
            let mut sombra =
                motor_sombra::Sombra::nova(url, cfg.motor_sombra_fuso_ms(), cfg.escrita_motor)?;
            for i in 0..ciclos {
                if i > 0 {
                    tokio::time::sleep(Duration::from_secs(cfg.motor_sombra_s.max(10))).await;
                }
                // Um ciclo que falha (rede lenta, legado reiniciando) não derruba os outros.
                match sombra.ciclo(&estado.db).await {
                    Ok((decisoes, acoes)) => println!(
                        "ciclo {}: {decisoes} decisões, {acoes} ações novas do legado",
                        i + 1
                    ),
                    Err(e) => println!("ciclo {}: falhou: {e:#}", i + 1),
                }
            }
            Ok(())
        }
        ["motor-sombra-relatorio", args @ ..] => {
            let horas = match args {
                ["--desde", h] => h.parse().context("--desde precisa de um número de horas")?,
                [] => 24,
                _ => anyhow::bail!("uso: motor-sombra-relatorio [--desde <horas>]"),
            };
            let estado = Estado::novo(&cfg).context("NEXUS_DATABASE_URL inválida")?;
            // Duas vezes o ciclo do legado, mais um minuto de folga.
            motor_sombra::relatorio(&estado.db, horas, 2 * 30_000 + 60_000).await
        }
        ["sombra-relatorio", args @ ..] => {
            let (mut horas, mut origem) = (168, "todas");
            let mut it = args.iter();
            while let Some(a) = it.next() {
                match (&**a, it.next()) {
                    ("--desde", Some(h)) => {
                        horas = h.parse().context("--desde espera horas inteiras")?;
                        ensure!(horas > 0, "--desde deve ser maior que zero");
                    }
                    ("--origem", Some(o)) => origem = o,
                    _ => bail!(USO),
                }
            }
            sombra_relatorio(&cfg, horas, origem).await
        }
        ["healthcheck"] => healthcheck(&cfg).await,
        ["comparar-motor", tipo] => {
            let texto =
                std::io::read_to_string(std::io::stdin()).context("não consegui ler o stdin")?;
            let db = Estado::novo(&cfg)
                .context("NEXUS_DATABASE_URL inválida")?
                .db;
            motor::comparar(&db, tipo, &texto).await
        }
        _ => bail!(USO),
    }
}

async fn servir(cfg: Configuracao) -> anyhow::Result<()> {
    telemetria::iniciar(cfg.log_formato).context("não consegui iniciar os logs")?;
    tracing::info!(?cfg, "configuração carregada");

    let metricas = metricas::instalar().context("não consegui iniciar as métricas")?;
    let mut estado = Estado::novo(&cfg).context("NEXUS_DATABASE_URL inválida")?;
    // Sinal de parada dos servidores HTTP; o WebSocket também o usa para fechar as conexões.
    let parar = CancellationToken::new();
    estado.ws = Arc::new(ws::Hub::novo(parar.clone()));

    // Uma task remonta o mercado em segundo plano; o comparador (se configurado) roda junto.
    let parar_tarefas = CancellationToken::new();
    Arc::clone(&estado.bolsa).iniciar(
        Duration::from_secs(cfg.bolsa_ciclo_s.max(10)),
        parar_tarefas.clone(),
    );
    // Modelo do jogo: calibração e divergência com o legado, só nas métricas.
    equipes::iniciar_modelos(
        estado.clone(),
        Duration::from_secs(600),
        parar_tarefas.clone(),
    );
    if let Some(url) = &cfg.paridade_url {
        let comparador = bolsa::paridade::Comparador::novo(Arc::clone(&estado.bolsa), url)
            .context("não consegui criar o cliente HTTP do comparador")?
            .com_equipes(estado.clone(), cfg.paridade_equipe)
            .com_motor(cfg.paridade_motor_url.as_deref());
        Arc::new(comparador).iniciar(
            Duration::from_secs(cfg.paridade_ciclo_s.max(60)),
            parar_tarefas.clone(),
        );
        tracing::info!(%url, "comparador de paridade ligado");
    }
    if cfg.escrita_bolsa {
        iniciar_limpeza_ofertas(estado.clone(), parar_tarefas.clone());
        tracing::warn!(
            "escrita da bolsa LIGADA: o Nexus grava em fato_oferta_mercado e fato_resumo_item"
        );
    }

    if cfg.escrita_comandos || cfg.espelho_comandos {
        comandos::iniciar(estado.clone(), parar_tarefas.clone());
        tracing::warn!(
            escrita = cfg.escrita_comandos,
            espelho = cfg.espelho_comandos,
            "fila de comandos do Nexus LIGADA"
        );
        ws::iniciar(estado.clone(), parar_tarefas.clone());
    }

    let app = rotas::router(estado.clone());
    let app_metricas = metricas::router(metricas, estado);

    let endereco = SocketAddr::new(cfg.endereco, cfg.porta);
    let endereco_metricas = SocketAddr::new(cfg.endereco, cfg.porta_metricas);
    let listener = abrir(endereco).await?;
    let listener_metricas = abrir(endereco_metricas).await?;
    tracing::info!(%endereco, %endereco_metricas, "nexus-api ouvindo");

    // Um único sinal de parada para os dois servidores.
    tokio::spawn({
        let parar = parar.clone();
        async move {
            desligamento::sinal_de_parada().await;
            parar.cancel();
        }
    });

    let api = axum::serve(listener, app).with_graceful_shutdown(parar.clone().cancelled_owned());
    let met = axum::serve(listener_metricas, app_metricas)
        .with_graceful_shutdown(parar.cancelled_owned());
    tokio::try_join!(api, met).context("servidor HTTP falhou")?;
    parar_tarefas.cancel();

    tracing::info!("nexus-api encerrado");
    Ok(())
}

async fn abrir(endereco: SocketAddr) -> anyhow::Result<TcpListener> {
    TcpListener::bind(endereco)
        .await
        .with_context(|| format!("não consegui abrir {endereco}"))
}

async fn sombra_relatorio(cfg: &Configuracao, horas: i32, origem: &str) -> anyhow::Result<()> {
    let estado = Estado::novo(cfg).context("NEXUS_DATABASE_URL inválida")?;
    sombra::executar_relatorio(&estado.db, horas, origem).await
}

async fn migrar(cfg: &Configuracao) -> anyhow::Result<()> {
    let estado = Estado::novo(cfg).context("NEXUS_DATABASE_URL inválida")?;
    sqlx::migrate!("../../migrations")
        .run(&estado.db)
        .await
        .context("falha ao aplicar as migrations")?;
    println!("migrations aplicadas");
    Ok(())
}

async fn criar_token(
    cfg: &Configuracao,
    nome: &str,
    escopos: &str,
    conta: Option<&str>,
) -> anyhow::Result<()> {
    let escopos = escopos
        .split(',')
        .map(|e| e.trim().parse::<Escopo>())
        .collect::<Result<Vec<_>, _>>()?;

    let estado = Estado::novo(cfg).context("NEXUS_DATABASE_URL inválida")?;
    ensure!(
        conta.is_none_or(|c| !c.trim().is_empty()),
        "conta vazia: omita a conta para um token sem conta"
    );
    let token = auth::criar_token(&estado.db, nome, &escopos, conta).await?;

    println!("token criado para {nome:?} com escopos {escopos:?}");
    println!("guarde agora, ele não será mostrado de novo:\n\n{token}\n");
    Ok(())
}

/// Formato do `items-catalogo.json` do legado: `{ "t": ..., "itens": [{ id, nome, ... }] }`.
#[derive(serde::Deserialize)]
struct ArquivoCatalogo {
    itens: Vec<ItemArquivo>,
}

#[derive(serde::Deserialize)]
struct ItemArquivo {
    id: i32,
    nome: String,
    categoria: Option<String>,
    #[serde(default)]
    raro: bool,
    npc: Option<i64>,
    icone: Option<String>,
}

/// Formato do `.bolsa-meus-itens.json`: `{ ts, itens: { [id]: quantidade } }`.
#[derive(serde::Deserialize)]
struct ArquivoMeusItens {
    ts: Option<i64>,
    #[serde(default)]
    itens: std::collections::BTreeMap<String, i64>,
}

/// Lê o primeiro array JSON que começa logo depois de `marca` em `texto`.
fn array_depois(texto: &str, marca: &str) -> anyhow::Result<Option<Vec<serde_json::Value>>> {
    let Some(pos) = texto.find(marca) else {
        return Ok(None);
    };
    let resto = &texto[pos + marca.len()..];
    let inicio = resto.find('[').context("array não encontrado")?;
    // O deserializador para no fim do primeiro valor; o JS que vem depois é ignorado.
    let mut fluxo = serde_json::Deserializer::from_str(&resto[inicio..]).into_iter();
    let valor: serde_json::Value = fluxo.next().context("array vazio")??;
    match valor {
        serde_json::Value::Array(lista) => Ok(Some(lista)),
        _ => anyhow::bail!("esperava um array depois de {marca:?}"),
    }
}

/// Catálogo de espécies a partir dos `.js` do precificador do legado (`especies.js` seguido de
/// `especies-extra.js`), sem Node: `PI_ESPECIES=[...]` e o complemento
/// `for(const e of [...])if(!tem.has(e.id))E.push(e)`, que só acrescenta ids novos.
fn especies_de_js(texto: &str) -> anyhow::Result<Vec<serde_json::Value>> {
    let mut lista =
        array_depois(texto, "PI_ESPECIES=")?.context("PI_ESPECIES não encontrado no catálogo")?;
    if let Some(extra) = array_depois(texto, "for(const e of ")? {
        // Como no JS: o conjunto é montado antes do laço e não muda dentro dele.
        let tem: std::collections::HashSet<i64> = lista
            .iter()
            .filter_map(|e| e.get("id").and_then(serde_json::Value::as_i64))
            .collect();
        for e in extra {
            let id = e.get("id").and_then(serde_json::Value::as_i64);
            if id.is_none_or(|id| !tem.contains(&id)) {
                lista.push(e);
            }
        }
    }
    Ok(lista)
}

/// Importa para o schema `nexus` um arquivo do legado lido do stdin.
async fn importar(cfg: &Configuracao, tipo: &str) -> anyhow::Result<()> {
    // Leitura bloqueante do stdin: é um comando de linha, roda uma vez e sai.
    let texto = std::io::read_to_string(std::io::stdin()).context("não consegui ler o stdin")?;
    let db = Estado::novo(cfg).context("NEXUS_DATABASE_URL inválida")?.db;
    match tipo {
        "itens" => {
            let arquivo: ArquivoCatalogo = serde_json::from_str(&texto).context("JSON inválido")?;
            ensure!(
                !arquivo.itens.is_empty(),
                "o catálogo veio vazio; nada foi alterado"
            );
            let itens: Vec<_> = arquivo
                .itens
                .into_iter()
                .map(|i| nucleo::bolsa::catalogo::ItemCatalogo {
                    id: i.id,
                    nome: i.nome,
                    categoria: i.categoria,
                    raro: i.raro,
                    preco_npc: i.npc,
                    icone: i.icone,
                })
                .collect();
            let n = persistencia::importacao::catalogo(&db, &itens).await?;
            println!("catálogo de itens importado: {n} itens");
        }
        "especies" | "especies-js" => {
            let lista: Vec<persistencia::importacao::EspecieArquivo> = if tipo == "especies-js" {
                serde_json::from_value(serde_json::Value::Array(especies_de_js(&texto)?))
                    .context("espécie fora do formato")?
            } else {
                serde_json::from_str(&texto).context("JSON inválido")?
            };
            ensure!(!lista.is_empty(), "a lista veio vazia; nada foi alterado");
            let n = persistencia::importacao::especies(&db, &lista).await?;
            println!("catálogo de espécies importado: {n} espécies");
        }
        "base" => {
            let base: persistencia::importacao::BaseArquivo =
                serde_json::from_str(&texto).context("JSON inválido")?;
            let (vendas, itens) = persistencia::importacao::base(&db, &base).await?;
            println!("base do precificador importada: {vendas} vendas, {itens} cotações de itens");
        }
        "meus-itens" => {
            use nucleo::bolsa::ingestao::{MeusItens, RepoIngestao};
            let arquivo: ArquivoMeusItens =
                serde_json::from_str(&texto).context("JSON inválido")?;
            let itens = arquivo
                .itens
                .into_iter()
                .filter_map(|(k, v)| Some((k.parse().ok()?, v)))
                .filter(|(k, v): &(i32, i64)| *k > 0 && *v > 0)
                .collect();
            let m = MeusItens {
                ts: arquivo.ts,
                itens,
            };
            persistencia::PgRepoMercado::novo(db)
                .gravar_meus_itens(&m)
                .await?;
            println!("inventário importado: {} itens", m.itens.len());
        }
        "pokedex" => {
            let valor: serde_json::Value = serde_json::from_str(&texto).context("JSON inválido")?;
            // Valida antes de gravar: uma pokédex que o motor não lê não substitui a atual.
            let dex = nucleo::combate::especie::Pokedex::de_valor(valor.clone())?;
            ensure!(
                !dex.especies().is_empty(),
                "a pokédex veio vazia; nada foi alterado"
            );
            let n = persistencia::equipes::importar_pokedex(&db, &valor).await?;
            println!("pokédex do motor importada: {n} espécies");
        }
        "equipes" => {
            // Com a escrita de equipes ligada, o Nexus é a fonte: a importação apagaria o
            // que o relay gravou aqui.
            ensure!(!cfg.escrita_equipes, "NEXUS_ESCRITA_EQUIPES ligada: o estado de equipes já é do Nexus; nada foi alterado");
            let e: nucleo::equipes::EstadoEquipes =
                serde_json::from_str(&texto).context("JSON inválido")?;
            ensure!(!(e.contas.is_empty() && e.bosses.is_empty()), "estado vazio; nada foi alterado");
            let n = persistencia::equipes::importar_estado(&db, &e).await?;
            println!(
                "estado de equipes importado: {} bosses, {n} contas",
                e.bosses.len()
            );
        }
        "wiki" => {
            let lista = nucleo::bolsa_equipe::parse_wiki(&texto);
            ensure!(!lista.is_empty(), "nenhum boss na seção \"## Bosses\"; nada foi alterado");
            let n = persistencia::bolsa_equipe::importar_wiki(&db, &lista).await?;
            println!("bosses da wiki importados: {n}");
        }
        "welcome" => {
            // O payload do `welcome` (como o coletor grava); as constantes ficam num bloco
            // interno que tem `tabelaTipos`.
            let v: serde_json::Value = serde_json::from_str(&texto).context("JSON inválido")?;
            fn acha(v: &serde_json::Value) -> Option<&serde_json::Value> {
                if v.get("tabelaTipos").is_some() {
                    return Some(v);
                }
                match v {
                    serde_json::Value::Object(m) => m.values().find_map(acha),
                    serde_json::Value::Array(a) => a.iter().find_map(acha),
                    _ => None,
                }
            }
            let bloco = acha(&v).context("o welcome não tem tabelaTipos")?;
            ensure!(
                nucleo::combate::tipos::Constantes::com_tabela_do_servidor(&bloco["tabelaTipos"]).is_some(),
                "tabelaTipos com tipo desconhecido; nada foi alterado"
            );
            let k = serde_json::json!({
                "tabelaTipos": bloco["tabelaTipos"], "ampliacaoHunt": bloco.get("ampliacaoHunt"),
                "multHpSelvagem": bloco.get("multHpSelvagem"), "multDanoSelvagem": bloco.get("multDanoSelvagem"),
                "maxEquipe": bloco.get("maxEquipe"), "potencias": bloco.get("potencias"),
            });
            persistencia::equipes::gravar_parametro(&db, equipes::CHAVE_CONSTANTES_SERVIDOR, &k).await?;
            println!("constantes do servidor importadas");
        }
        "raros" => {
            // raros-estado.json do legado: os nomes por id (sem o "(provável)") para a bolsa e o
            // estado inteiro (itens, histórico e acumulado de shiny) para `nexus.estado_raros`.
            let v: serde_json::Value = serde_json::from_str(&texto).context("JSON inválido")?;
            let itens = v.pointer("/raros/itens").and_then(serde_json::Value::as_array).context("sem raros.itens")?;
            let nomes: serde_json::Map<String, serde_json::Value> = itens
                .iter()
                .filter_map(|i| {
                    let id = i.get("id")?.as_i64()?;
                    let nome = i.get("nome")?.as_str()?.replacen(" (provável)", "", 1).replacen("(provável)", "", 1);
                    Some((id.to_string(), serde_json::Value::from(nome.trim_end().to_owned())))
                })
                .collect();
            let n = nomes.len();
            persistencia::equipes::gravar_parametro(&db, bolsa::consultas::CHAVE_RAROS_NOMES, &serde_json::Value::Object(nomes)).await?;
            println!("nomes de itens raros importados: {n}");
            if cfg.escrita_raros {
                println!("NEXUS_ESCRITA_RAROS ligada: o estado de raros já é do Nexus; nada foi alterado");
            } else {
                persistencia::raros::substituir(&db, &v).await?;
                let contas = v.pointer("/acc").and_then(serde_json::Value::as_object).map_or(0, serde_json::Map::len);
                println!("estado de raros importado (contas com histórico de shiny: {contas})");
            }
        }
        "faltas" => {
            ensure!(!cfg.escrita_equipes, "NEXUS_ESCRITA_EQUIPES ligada: as faltas já são do Nexus; nada foi alterado");
            let m: serde_json::Map<String, serde_json::Value> =
                serde_json::from_str(&texto).context("JSON inválido")?;
            let f = nucleo::bolsa_equipe::Faltas(m);
            persistencia::bolsa_equipe::gravar_faltas(&db, &f).await?;
            println!("faltas de coleta importadas: {}", f.0.len());
        }
        "motor" | "motor-contas" => motor::importar(&db, tipo, &texto).await?,
        outro => bail!(
            "não sei importar {outro:?} (use itens, especies, especies-js, base, meus-itens, pokedex, equipes, wiki, faltas, welcome, raros, motor ou motor-contas)"
        ),
    }
    Ok(())
}

/// Uma rodada do comparador, com o relatório no stdout.
async fn paridade(cfg: &Configuracao, url: Option<&str>) -> anyhow::Result<()> {
    let url = url
        .map(str::to_owned)
        .or_else(|| cfg.paridade_url.clone())
        .context("informe a URL do legado (argumento ou NEXUS_PARIDADE_URL)")?;
    let estado = Estado::novo(cfg).context("NEXUS_DATABASE_URL inválida")?;
    let comparador = bolsa::paridade::Comparador::novo(Arc::clone(&estado.bolsa), &url)?
        .com_equipes(estado.clone(), cfg.paridade_equipe)
        .com_motor(cfg.paridade_motor_url.as_deref());
    let relatorio = comparador.rodada().await.map_err(anyhow::Error::msg)?;
    println!("{}", serde_json::to_string_pretty(&relatorio)?);
    Ok(())
}

/// Compara só as rotas `GET` do motor com o motor do legado em `url`. Sai com erro se alguma
/// resposta diferir.
async fn paridade_motor(cfg: &Configuracao, url: &str) -> anyhow::Result<()> {
    let estado = Estado::novo(cfg).context("NEXUS_DATABASE_URL inválida")?;
    let comparador = bolsa::paridade::Comparador::novo(Arc::clone(&estado.bolsa), url)?
        .com_equipes(estado.clone(), false)
        .com_motor(Some(url));
    let rel = comparador
        .rodada_motor()
        .await
        .map_err(anyhow::Error::msg)?;
    println!("{}", serde_json::to_string_pretty(&rel)?);
    let diferentes: u64 = rel.values().map(|r| r.comparados - r.iguais).sum();
    anyhow::ensure!(diferentes == 0, "{diferentes} respostas do motor diferem");
    Ok(())
}

/// Comparador contínuo. Roda num container à parte, na rede do host (o firewall não deixa a
/// rede do Docker chegar na porta do legado), e grava cada rodada em `nexus.paridade`. A API
/// publica a última rodada em /metrics.
async fn paridade_continua(cfg: Configuracao) -> anyhow::Result<()> {
    telemetria::iniciar(cfg.log_formato).context("não consegui iniciar os logs")?;
    let url = cfg
        .paridade_url
        .clone()
        .context("NEXUS_PARIDADE_URL é obrigatória para o comparador")?;
    let ciclo = Duration::from_secs(cfg.paridade_ciclo_s.max(60));
    let estado = Estado::novo(&cfg).context("NEXUS_DATABASE_URL inválida")?;
    let comparador = bolsa::paridade::Comparador::novo(Arc::clone(&estado.bolsa), &url)?
        .com_equipes(estado.clone(), cfg.paridade_equipe)
        .com_motor(cfg.paridade_motor_url.as_deref());
    tracing::info!(%url, ciclo_s = ciclo.as_secs(), "comparador de paridade iniciado");
    let parar_sombra = tokio_util::sync::CancellationToken::new();
    // Como no `serve`: sem remontar, o snapshot do mercado fica congelado no início do
    // processo e a equipe da bolsa do Nexus é calculada sobre ofertas velhas.
    Arc::clone(&estado.bolsa).iniciar(
        Duration::from_secs(cfg.bolsa_ciclo_s.max(10)),
        parar_sombra.clone(),
    );
    // Fase 4: o laço do motor em sombra roda ao lado do comparador (mesma rede do host).
    if let Some(url_motor) = &cfg.motor_sombra_url {
        let sombra =
            motor_sombra::Sombra::nova(url_motor, cfg.motor_sombra_fuso_ms(), cfg.escrita_motor)?;
        tokio::spawn(sombra.rodar(
            estado.db.clone(),
            Duration::from_secs(cfg.motor_sombra_s.max(10)),
            parar_sombra.clone(),
        ));
    }
    let _cancelar_sombra = parar_sombra.drop_guard();
    let parar = desligamento::sinal_de_parada();
    tokio::pin!(parar);
    loop {
        match comparador.rodada().await {
            Ok(rel) => {
                rel.iter().for_each(|(rota, r)| {
                    tracing::info!(rota, comparados = r.comparados, iguais = r.iguais,
                        so_legado = r.so_legado, so_nexus = r.so_nexus, exemplos = ?r.exemplos, "paridade");
                });
                if let Err(erro) =
                    persistencia::paridade::gravar(&estado.db, &bolsa::paridade::linhas(&rel)).await
                {
                    tracing::warn!(%erro, "não consegui gravar a rodada de paridade");
                }
            }
            Err(erro) => tracing::warn!(%erro, "rodada de paridade falhou"),
        }
        tokio::select! {
            () = &mut parar => break,
            () = tokio::time::sleep(ciclo) => {}
        }
    }
    tracing::info!("comparador encerrado");
    Ok(())
}

/// Diagnóstico do precificador: lê `{ fichas: [{ id, ficha }] }` do stdin e devolve o valor
/// justo de cada uma no instante dado, para comparar com o precificador do legado.
async fn avaliar_fichas(cfg: &Configuracao, agora: &str) -> anyhow::Result<()> {
    use nucleo::precificador::ficha::Ficha;
    let agora: i64 = agora.parse().context("agora_ms inválido")?;
    let entrada: serde_json::Value =
        serde_json::from_str(&std::io::read_to_string(std::io::stdin())?)?;
    let estado = Estado::novo(cfg).context("NEXUS_DATABASE_URL inválida")?;
    let s = estado
        .bolsa
        .snapshot_em(agora)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let avaliador = s.mercado.avaliador();
    let saida: Vec<serde_json::Value> = entrada["fichas"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .map(|x| {
            let res = avaliador.avaliar(&Ficha::de_json(&x["ficha"]), agora).map(|v| {
                let e = &v.estimativa;
                serde_json::json!({
                    "low": e.low, "median": e.median, "high": e.high, "mercado": e.mercado_total,
                    "familia": e.familia_total, "percent": e.confianca.percent,
                    "chance": e.confianca.chance, "lowest": e.confianca.lowest, "pve": v.pve,
                    "nivel": v.nivel, "familiaNome": v.familia_nome, "faltam": v.faltam_adicionais,
                })
            });
            serde_json::json!({ "id": x["id"], "res": res })
        })
        .collect();
    println!("{}", serde_json::to_string(&saida)?);
    Ok(())
}

/// Marca inativas as ofertas expiradas ou não vistas há 6 h (`limparOfertas` do legado).
/// Só roda com a escrita da bolsa ligada.
fn iniciar_limpeza_ofertas(estado: Estado, parar: CancellationToken) {
    use nucleo::bolsa::ingestao::RepoIngestao;
    tokio::spawn(async move {
        let mut espera = Duration::from_secs(30);
        loop {
            tokio::select! {
                () = parar.cancelled() => break,
                () = tokio::time::sleep(espera) => {}
            }
            espera = Duration::from_secs(5 * 60);
            match estado.bolsa.repo.limpar_ofertas(bolsa::agora_ms()).await {
                Ok(n) if n > 0 => tracing::info!(n, "ofertas marcadas inativas"),
                Ok(_) => {}
                Err(erro) => tracing::warn!(%erro, "limpeza de ofertas falhou"),
            }
        }
    });
}

/// Faz um GET mínimo em /health/ready. A imagem final não tem curl nem shell,
/// então o próprio binário faz o papel de cliente.
async fn healthcheck(cfg: &Configuracao) -> anyhow::Result<()> {
    let checagem = async {
        let mut conexao = TcpStream::connect((Ipv4Addr::LOCALHOST, cfg.porta)).await?;
        conexao
            .write_all(b"GET /health/ready HTTP/1.0\r\nHost: localhost\r\n\r\n")
            .await?;
        let mut inicio = [0u8; 12];
        conexao.read_exact(&mut inicio).await?;
        anyhow::Ok(inicio)
    };

    let inicio = tokio::time::timeout(Duration::from_secs(3), checagem)
        .await
        .context("healthcheck: sem resposta em 3s")??;
    ensure!(
        inicio.ends_with(b" 200"),
        "healthcheck: resposta {:?}",
        String::from_utf8_lossy(&inicio)
    );
    Ok(())
}

/// Compara as duas pokédex do sistema: a do motor de combate (`nexus.pokedex_motor`) e o
/// catálogo do precificador (`nexus.especie`). Só lê.
async fn pokedex_divergencias(cfg: &Configuracao) -> anyhow::Result<()> {
    use std::collections::BTreeMap;
    let db = Estado::novo(cfg).context("NEXUS_DATABASE_URL inválida")?.db;
    let motor = persistencia::equipes::carregar_pokedex(&db).await?;
    let precificador: serde_json::Value = sqlx::query_scalar(
        "SELECT coalesce(jsonb_agg(jsonb_build_object('id', id, 'nome', nome, 'tipos', tipos, 'base', base)), '[]') FROM nexus.especie",
    )
    .fetch_one(&db)
    .await?;
    let por_id = |v: &serde_json::Value, id: &str| -> BTreeMap<i64, serde_json::Value> {
        v.as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| Some((x.get(id)?.as_i64()?, x.clone())))
                    .collect()
            })
            .unwrap_or_default()
    };
    let (m, p) = (por_id(&motor, "d"), por_id(&precificador, "id"));
    let so_motor: Vec<String> = m
        .iter()
        .filter(|(k, _)| !p.contains_key(k))
        .map(|(k, v)| format!("{k} {}", v["n"]))
        .collect();
    let so_prec: Vec<String> = p
        .iter()
        .filter(|(k, _)| !m.contains_key(k))
        .map(|(k, v)| format!("{k} {}", v["nome"]))
        .collect();
    let mut nome = Vec::new();
    let mut tipos = Vec::new();
    let mut base = Vec::new();
    for (k, a) in &m {
        let Some(b) = p.get(k) else { continue };
        if a["n"].as_str().map(str::to_lowercase) != b["nome"].as_str().map(str::to_lowercase) {
            nome.push(format!("{k}: {} × {}", a["n"], b["nome"]));
        }
        if a["t"] != b["tipos"] {
            tipos.push(format!("{k} {}: {} × {}", a["n"], a["t"], b["tipos"]));
        }
        let num = |v: &serde_json::Value| {
            v.as_array().map(|x| {
                x.iter()
                    .filter_map(serde_json::Value::as_f64)
                    .collect::<Vec<_>>()
            })
        };
        if num(&a["s"]) != num(&b["base"]) {
            base.push(format!("{k} {}: {} × {}", a["n"], a["s"], b["base"]));
        }
    }
    let mostra = |titulo: &str, l: &[String]| {
        println!("{titulo}: {}", l.len());
        for x in l.iter().take(15) {
            println!("  {x}");
        }
    };
    println!(
        "motor: {} espécies, precificador: {} espécies",
        m.len(),
        p.len()
    );
    mostra("só no motor", &so_motor);
    mostra("só no precificador", &so_prec);
    mostra("nome diferente", &nome);
    mostra("tipos diferentes", &tipos);
    mostra("status base diferentes", &base);
    Ok(())
}

#[cfg(test)]
mod testes {
    use super::especies_de_js;

    #[test]
    fn especies_de_js_junta_o_complemento_como_o_legado() {
        let base = r#"// gerado
(typeof window==='object'?window:globalThis).PI_ESPECIES=[{"id":1,"n":"A","t":["X]"]},{"id":2,"n":"B"}];
"#;
        let extra = r#"(function(root){'use strict';const E=root.PI_ESPECIES;if(!Array.isArray(E))return;const tem=new Set(E.map(e=>e.id));
for(const e of [{"id":2,"n":"B2"},{"id":3,"n":"C"},{"id":3,"n":"C2"}])if(!tem.has(e.id))E.push(e);
})(typeof window==='object'?window:globalThis);"#;
        let lista = especies_de_js(&format!("{base}{extra}")).expect("catálogo");
        let nomes: Vec<&str> = lista.iter().filter_map(|e| e["n"].as_str()).collect();
        // id 2 já existia; o 3 entra duas vezes, como no JS (o Set não muda no laço).
        assert_eq!(nomes, ["A", "B", "C", "C2"]);
        assert!(especies_de_js(extra).is_err());
        assert_eq!(especies_de_js(base).expect("só base").len(), 2);
    }
}

#[cfg(test)]
mod testes_catalogo_real {
    /// Confere com a saída do `node -e` antigo: `NEXUS_TESTE_ESPECIES_JS` (os .js concatenados)
    /// e `NEXUS_TESTE_ESPECIES_JSON` (JSON do Node). Roda só à mão (`--ignored`).
    #[test]
    #[ignore = "precisa dos arquivos do legado"]
    fn especies_de_js_bate_com_o_node() {
        let (Ok(js), Ok(json)) = (
            std::env::var("NEXUS_TESTE_ESPECIES_JS"),
            std::env::var("NEXUS_TESTE_ESPECIES_JSON"),
        ) else {
            return;
        };
        let texto = std::fs::read_to_string(js).expect("js");
        let node: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(json).expect("json")).expect("json");
        let rust = serde_json::Value::Array(super::especies_de_js(&texto).expect("rust"));
        assert_eq!(rust, node);
    }
}
