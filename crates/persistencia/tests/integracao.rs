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
    fase3_eventos_e_hunt(&db).await;
    fase3_comandos(&db).await;
    fase3_raros_e_tentativas(&db).await;
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

/// Eventos idempotentes (mesmo lote 2x = mesmas linhas) e hunt por conta.
async fn fase3_eventos_e_hunt(db: &PgPool) {
    use persistencia::eventos::{gravar, gravar_hunt, validar};

    // A tabela é hypertable quando o Timescale está na imagem (é o caso do container de teste).
    let hyper: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM timescaledb_information.hypertables
         WHERE hypertable_schema = 'nexus' AND hypertable_name = 'evento'",
    )
    .fetch_one(db)
    .await
    .unwrap();
    assert_eq!(hyper, 1, "nexus.evento deveria ser hypertable");

    let t = 1_800_000_000_000_i64;
    let eventos = vec![
        json!({"k": "kill", "ts": t, "nome": "Rattata", "xp_treinador": 5}),
        json!({"k": "kill", "ts": t + 1000, "nome": "Rattata", "xp_treinador": 5}),
        json!({"k": "perfil", "ts": t, "tr_lv": 40}),
        json!({"k": "capturado", "ts": t, "id": 77, "especie": "Pidgey"}),
        json!({"k": "inventado", "ts": t}),
    ];
    let lote = validar("conta-a", &eventos, t, false);
    assert_eq!((lote.eventos.len(), lote.erros.len()), (4, 1));
    let primeira = gravar(db, "conta-a", Some("id-a"), &lote.eventos)
        .await
        .unwrap();
    assert_eq!(primeira.len(), 4);
    // Mesmo lote de novo: nada entra, as linhas continuam as mesmas.
    let segunda = gravar(db, "conta-a", Some("id-a"), &lote.eventos)
        .await
        .unwrap();
    assert!(segunda.is_empty());
    // Outra conta com os mesmos eventos é outra linha.
    let outra = validar("conta-b", &eventos[..1], t, false);
    assert_eq!(
        gravar(db, "conta-b", None, &outra.eventos)
            .await
            .unwrap()
            .len(),
        1
    );

    let contagem: Vec<(String, String, i64)> = sqlx::query_as(
        "SELECT conta, tipo, count(*) FROM nexus.evento GROUP BY 1, 2 ORDER BY 1, 2",
    )
    .fetch_all(db)
    .await
    .unwrap();
    assert_eq!(
        contagem,
        vec![
            ("conta-a".into(), "capturado".into(), 1),
            ("conta-a".into(), "kill".into(), 2),
            ("conta-a".into(), "perfil".into(), 1),
            ("conta-b".into(), "kill".into(), 1),
        ]
    );
    // O `conta_id` do lote fica gravado ao lado da conta.
    let ids: Vec<(String, Option<String>)> =
        sqlx::query_as("SELECT DISTINCT conta, conta_id FROM nexus.evento ORDER BY 1")
            .fetch_all(db)
            .await
            .unwrap();
    assert_eq!(
        ids,
        vec![
            ("conta-a".into(), Some("id-a".into())),
            ("conta-b".into(), None)
        ]
    );
    // O instante gravado é o do evento.
    let em_ms: i64 = sqlx::query_scalar(
        "SELECT (extract(epoch FROM em) * 1000)::bigint FROM nexus.evento WHERE tipo = 'perfil'",
    )
    .fetch_one(db)
    .await
    .unwrap();
    assert_eq!(em_ms, t);

    let contas = vec![("a".to_owned(), "furious_scizor".to_owned())];
    assert_eq!(gravar_hunt(db, &contas, 10).await.unwrap(), 1);
    let contas = vec![("a".to_owned(), "ancient_pupitar".to_owned())];
    assert_eq!(gravar_hunt(db, &contas, 20).await.unwrap(), 1);
    let (n, slug): (i64, String) =
        sqlx::query_as("SELECT count(*) OVER (), dados->>'hunt_slug' FROM nexus.config_hunt")
            .fetch_one(db)
            .await
            .unwrap();
    assert_eq!((n, slug.as_str()), (1, "ancient_pupitar"));
}

