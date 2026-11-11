//! Testes de integração contra um Postgres de verdade (TimescaleDB, como em produção),
//! num container descartável. Precisa de Docker.
//!
//! O banco recebe a baseline das tabelas do legado (`migrations-teste/`, só para testes) e
//! depois as migrations do Nexus. É o único lugar onde as rotas de escrita da bolsa rodam
//! antes do cutover.

// Arquivo de teste: falhar com pânico é o jeito certo de reprovar. O `clippy.toml` já libera
// isso dentro de funções `#[test]`, mas não nas funções auxiliares deste arquivo.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::time::Duration;

use nucleo::bolsa::carregador::Carregador;
use nucleo::bolsa::catalogo::{Catalogo, ItemCatalogo};
use nucleo::bolsa::ingestao::{validar_ofertas, validar_resumo, MeusItens, RepoIngestao};
use nucleo::bolsa::mercado::Mercado;
use nucleo::bolsa::repo::{FiltroOfertas, RepoMercado};
use nucleo::bolsa::ticker::TickerId;
use persistencia::{importacao, PgRepoMercado};
use serde_json::{json, Value};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use testcontainers::core::{IntoContainerPort, WaitFor};
use testcontainers::runners::AsyncRunner;
use testcontainers::{ContainerAsync, GenericImage, ImageExt};

const AGORA: i64 = 1_800_000_000_000;

async fn banco() -> (ContainerAsync<GenericImage>, PgPool) {
    let container = GenericImage::new("timescale/timescaledb-ha", "pg16")
        .with_exposed_port(5432.tcp())
        .with_wait_for(WaitFor::message_on_stderr(
            "database system is ready to accept connections",
        ))
        .with_env_var("POSTGRES_PASSWORD", "teste")
        .start()
        .await
        .expect("não consegui subir o container do Postgres (o Docker está rodando?)");
    let porta = container.get_host_port_ipv4(5432).await.unwrap();
    let url = format!("postgres://postgres:teste@127.0.0.1:{porta}/postgres");
    // O Postgres reinicia uma vez depois do initdb: tenta conectar por até 60 s.
    let mut tentativas = 0;
    let pool = loop {
        match PgPoolOptions::new().max_connections(4).connect(&url).await {
            Ok(p) if sqlx::query("SELECT 1").execute(&p).await.is_ok() => break p,
            _ if tentativas < 120 => {
                tentativas += 1;
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            Err(e) => panic!("Postgres de teste não respondeu: {e}"),
            Ok(_) => panic!("Postgres de teste não respondeu"),
        }
    };
    sqlx::raw_sql(include_str!(
        "../../../migrations-teste/0001_baseline_public.sql"
    ))
    .execute(&pool)
    .await
    .unwrap();
    sqlx::migrate!("../../migrations").run(&pool).await.unwrap();
    (container, pool)
}

async fn ofertas_no_banco(db: &PgPool) -> Vec<Value> {
    sqlx::query_scalar::<_, Value>(
        "SELECT coalesce(jsonb_agg(to_jsonb(t) ORDER BY id), '[]') FROM public.fato_oferta_mercado t",
    )
    .fetch_one(db)
    .await
    .unwrap()
    .as_array()
    .cloned()
    .unwrap()
}

fn sem_ts_visto(mut linhas: Vec<Value>) -> Vec<Value> {
    linhas.iter_mut().for_each(|l| {
        l.as_object_mut().map(|o| o.remove("ts_visto"));
    });
    linhas
}

/// Um único teste com vários cenários: subir o container é o passo caro.
#[tokio::test]
async fn bolsa_no_banco_de_teste() {
    let (_container, db) = banco().await;
    let repo = PgRepoMercado::novo(db.clone());
    ingestao_de_ofertas_e_idempotente(&repo, &db).await;
    coleta_completa_so_desativa_o_proprio_escopo(&repo, &db).await;
    resumo_e_inventario(&repo).await;
    leitura_monta_o_mercado(&repo, &db).await;
    equipes_no_banco(&db).await;
    equipe_da_bolsa_no_banco(&db).await;
}

/// Wiki de bosses, faltas, sugestões e preços típicos.
async fn equipe_da_bolsa_no_banco(db: &PgPool) {
    use nucleo::bolsa_equipe::{parse_wiki, Faltas};
    use persistencia::bolsa_equipe as be;

    let wiki = parse_wiki("## Bosses\n| Kanto · Aves | Articuno | 300 | Fragmento de Z-Move (0,5%) · TM AoE Piece (0,5%) |\n## Fim");
    assert_eq!(be::importar_wiki(db, &wiki).await.unwrap(), 1);
    assert_eq!(be::carregar_wiki(db).await.unwrap(), wiki);

    let mut f = Faltas::default();
    f.0.insert(
        "articuno|economica".into(),
        json!({"boss": "articuno", "nivel": 2, "expira": 10}),
    );
    be::gravar_faltas(db, &f).await.unwrap();
    assert_eq!(be::carregar_faltas(db).await.unwrap(), f);

    be::registrar_sugestao(db, 1, "articuno", &json!({"ok": true}))
        .await
        .unwrap();
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM nexus.equipe_sugestao")
        .fetch_one(db)
        .await
        .unwrap();
    assert_eq!(n, 1);

    // Preços típicos: só roda a consulta contra o schema do legado (o conteúdo vem das
    // vendas de teste, que podem não ter pokémon na faixa).
    be::precos_tipicos(db, "orb", 1.0, 1e9, 0).await.unwrap();
}

/// Pokédex do motor e estado de equipes: ida e volta, com a ordem preservada.
async fn equipes_no_banco(db: &PgPool) {
    use persistencia::equipes;

    let dex = json!([{"d": 9, "n": "B"}, {"d": 3, "n": "A"}]);
    assert_eq!(equipes::importar_pokedex(db, &dex).await.unwrap(), 2);
    assert_eq!(equipes::carregar_pokedex(db).await.unwrap(), dex);

    let vazio = equipes::carregar_estado(db).await.unwrap();
    assert!(vazio.bosses.is_empty() && vazio.contas.is_empty());

    let mut e: nucleo::equipes::EstadoEquipes = serde_json::from_str(
        r#"{"bosses":[{"key":"x"}],"bossesEm":5,"contas":{"zeta":{"trLv":1},"alfa":{"trLv":2}}}"#,
    )
    .unwrap();
    assert_eq!(equipes::importar_estado(db, &e).await.unwrap(), 2);
    assert_eq!(equipes::carregar_estado(db).await.unwrap(), e);

    let corpo =
        json!({"nick": "novo", "trLv": 3, "bosses": [{"key": "a"}, {"key": "b"}, {"key": "c"}]});
    let (nick, _) = e.ingerir(&corpo, 99).unwrap();
    equipes::gravar_ingest(db, &e, &nick).await.unwrap();
    let lido = equipes::carregar_estado(db).await.unwrap();
    assert_eq!(lido, e);
    assert_eq!(
        lido.contas
            .iter()
            .map(|(n, _)| n.as_str())
            .collect::<Vec<_>>(),
        ["zeta", "alfa", "novo"]
    );
}

async fn ingestao_de_ofertas_e_idempotente(repo: &PgRepoMercado, db: &PgPool) {
    let lote = validar_ofertas(
        &json!([
            { "id": 1, "preco": 500, "moeda": "orb", "ficha": { "speciesId": 25, "nome": "Pikachu" } },
            { "id": 2, "preco": 90, "itemId": 7, "qtd": 3 },
            { "id": 2, "preco": 95, "itemId": 7, "qtd": 3, "expiraEm": 1_900_000_000_000_i64 },
        ]),
        &Value::Null,
        &Catalogo::default(),
    );
    assert_eq!(lote.ofertas.len(), 2);
    assert_eq!(
        repo.gravar_ofertas(&lote.ofertas, AGORA, None)
            .await
            .unwrap(),
        2
    );
    let antes = ofertas_no_banco(db).await;

    // Mesmo lote, mesmo instante: estado idêntico.
    repo.gravar_ofertas(&lote.ofertas, AGORA, None)
        .await
        .unwrap();
    assert_eq!(ofertas_no_banco(db).await, antes);

    // Mesmo lote mais tarde: só o "visto por último" muda; ts_primeiro e expira_em ficam.
    repo.gravar_ofertas(&lote.ofertas, AGORA + 60_000, None)
        .await
        .unwrap();
    let depois = ofertas_no_banco(db).await;
    assert_eq!(sem_ts_visto(depois.clone()), sem_ts_visto(antes));
    // Repetida no lote, vale a última ocorrência.
    assert_eq!(depois[1]["preco"], json!(95));
    assert_eq!(depois[1]["quantidade"], json!(3));

    // Regravar sem `expiraEm` mantém o vencimento já conhecido.
    let sem_expira = validar_ofertas(
        &json!([{ "id": 2, "preco": 95, "itemId": 7, "qtd": 3 }]),
        &Value::Null,
        &Catalogo::default(),
    );
    repo.gravar_ofertas(&sem_expira.ofertas, AGORA + 60_000, None)
        .await
        .unwrap();
    assert_eq!(
        ofertas_no_banco(db).await[1]["expira_em"],
        json!(1_900_000_000_000_i64)
    );
}

async fn coleta_completa_so_desativa_o_proprio_escopo(repo: &PgRepoMercado, db: &PgPool) {
    // Nova coleta completa só de itens em gold: a oferta 2 (item/gold) some do lote e é
    // desativada; a 1 (pokémon/orb) não é tocada, porque é de outro escopo.
    let lote = validar_ofertas(
        &json!([{ "id": 3, "preco": 80, "itemId": 7, "moeda": "gold" }]),
        &json!(true),
        &Catalogo::default(),
    );
    repo.gravar_ofertas(
        &lote.ofertas,
        AGORA + 120_000,
        lote.escopo_completo.as_ref(),
    )
    .await
    .unwrap();
    let ativo: Vec<(i64, bool)> =
        sqlx::query_as("SELECT id, ativo FROM public.fato_oferta_mercado ORDER BY id")
            .fetch_all(db)
            .await
            .unwrap();
    assert_eq!(ativo, vec![(1, true), (2, false), (3, true)]);

    // Limpeza periódica: oferta não vista há mais de 6 h fica inativa.
    let n = repo.limpar_ofertas(AGORA + 7 * 3_600_000).await.unwrap();
    assert_eq!(n, 2);
}

async fn resumo_e_inventario(repo: &PgRepoMercado) {
    let (itens, linhas) =
        validar_resumo(&json!({ "7": { "minOrb": 3, "minGold": 900, "anuncios": 4 } }));
    assert_eq!((itens, linhas.len()), (1, 2));
    assert_eq!(repo.gravar_resumo(&linhas, AGORA).await.unwrap(), 2);
    // A chave é (ts, item, moeda): reenviar o mesmo resumo não duplica.
    assert_eq!(repo.gravar_resumo(&linhas, AGORA).await.unwrap(), 0);

    let inv = MeusItens {
        ts: Some(AGORA),
        itens: [(7, 3), (9, 1)].into_iter().collect(),
    };
    repo.gravar_meus_itens(&inv).await.unwrap();
    repo.gravar_meus_itens(&inv).await.unwrap();
    assert_eq!(repo.meus_itens().await.unwrap(), inv);
}

async fn leitura_monta_o_mercado(repo: &PgRepoMercado, db: &PgPool) {
    importacao::catalogo(
        db,
        &[ItemCatalogo {
            id: 7,
            nome: "Rare Candy".into(),
            categoria: None,
            raro: false,
            preco_npc: None,
            icone: Some("rc.png".into()),
        }],
    )
    .await
    .unwrap();
    // 10 vendas de item sem item_id, em dias diferentes: o catálogo resolve o id pelo nome.
    for i in 0..10_i64 {
        sqlx::query(
            "INSERT INTO public.fato_venda_mercado (id, em, lido_em, tipo, moeda, preco, descricao, quantidade)
             VALUES ($1, $2, $2, 'item', 'gold', $3, '2 × Rare Candy', 2)",
        )
        .bind(100 + i)
        .bind(AGORA - i * 86_400_000 / 2)
        .bind(200 + i * 2)
        .execute(db)
        .await
        .unwrap();
    }
    let mut carregador = Carregador::default();
    let fontes = carregador.fontes(repo, AGORA).await.unwrap();
    let m = Mercado::montar(&fontes, AGORA);
    let id: TickerId = "i:7:gold".parse().unwrap();
    let t = m.ticker(&id).expect("ticker do item");
    assert_eq!(t.nome, "Rare Candy");
    assert_eq!(t.sprite.as_deref(), Some("rc.png"));
    assert_eq!(m.vendas_de(&id).len(), 10);
    assert!(t.a.ultimo > 0.0 && t.vd.score.is_finite());

    // Ofertas ativas (a 3 está ativa? não: a limpeza acima desativou). Reativa e lê.
    let lote = validar_ofertas(
        &json!([{ "id": 3, "preco": 80, "itemId": 7, "moeda": "gold" }]),
        &Value::Null,
        &Catalogo::default(),
    );
    repo.gravar_ofertas(&lote.ofertas, AGORA, None)
        .await
        .unwrap();
    let ofertas = repo
        .ofertas_ativas(&FiltroOfertas {
            corte_item: AGORA - 45 * 60_000,
            corte_pokemon: AGORA - 90 * 60_000,
            limite: 100,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(ofertas.iter().map(|o| o.id).collect::<Vec<_>>(), vec![3]);
    let ag = repo
        .agregados(
            AGORA - 45 * 60_000,
            AGORA - 90 * 60_000,
            AGORA - 3 * 3_600_000,
            true,
        )
        .await
        .unwrap();
    assert_eq!(ag.ofertas_itens.len(), 1);
    assert_eq!(ag.hist_itens.len(), 1);
    assert_eq!(ag.asks.len(), 2);
}