/// Fila: reserva, ordem, lease, esgotar tentativas, confirmação idempotente, expiração,
/// espelho do legado e NOTIFY.
async fn fase3_comandos(db: &PgPool) {
    use persistencia::comandos::{self, Confirmacao, NovoComando, Reserva};

    let mut ouvinte = sqlx::postgres::PgListener::connect_with(db).await.unwrap();
    ouvinte.listen("nexus_comando").await.unwrap();

    let futuro = 4_000_000_000_000_i64;
    let params = json!({"recarregar": true});
    let novo = |conta: &'static str, prioridade: i32, expira_ms: i64| NovoComando {
        conta,
        tipo: "config",
        params: &params,
        prioridade,
        origem: "teste",
        expira_ms,
    };
    let a = comandos::inserir(db, &novo("c1", 5, futuro)).await.unwrap();
    let b = comandos::inserir(db, &novo("c1", 1, futuro)).await.unwrap();
    let c = comandos::inserir(db, &novo("c1", 9, futuro)).await.unwrap();
    let _expirado = comandos::inserir(db, &novo("c1", 5, 1_000_000_000_000))
        .await
        .unwrap();
    let _outra = comandos::inserir(db, &novo("c2", 5, futuro)).await.unwrap();

    // O trigger avisa de cada inserção.
    let aviso = tokio::time::timeout(Duration::from_secs(5), ouvinte.recv())
        .await
        .unwrap()
        .unwrap();
    let aviso: Value = serde_json::from_str(aviso.payload()).unwrap();
    assert_eq!(
        (aviso["id"].as_i64(), aviso["evento"].as_str()),
        (Some(a), Some("novo"))
    );

    // Ordem do legado: número menor primeiro; o expirado e o de outra conta ficam de fora.
    let r1 = comandos::reservar(db, "c1", &Reserva::nova("relay-1"))
        .await
        .unwrap();
    assert_eq!(r1.iter().map(|x| x.id).collect::<Vec<_>>(), vec![b, a, c]);
    // Já reservados: ninguém mais os recebe.
    assert!(comandos::reservar(db, "c1", &Reserva::nova("relay-2"))
        .await
        .unwrap()
        .is_empty());

    // Confirmação é idempotente.
    assert_eq!(
        comandos::confirmar(db, a, "ok").await.unwrap(),
        Confirmacao::Gravada
    );
    assert_eq!(
        comandos::confirmar(db, a, "outro").await.unwrap(),
        Confirmacao::Repetida
    );
    assert_eq!(
        comandos::confirmar(db, 999_999, "ok").await.unwrap(),
        Confirmacao::Inexistente
    );
    let resultado: String = sqlx::query_scalar("SELECT resultado FROM nexus.comando WHERE id = $1")
        .bind(a)
        .fetch_one(db)
        .await
        .unwrap();
    assert_eq!(resultado, "ok");

    // Lease vencido: b volta a pendente e o mesmo relay o recebe de novo (repetido).
    sqlx::query(
        "UPDATE nexus.comando SET reservado_em = now() - interval '10 minutes' WHERE id = ANY($1)",
    )
    .bind(vec![b, c])
    .execute(db)
    .await
    .unwrap();
    let r2 = comandos::reservar(db, "c1", &Reserva::nova("relay-1"))
        .await
        .unwrap();
    assert_eq!(r2.iter().map(|x| x.id).collect::<Vec<_>>(), vec![b, c]);
    assert!(r2.iter().all(|x| x.repetido));
    let tentativas: i32 = sqlx::query_scalar("SELECT tentativas FROM nexus.comando WHERE id = $1")
        .bind(b)
        .fetch_one(db)
        .await
        .unwrap();
    assert_eq!(tentativas, 1);

    // Depois de 5 reservas sem confirmação, sai da fila com erro.
    for _ in 0..4 {
        sqlx::query(
            "UPDATE nexus.comando SET reservado_em = now() - interval '10 minutes' WHERE id = $1",
        )
        .bind(b)
        .execute(db)
        .await
        .unwrap();
        let (devolvidos, encerrados) = comandos::devolver_vencidos(db, 120).await.unwrap();
        assert!(devolvidos + encerrados >= 1);
        let _ = comandos::reservar(db, "c1", &Reserva::nova("relay-1"))
            .await
            .unwrap();
    }
    let (res, exec): (Option<String>, bool) = sqlx::query_as(
        "SELECT resultado, executado_em IS NOT NULL FROM nexus.comando WHERE id = $1",
    )
    .bind(b)
    .fetch_one(db)
    .await
    .unwrap();
    assert_eq!((res.as_deref(), exec), (Some("erro:sem confirmacao"), true));

    // Expirados sem entrega.
    let expirados = comandos::expirar(db).await.unwrap();
    assert_eq!(expirados, vec![false]);

    // Confirmar comando nunca entregue não vale.
    let virgem = comandos::inserir(db, &novo("c9", 5, futuro)).await.unwrap();
    assert_eq!(
        comandos::confirmar(db, virgem, "ok").await.unwrap(),
        Confirmacao::NaoReservado
    );
    // Reserva (por "relay-9") e devolução do consumidor: volta à fila contando tentativa.
    let _ = comandos::reservar(db, "c9", &Reserva::nova("relay-9"))
        .await
        .unwrap();
    assert_eq!(
        comandos::devolver_do_consumidor(db, "relay-9")
            .await
            .unwrap(),
        1
    );
    let de_novo = comandos::reservar(db, "c9", &Reserva::nova("relay-9"))
        .await
        .unwrap();
    assert!(de_novo[0].repetido);
    // Devolver sem contar tentativa (envio que falhou).
    assert_eq!(comandos::devolver(db, &[virgem]).await.unwrap(), 1);
    let t: i32 = sqlx::query_scalar("SELECT tentativas FROM nexus.comando WHERE id = $1")
        .bind(virgem)
        .fetch_one(db)
        .await
        .unwrap();
    assert_eq!(t, 1);

    // Espelho: o histórico existente não é copiado no primeiro ciclo; depois entram só as novas.
    sqlx::query(
        "INSERT INTO public.comando_fila (nick, cmd, params, ts_expira, origem)
         VALUES ('antigo', 'reload', '{}', 4000000000000, 'manual')",
    )
    .execute(db)
    .await
    .unwrap();
    let mut marca = None;
    assert_eq!(comandos::espelhar(db, 500, &mut marca).await.unwrap().1, 0);
    assert!(marca.is_some());
    sqlx::query(
        "INSERT INTO public.comando_fila (nick, cmd, params, ts_expira, ts_executado, resultado, origem)
         VALUES (' Espelho ', 'reload', '{}', 4000000000000, NULL, NULL, 'ia_llm'),
                ('espelho', 'config', '{}', 4000000000000, 1700000000000, 'ok', 'manual')",
    )
    .execute(db)
    .await
    .unwrap();
    assert_eq!(comandos::espelhar(db, 500, &mut marca).await.unwrap().1, 2);
    assert_eq!(comandos::espelhar(db, 500, &mut marca).await.unwrap().1, 0);
    // Commit fora de ordem: a linha de id menor aparece depois da de id maior.
    let m = marca.unwrap().1;
    for id in [m + 10, m + 5] {
        sqlx::query(
            "INSERT INTO public.comando_fila (id, nick, cmd, params, ts_expira, origem)
             OVERRIDING SYSTEM VALUE VALUES ($1::int, 'tardio', 'a', '{}', 4000000000000, 'm')",
        )
        .bind(id)
        .execute(db)
        .await
        .unwrap();
        assert_eq!(comandos::espelhar(db, 500, &mut marca).await.unwrap().1, 1);
    }
    sqlx::query("UPDATE nexus.comando SET executado_em = now() WHERE conta = 'tardio'")
        .execute(db)
        .await
        .unwrap();
    let pend = comandos::reservar(db, "espelho", &Reserva::nova("sombra"))
        .await
        .unwrap();
    assert_eq!(pend.len(), 1);
    assert_eq!(
        (pend[0].tipo.as_str(), pend[0].origem.as_str()),
        ("reload", "espelho")
    );
    assert_eq!(
        comandos::confirmar(db, pend[0].id, "sombra").await.unwrap(),
        Confirmacao::Gravada
    );
}

/// Estado de raros (merge de deltas, transação) e tentativas de boss idempotentes.
async fn fase3_raros_e_tentativas(db: &PgPool) {
    use persistencia::equipes::{registrar_tentativa, LinhaTentativa};
    use persistencia::raros;

    assert_eq!(raros::carregar(db).await.unwrap(), json!({}));
    let lote = |bolas: i64| {
        json!({
            "raros": {"itens": [{"id": 1, "nome": "X", "qtd": 2}], "porConta": []},
            "historico": {"dia": {"Ana": {"2026-11-01": {"bolas": bolas, "shiny": 0, "kills": bolas}}}},
        })
    };
    let agora = 1_800_000_000_000_i64;
    for bolas in [100, 130, 20] {
        let n = raros::modificar(db, |r| nucleo::raros::ingerir(r, &lote(bolas), agora))
            .await
            .unwrap();
        assert_eq!(n, Ok(1));
    }
    // 100, depois +30, depois o contador reiniciou (20 < 130): soma 20.
    let r = raros::carregar(db).await.unwrap();
    assert_eq!(r["acc"]["ana"]["dias"]["2026-11-01"]["bolas"], json!(150));
    assert_eq!(r["acc"]["ana"]["nome"], json!("Ana"));

    // Corpo inválido: erro e nada muda.
    let ruim = raros::modificar(db, |r| {
        nucleo::raros::ingerir(r, &json!({"raros": {}}), agora)
    })
    .await
    .unwrap();
    assert_eq!(ruim, Err("raros inválido"));
    assert_eq!(raros::carregar(db).await.unwrap(), r);

    let v = raros::modificar(db, |r| {
        nucleo::raros::registrar_shiny(
            r,
            &json!({"nick": "Ana", "dia": "2026-11-01", "shiny": 1, "bolas": 5}),
            "2026-11-02",
        )
    })
    .await
    .unwrap()
    .unwrap();
    assert_eq!(v["bolas"], json!(155));
    assert_eq!(v["shiny"], json!(1));
    let r = raros::carregar(db).await.unwrap();
    let resposta = nucleo::raros::shiny(&r, "", None);
    assert_eq!(resposta["vazio"], json!(false));
    assert_eq!(resposta["contas"][0]["shinies"], json!(1));

    // Sem a tabela do legado populada, o cofre vem vazio e o cap cai no padrão.
    assert!(raros::historico_cofre(db, 0).await.is_empty());
    assert!(raros::captura_shiny(db).await.is_none());

    // Tentativas: a mesma chave não grava duas vezes; sem chave, sempre grava.
    let linha = |chave: Option<&str>| LinhaTentativa {
        em: 1_800_000_000_000,
        conta: Some("ana".into()),
        boss: "articuno".into(),
        nivel_boss: 300.0,
        tipos_boss: vec!["ICE".into()],
        venceu: true,
        duracao_s: 12.0,
        equipe: json!([{"nome": "A"}]),
        dano_causado: None,
        dano_recebido: None,
        eventos: None,
        chave: chave.map(str::to_owned),
    };
    assert!(registrar_tentativa(db, &linha(Some("k1"))).await.unwrap());
    assert!(!registrar_tentativa(db, &linha(Some("k1"))).await.unwrap());
    assert!(registrar_tentativa(db, &linha(Some("k2"))).await.unwrap());
    assert!(registrar_tentativa(db, &linha(None)).await.unwrap());
    assert!(registrar_tentativa(db, &linha(None)).await.unwrap());
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM nexus.boss_tentativa")
        .fetch_one(db)
        .await
        .unwrap();
    assert_eq!(n, 4);
}
