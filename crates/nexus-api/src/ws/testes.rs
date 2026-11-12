//! Testes do WebSocket contra o servidor de verdade (axum numa porta local) e um Postgres
//! TimescaleDB em container descartável. Precisa de Docker.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use contrato::compressao;
use contrato::ws::{
    ComandoResultado, Envelope, EventoLote, MensagemCliente, MensagemServidor, Retomar,
    SUBPROTOCOLO, SUBPROTOCOLO_DEFLATE,
};
use futures_util::{SinkExt, StreamExt};
use persistencia::comandos::{inserir, NovoComando};
use serde_json::json;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use testcontainers::core::{IntoContainerPort, WaitFor};
use testcontainers::runners::AsyncRunner;
use testcontainers::{ContainerAsync, GenericImage, ImageExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use tokio_util::sync::CancellationToken;

use super::{iniciar_listener, Hub};
use crate::auth::{criar_token, Escopo};
use crate::bolsa::agora_ms;
use crate::comandos;
use crate::estado::Estado;

type Cli = WebSocketStream<MaybeTlsStream<TcpStream>>;

async fn banco() -> (ContainerAsync<GenericImage>, PgPool) {
    let container = GenericImage::new("timescale/timescaledb-ha", "pg16")
        .with_exposed_port(5432.tcp())
        .with_wait_for(WaitFor::message_on_stderr(
            "database system is ready to accept connections",
        ))
        .with_env_var("POSTGRES_PASSWORD", "teste")
        .start()
        .await
        .unwrap();
    let porta = container.get_host_port_ipv4(5432).await.unwrap();
    let url = format!("postgres://postgres:teste@127.0.0.1:{porta}/postgres");
    let mut tentativas = 0;
    let pool = loop {
        match PgPoolOptions::new().max_connections(8).connect(&url).await {
            Ok(p) if sqlx::query("SELECT 1").execute(&p).await.is_ok() => break p,
            _ if tentativas < 120 => {
                tentativas += 1;
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            _ => panic!("Postgres de teste não respondeu"),
        }
    };
    sqlx::raw_sql(include_str!(
        "../../../../migrations-teste/0001_baseline_public.sql"
    ))
    .execute(&pool)
    .await
    .unwrap();
    // Como em produção: o role do Nexus tem search_path=nexus e a extensão mora em public.
    // Migration que chamar função do Timescale sem qualificar falha aqui.
    let opcoes = url
        .parse::<sqlx::postgres::PgConnectOptions>()
        .unwrap()
        .options([("search_path", "nexus")]);
    let pool_migracao = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(opcoes)
        .await
        .unwrap();
    sqlx::migrate!("../../migrations")
        .run(&pool_migracao)
        .await
        .unwrap();
    pool_migracao.close().await;
    (container, pool)
}

struct Servidor {
    addr: SocketAddr,
    estado: Estado,
    parar: CancellationToken,
}

/// Sobe a API numa porta livre. `ligado`: NEXUS_ESCRITA_COMANDOS (com o listener).
async fn servir(db: &PgPool, ligado: bool) -> Servidor {
    servir_com(db, ligado, false, (50.0, 200.0)).await
}

/// `melhorias`: NEXUS_MELHORIAS; `limite`: (por segundo, rajada) da ingestão.
async fn servir_com(db: &PgPool, ligado: bool, melhorias: bool, limite: (f64, f64)) -> Servidor {
    servir_completo(db, ligado, ligado, melhorias, limite).await
}

/// `ingestao`: NEXUS_ESCRITA_INGESTAO, separada da fila de comandos.
async fn servir_completo(
    db: &PgPool,
    ligado: bool,
    ingestao: bool,
    melhorias: bool,
    limite: (f64, f64),
) -> Servidor {
    let parar = CancellationToken::new();
    let mut estado = Estado::com_pool(db.clone());
    estado.melhorias = melhorias;
    estado.escrita_raros = ligado;
    estado.limite_ingestao = Arc::new(crate::limite::Limitador::novo(limite.0, limite.1));
    estado.escrita_comandos = ligado;
    estado.escrita_ingestao = ingestao;
    estado.ws = Arc::new(Hub::novo(parar.clone()));
    if ligado {
        iniciar_listener(estado.clone(), parar.clone());
    }
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    let app = crate::rotas::router(estado.clone());
    let p = parar.clone();
    tokio::spawn(async move {
        axum::serve(l, app)
            .with_graceful_shutdown(p.cancelled_owned())
            .await
    });
    Servidor {
        addr,
        estado,
        parar,
    }
}

async fn conectar(
    addr: SocketAddr,
    token: Option<&str>,
    sub: bool,
) -> Result<Cli, tokio_tungstenite::tungstenite::Error> {
    let mut req = format!("ws://{addr}/ws").into_client_request().unwrap();
    if let Some(t) = token {
        req.headers_mut()
            .insert("Authorization", format!("Bearer {t}").parse().unwrap());
    }
    if sub {
        req.headers_mut()
            .insert("Sec-WebSocket-Protocol", SUBPROTOCOLO.parse().unwrap());
    }
    tokio_tungstenite::connect_async(req).await.map(|(c, _)| c)
}

async fn conectar_ok(s: &Servidor, token: &str) -> Cli {
    conectar(s.addr, Some(token), true).await.unwrap()
}

/// Próxima mensagem do servidor (ignora ping/pong); `None` se nada chegar em `espera`.
async fn tentar(c: &mut Cli, espera: Duration) -> Option<Envelope<MensagemServidor>> {
    let fim = tokio::time::Instant::now() + espera;
    loop {
        let m = tokio::time::timeout_at(fim, c.next()).await.ok()?;
        match m.unwrap().unwrap() {
            Message::Text(t) => return Some(serde_json::from_str(t.as_str()).unwrap()),
            Message::Close(f) => panic!("fechou: {f:?}"),
            _ => {}
        }
    }
}

async fn proxima(c: &mut Cli) -> Envelope<MensagemServidor> {
    tentar(c, Duration::from_secs(10)).await.expect("timeout")
}

async fn enviar(c: &mut Cli, msg: MensagemCliente) {
    let env = Envelope::novo(None, agora_ms(), msg);
    c.send(Message::text(serde_json::to_string(&env).unwrap()))
        .await
        .unwrap();
}

fn id_do_novo(e: &Envelope<MensagemServidor>) -> i64 {
    match &e.msg {
        MensagemServidor::ComandoNovo(c) => c.id,
        outro => panic!("esperava comando.novo, veio {outro:?}"),
    }
}

async fn novo_cmd(db: &PgPool, conta: &str, expira_ms: i64) -> i64 {
    inserir(
        db,
        &NovoComando {
            conta,
            tipo: "hunt",
            params: &json!({"slug": "x"}),
            prioridade: 5,
            origem: "teste",
            expira_ms,
        },
    )
    .await
    .unwrap()
}

async fn estado_do_cmd(db: &PgPool, id: i64) -> (bool, Option<String>) {
    sqlx::query_as("SELECT executado_em IS NOT NULL, resultado FROM nexus.comando WHERE id = $1")
        .bind(id)
        .fetch_one(db)
        .await
        .unwrap()
}

async fn esperar_executado(db: &PgPool, id: i64) {
    for _ in 0..100 {
        if estado_do_cmd(db, id).await.0 {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("comando {id} não foi confirmado");
}

/// Roda o corpo do teste com limite de tempo. O container fica aqui fora: é derrubado ao
/// fim mesmo se o corpo travar (estourar o prazo) ou entrar em pânico.
async fn com_limite<F, Fut>(corpo: F)
where
    F: FnOnce(PgPool) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    let (_container, db) = banco().await;
    tokio::time::timeout(Duration::from_secs(90), corpo(db))
        .await
        .expect("teste do WebSocket travou (90 s)");
}

async fn acesso_401_403_e_503_corpo(db: PgPool) {
    let ligado = servir(&db, true).await;
    let desligado = servir(&db, false).await;
    let t_relay = criar_token(&db, "r", &[Escopo::Relay], None).await.unwrap();
    let t_leitura = criar_token(&db, "l", &[Escopo::Leitura], None)
        .await
        .unwrap();

    let status = |e: tokio_tungstenite::tungstenite::Error| match e {
        tokio_tungstenite::tungstenite::Error::Http(r) => r.status().as_u16(),
        outro => panic!("{outro:?}"),
    };
    assert_eq!(
        status(conectar(ligado.addr, None, true).await.unwrap_err()),
        401
    );
    assert_eq!(
        status(
            conectar(ligado.addr, Some("nx_invalido"), true)
                .await
                .unwrap_err()
        ),
        401
    );
    assert_eq!(
        status(
            conectar(ligado.addr, Some(&t_leitura), true)
                .await
                .unwrap_err()
        ),
        403
    );
    // Sem o subprotocolo nexus.v1 não abre.
    assert_eq!(
        status(
            conectar(ligado.addr, Some(&t_relay), false)
                .await
                .unwrap_err()
        ),
        400
    );
    // Flags desligadas: 503 (com token válido).
    assert_eq!(
        status(
            conectar(desligado.addr, Some(&t_relay), true)
                .await
                .unwrap_err()
        ),
        503
    );
    // E com tudo certo, abre e manda sessao.pronta.
    let mut c = conectar_ok(&ligado, &t_relay).await;
    assert!(matches!(
        proxima(&mut c).await.msg,
        MensagemServidor::SessaoPronta(_)
    ));
    ligado.parar.cancel();
    desligado.parar.cancel();
}

async fn fluxo_de_comandos_corpo(db: PgPool) {
    let s = servir(&db, true).await;
    let t_ash = criar_token(&db, "relay-ash", &[Escopo::Relay], Some("ash"))
        .await
        .unwrap();
    let t_global = criar_token(&db, "relay-global", &[Escopo::Relay], None)
        .await
        .unwrap();
    let futuro = agora_ms() + 3_600_000;

    // Token de conta só recebe comandos da própria conta.
    let mut c1 = conectar_ok(&s, &t_ash).await;
    let pronta = proxima(&mut c1).await;
    assert!(matches!(pronta.msg, MensagemServidor::SessaoPronta(_)));
    let misty = novo_cmd(&db, "misty", futuro).await;
    let a = novo_cmd(&db, "ash", futuro).await;
    let env_a = proxima(&mut c1).await;
    assert_eq!(id_do_novo(&env_a), a);
    match &env_a.msg {
        MensagemServidor::ComandoNovo(c) => {
            assert_eq!(c.conta, "ash");
            assert_eq!(c.tipo, "hunt");
            assert_eq!(c.params, json!({"slug": "x"}));
            assert_eq!(c.prioridade, 5);
            assert_eq!(c.expira_em, Some(futuro));
        }
        _ => unreachable!(),
    }
    assert!(env_a.seq.is_some());
    assert!(tentar(&mut c1, Duration::from_millis(400)).await.is_none());

    // Token global recebe o que estava pendente de todas as contas.
    let mut g = conectar_ok(&s, &t_global).await;
    assert!(matches!(
        proxima(&mut g).await.msg,
        MensagemServidor::SessaoPronta(_)
    ));
    assert_eq!(id_do_novo(&proxima(&mut g).await), misty);
    drop(g);

    // Confirmação pelo WS; a repetida não muda nada.
    let confirmar = |id, r: &str| {
        MensagemCliente::ComandoResultado(ComandoResultado {
            id,
            resultado: r.into(),
        })
    };
    enviar(&mut c1, confirmar(a, "ok")).await;
    esperar_executado(&db, a).await;
    enviar(&mut c1, confirmar(a, "outro")).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(estado_do_cmd(&db, a).await, (true, Some("ok".into())));
    // Token de conta não confirma comando de outra conta.
    enviar(&mut c1, confirmar(misty, "invasao")).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_ne!(estado_do_cmd(&db, misty).await.1, Some("invasao".into()));

    // B chega e não é confirmado; a conexão cai.
    let b = novo_cmd(&db, "ash", futuro).await;
    let env_b = proxima(&mut c1).await;
    assert_eq!(id_do_novo(&env_b), b);
    let ultimo_seq = env_b.seq.unwrap();
    drop(c1);
    // Com o cliente fora: um comando válido e um já vencido.
    let c_novo = novo_cmd(&db, "ash", futuro).await;
    let vencido = novo_cmd(&db, "ash", 1_000_000_000_000).await;

    // Reconecta: B (reservado pela conexão anterior e não confirmado) volta na hora, e C chega.
    // A (executado) e o vencido não voltam. Retomar depois não entrega nada de novo.
    let mut c2 = conectar_ok(&s, &t_ash).await;
    assert!(matches!(
        proxima(&mut c2).await.msg,
        MensagemServidor::SessaoPronta(_)
    ));
    let mut ids = vec![
        id_do_novo(&proxima(&mut c2).await),
        id_do_novo(&proxima(&mut c2).await),
    ];
    ids.sort_unstable();
    assert_eq!(ids, vec![b, c_novo]);
    enviar(
        &mut c2,
        MensagemCliente::Retomar(Retomar { desde: ultimo_seq }),
    )
    .await;
    assert!(tentar(&mut c2, Duration::from_millis(500)).await.is_none());

    // Lease vencido: B volta à fila e é reentregue; o vencido vira `comando.cancelado`.
    sqlx::query(
        "UPDATE nexus.comando SET reservado_em = now() - interval '200 seconds' WHERE id = $1",
    )
    .bind(b)
    .execute(&db)
    .await
    .unwrap();
    comandos::manter(&s.estado, &mut None).await.unwrap();
    let mut recebidos = Vec::new();
    for _ in 0..2 {
        recebidos.push(proxima(&mut c2).await.msg);
    }
    assert!(recebidos
        .iter()
        .any(|m| matches!(m, MensagemServidor::ComandoNovo(c) if c.id == b)));
    assert!(recebidos.iter().any(|m| matches!(m,
        MensagemServidor::ComandoCancelado(c) if c.id == vencido && c.motivo == "expirado")));
    assert_eq!(estado_do_cmd(&db, vencido).await.1, Some("expirado".into()));
    // O vencido nunca foi entregue como comando.novo.
    assert!(!recebidos
        .iter()
        .any(|m| matches!(m, MensagemServidor::ComandoNovo(c) if c.id == vencido)));

    enviar(&mut c2, confirmar(b, "ok")).await;
    enviar(&mut c2, confirmar(c_novo, "ok")).await;
    esperar_executado(&db, b).await;
    esperar_executado(&db, c_novo).await;
    drop(c2);

    // Retomar com eventos informativos: reenvia o que passou de `desde`.
    let hub = Arc::clone(&s.estado.ws);
    let info = || MensagemServidor::BolsaAtualizada(contrato::ws::Atualizacao { em: agora_ms() });
    let mut c3 = conectar_ok(&s, &t_ash).await;
    proxima(&mut c3).await;
    hub.publicar_info(info());
    hub.publicar_info(info());
    hub.publicar_info(info());
    let primeiro = proxima(&mut c3).await.seq.unwrap();
    proxima(&mut c3).await;
    proxima(&mut c3).await;
    drop(c3);
    let mut c4 = conectar_ok(&s, &t_ash).await;
    proxima(&mut c4).await;
    enviar(
        &mut c4,
        MensagemCliente::Retomar(Retomar { desde: primeiro }),
    )
    .await;
    let r1 = proxima(&mut c4).await;
    let r2 = proxima(&mut c4).await;
    assert_eq!((r1.seq, r2.seq), (Some(primeiro + 1), Some(primeiro + 2)));
    assert!(matches!(r1.msg, MensagemServidor::BolsaAtualizada(_)));
    drop(c4);

    // Seq no futuro (servidor reiniciou) e seq fora do buffer: ressincronizar.
    for _ in 0..1005 {
        hub.publicar_info(info());
    }
    let mut c5 = conectar_ok(&s, &t_ash).await;
    proxima(&mut c5).await;
    let motivo = |m: MensagemServidor| match m {
        MensagemServidor::SessaoRessincronizar(r) => r.motivo,
        outro => panic!("{outro:?}"),
    };
    enviar(&mut c5, MensagemCliente::Retomar(Retomar { desde: 1 })).await;
    assert_eq!(motivo(proxima(&mut c5).await.msg), "fora_do_buffer");
    enviar(
        &mut c5,
        MensagemCliente::Retomar(Retomar { desde: 999_999 }),
    )
    .await;
    assert_eq!(motivo(proxima(&mut c5).await.msg), "seq_futuro");
    s.parar.cancel();
}

async fn cliente_lento_e_fechado_com_1013_corpo(db: PgPool) {
    let s = servir(&db, true).await;
    let t_lento = criar_token(&db, "lento", &[Escopo::Relay], Some("lenta"))
        .await
        .unwrap();
    let t_bom = criar_token(&db, "bom", &[Escopo::Relay], Some("boa"))
        .await
        .unwrap();
    let mut lento = conectar_ok(&s, &t_lento).await;
    let mut bom = conectar_ok(&s, &t_bom).await;
    assert!(matches!(
        proxima(&mut bom).await.msg,
        MensagemServidor::SessaoPronta(_)
    ));

    // 600 comandos de ~100 KB: bem mais que os buffers do kernel e a fila de 256. O cliente lento
    // não lê nada.
    sqlx::query(
        "INSERT INTO nexus.comando (conta, tipo, params, expira_em)
         SELECT 'lenta', 'hunt', jsonb_build_object('p', repeat('x', 100000)), now() + interval '1 hour'
         FROM generate_series(1, 600)",
    )
    .execute(&db)
    .await
    .unwrap();

    // Espera o servidor parar de entregar (a fila de saída encheu e ele fechou a conexão):
    // as reservas deixam de crescer. Só então o cliente começa a ler.
    // O servidor espera um pouco a fila esvaziar antes de desistir: as reservas precisam ficar
    // paradas por mais que essa espera (8 consultas de 500 ms).
    let (mut antes, mut parado) = (-1, 0);
    loop {
        tokio::time::sleep(Duration::from_millis(500)).await;
        let n: i64 =
            sqlx::query_scalar("SELECT count(*) FROM nexus.comando WHERE reservado_em IS NOT NULL")
                .fetch_one(&db)
                .await
                .unwrap();
        parado = if n == antes && n > 0 { parado + 1 } else { 0 };
        if parado >= 8 {
            assert!(n < 600, "entregou tudo sem encher a fila ({n})");
            break;
        }
        antes = n;
    }
    // Agora o cliente lê: vem o que já estava no caminho e, por fim, o fechamento 1013.
    let codigo = loop {
        let m = tokio::time::timeout(Duration::from_secs(60), lento.next())
            .await
            .expect("sem fechamento");
        match m {
            Some(Ok(Message::Close(Some(f)))) => break f.code,
            Some(Ok(_)) => {}
            outro => panic!("terminou sem Close: {outro:?}"),
        }
    };
    assert_eq!(codigo, CloseCode::Again);

    // O outro cliente segue normal.
    let id = novo_cmd(&db, "boa", agora_ms() + 60_000).await;
    assert_eq!(id_do_novo(&proxima(&mut bom).await), id);
    s.parar.cancel();
}

/// A8 e B1: confirmar exige comando entregue; token de conta não confirma comando alheio (HTTP).
async fn confirmacao_segura_corpo(db: PgPool) {
    let s = servir(&db, true).await;
    let t_ash = criar_token(&db, "ash", &[Escopo::Relay], Some("ash"))
        .await
        .unwrap();
    let futuro = agora_ms() + 3_600_000;
    let nunca = novo_cmd(&db, "ash", futuro).await;
    let alheio = novo_cmd(&db, "misty", futuro).await;
    // Marca o alheio como entregue, para que só o escopo o proteja.
    sqlx::query("UPDATE nexus.comando SET reservado_em = now(), reservado_por = 'x' WHERE id = $1")
        .bind(alheio)
        .execute(&db)
        .await
        .unwrap();
    let http = reqwest_post(s.addr, &t_ash);
    assert_eq!(http(alheio).await, 403);
    assert_eq!(estado_do_cmd(&db, alheio).await, (false, None));
    // Nunca entregue: responde 200 (contrato do legado), mas não grava.
    assert_eq!(http(nunca).await, 200);
    assert_eq!(estado_do_cmd(&db, nunca).await, (false, None));
    s.parar.cancel();
}

/// POST com Bearer e corpo JSON; devolve o status HTTP e o corpo da resposta.
async fn post(addr: SocketAddr, token: &str, caminho: &str, corpo: &str) -> (u16, String) {
    http(addr, "POST", token, caminho, "", corpo).await
}

async fn get(addr: SocketAddr, token: &str, caminho: &str) -> (u16, String) {
    http(addr, "GET", token, caminho, "", "").await
}

/// `extra`: cabeçalhos adicionais, cada um terminado em `\r\n`.
async fn http(
    addr: SocketAddr,
    metodo: &str,
    token: &str,
    caminho: &str,
    extra: &str,
    corpo: &str,
) -> (u16, String) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let req = format!(
        "{metodo} {caminho} HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer {token}\r\n{extra}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{corpo}",
        corpo.len()
    );
    let mut c = TcpStream::connect(addr).await.unwrap();
    c.write_all(req.as_bytes()).await.unwrap();
    let mut r = String::new();
    c.read_to_string(&mut r).await.unwrap();
    let status = r.split_whitespace().nth(1).unwrap().parse().unwrap();
    let corpo = r.split("\r\n\r\n").nth(1).unwrap_or_default().to_owned();
    (status, corpo)
}

fn reqwest_post(
    addr: SocketAddr,
    token: &str,
) -> impl Fn(i64) -> std::pin::Pin<Box<dyn std::future::Future<Output = u16> + Send>> {
    let token = token.to_owned();
    move |id| {
        let token = token.clone();
        Box::pin(async move {
            let corpo = format!("{{\"id\":{id},\"resultado\":\"ok\"}}");
            post(addr, &token, "/api/confirmar-comando", &corpo).await.0
        })
    }
}

/// C1, C2, C5 e C6: ingestão por conta, hunt-sync com nick fora do escopo e inserir-comando.
async fn ingestao_e_insercao_corpo(db: PgPool) {
    let s = servir(&db, true).await;
    let t_ash = criar_token(&db, "ash", &[Escopo::Relay], Some("ash"))
        .await
        .unwrap();
    let t_glob = criar_token(&db, "glob", &[Escopo::Relay], None)
        .await
        .unwrap();
    let ev = |conta_id: &str, nick: Option<&str>| {
        let n = nick.map_or(String::new(), |n| format!(",\"nick\":\"{n}\""));
        format!(
            "{{\"conta_id\":\"{conta_id}\"{n},\"eventos\":[{{\"k\":\"kill\",\"ts\":1800000000000,\"nome\":\"R\"}}]}}"
        )
    };
    // Token de conta: sem nick (conta global) ou com nick de outra conta, 403.
    assert_eq!(
        post(s.addr, &t_ash, "/ingest", &ev("X1", None)).await.0,
        403
    );
    assert_eq!(
        post(s.addr, &t_ash, "/ingest", &ev("X1", Some("Misty")))
            .await
            .0,
        403
    );
    assert_eq!(
        post(s.addr, &t_ash, "/ingest", &ev("X1", Some(" Ash ")))
            .await
            .0,
        200
    );
    // Token global sem nick: a conta é o conta_id.
    assert_eq!(
        post(s.addr, &t_glob, "/ingest", &ev("Y2", None)).await.0,
        200
    );
    let linhas: Vec<(String, Option<String>)> =
        sqlx::query_as("SELECT conta, conta_id FROM nexus.evento ORDER BY 1")
            .fetch_all(&db)
            .await
            .unwrap();
    assert_eq!(
        linhas,
        vec![
            ("ash".into(), Some("x1".into())),
            ("y2".into(), Some("y2".into()))
        ]
    );

    // hunt-sync: o nick fora do escopo é pulado, o lote não cai.
    let corpo = "{\"eventos\":[{\"nick\":\"ash\",\"hunt_slug\":\"a\"},{\"nick\":\"misty\",\"hunt_slug\":\"b\"}]}";
    let (st, resp) = post(s.addr, &t_ash, "/api/hunt-sync", corpo).await;
    assert_eq!(st, 200);
    assert!(resp.contains("\"atualizados\":1"), "{resp}");

    // inserir-comando: prazo fora de ±1440 é 400; params null é preservado.
    let ins = |extra: &str| format!("{{\"nick\":\"ash\",\"cmd\":\"x\"{extra}}}");
    assert_eq!(
        post(
            s.addr,
            &t_ash,
            "/api/inserir-comando",
            &ins(",\"ts_expira_min\":99999999999")
        )
        .await
        .0,
        200
    );
    assert_eq!(
        post(
            s.addr,
            &t_ash,
            "/api/inserir-comando",
            &ins(",\"ts_expira_min\":-1441")
        )
        .await
        .0,
        400
    );
    assert_eq!(
        post(
            s.addr,
            &t_ash,
            "/api/inserir-comando",
            &ins(",\"ts_expira_min\":1440")
        )
        .await
        .0,
        200
    );
    assert_eq!(
        post(
            s.addr,
            &t_ash,
            "/api/inserir-comando",
            &ins(",\"params\":null")
        )
        .await
        .0,
        200
    );
    let nulos: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM nexus.comando WHERE jsonb_typeof(params) = 'null'",
    )
    .fetch_one(&db)
    .await
    .unwrap();
    assert_eq!(nulos, 1);
    s.parar.cancel();
}

async fn lote(c: &mut Cli, seq: u64, conta: Option<&str>, evs: Vec<serde_json::Value>) {
    enviar(
        c,
        MensagemCliente::EventoLote(EventoLote {
            seq_cliente: seq,
            conta: conta.map(str::to_owned),
            eventos: evs,
        }),
    )
    .await;
}

async fn total_eventos(db: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM nexus.evento")
        .fetch_one(db)
        .await
        .unwrap()
}

/// Lote 2x = 1 gravação, ack correto, escopo de conta e limite de taxa.
async fn evento_lote_corpo(db: PgPool) {
    let s = servir_com(&db, true, true, (0.0, 5.0)).await;
    let t_ash = criar_token(&db, "ash", &[Escopo::Relay], Some("ash"))
        .await
        .unwrap();
    let mut c = conectar_ok(&s, &t_ash).await;
    assert!(matches!(
        proxima(&mut c).await.msg,
        MensagemServidor::SessaoPronta(_)
    ));
    let ev = vec![
        json!({"k": "kill", "ts": 1_800_000_000_000_i64, "nome": "R"}),
        json!({"k": "tipo_invalido"}),
    ];
    lote(&mut c, 1, None, ev.clone()).await;
    let MensagemServidor::EventoAck(a) = proxima(&mut c).await.msg else {
        panic!("esperava evento.ack")
    };
    assert_eq!(
        (a.seq_cliente, a.ok, a.duplicados, a.erros),
        (1, true, 0, 1)
    );
    assert_eq!(total_eventos(&db).await, 1);
    // Mesmo lote, mesmo seq_cliente: nada novo, 1 duplicado.
    lote(&mut c, 1, None, ev.clone()).await;
    let MensagemServidor::EventoAck(a) = proxima(&mut c).await.msg else {
        panic!("esperava evento.ack")
    };
    assert_eq!((a.duplicados, a.erros), (1, 1));
    assert_eq!(total_eventos(&db).await, 1);
    // Contador zerado (outro seq_cliente) e o mesmo evento por HTTP: continua 1 linha.
    lote(&mut c, 2, None, ev.clone()).await;
    let MensagemServidor::EventoAck(a) = proxima(&mut c).await.msg else {
        panic!("esperava evento.ack")
    };
    assert_eq!((a.seq_cliente, a.duplicados), (2, 1));
    let http = "{\"conta_id\":\"ash\",\"nick\":\"ash\",\"eventos\":[{\"k\":\"kill\",\"ts\":1800000000000,\"nome\":\"R\"}]}";
    assert_eq!(post(s.addr, &t_ash, "/ingest", http).await.0, 200);
    assert_eq!(total_eventos(&db).await, 1);
    // Conta de outro nick: erro, nada gravado.
    lote(&mut c, 3, Some("misty"), ev.clone()).await;
    let MensagemServidor::Erro(e) = proxima(&mut c).await.msg else {
        panic!("esperava erro")
    };
    assert_eq!(
        (e.motivo.as_str(), e.seq_cliente),
        ("conta_proibida", Some(3))
    );
    assert_eq!(total_eventos(&db).await, 1);
    // Rajada 5 (sem reposição), por token: 4 lotes WS + 1 POST gastaram tudo.
    lote(&mut c, 4, None, ev).await;
    let MensagemServidor::Erro(e) = proxima(&mut c).await.msg else {
        panic!("esperava erro")
    };
    assert_eq!(e.motivo, "limite_taxa");
    s.parar.cancel();

    // NEXUS_ESCRITA_INGESTAO desligada: `erro` e nada gravado.
    let s = servir_completo(&db, true, false, false, (50.0, 200.0)).await;
    let mut c = conectar_ok(&s, &t_ash).await;
    assert!(matches!(
        proxima(&mut c).await.msg,
        MensagemServidor::SessaoPronta(_)
    ));
    lote(
        &mut c,
        9,
        None,
        vec![json!({"k": "kill", "ts": 1_800_000_050_000_i64})],
    )
    .await;
    let MensagemServidor::Erro(e) = proxima(&mut c).await.msg else {
        panic!("esperava erro")
    };
    assert_eq!(e.motivo, "ingestao_desligada");
    assert_eq!(total_eventos(&db).await, 1);
    s.parar.cancel();
}

/// Push (NEXUS_MELHORIAS): `comando.estado` e `raro.novo`/`shiny.novo` respeitam a conta.
async fn push_ao_vivo_corpo(db: PgPool) {
    let s = servir_com(&db, true, true, (50.0, 200.0)).await;
    let t_ash = criar_token(&db, "ash", &[Escopo::Relay], Some("ash"))
        .await
        .unwrap();
    let t_misty = criar_token(&db, "misty", &[Escopo::Relay], Some("misty"))
        .await
        .unwrap();
    let t_glob = criar_token(&db, "glob", &[Escopo::Relay, Escopo::Escrita], None)
        .await
        .unwrap();
    let mut ash = conectar_ok(&s, &t_ash).await;
    let mut misty = conectar_ok(&s, &t_misty).await;
    let mut glob = conectar_ok(&s, &t_glob).await;
    for c in [&mut ash, &mut misty, &mut glob] {
        assert!(matches!(
            proxima(c).await.msg,
            MensagemServidor::SessaoPronta(_)
        ));
    }
    let id = novo_cmd(&db, "ash", 0).await;
    // ash recebe `comando.estado` criado (e o comando); misty não recebe nada de ash.
    let mut estados = Vec::new();
    for _ in 0..6 {
        match tentar(&mut ash, Duration::from_millis(800)).await {
            Some(Envelope {
                msg: MensagemServidor::ComandoEstado(e),
                ..
            }) => estados.push((e.id, e.estado)),
            Some(_) => {}
            None => break,
        }
    }
    assert!(estados.contains(&(id, "criado".to_string())), "{estados:?}");
    let ouviu_misty = loop {
        match tentar(&mut misty, Duration::from_millis(500)).await {
            Some(Envelope {
                msg: MensagemServidor::ComandoEstado(_),
                ..
            }) => break true,
            Some(_) => {}
            None => break false,
        }
    };
    assert!(!ouviu_misty);

    // Raros: o 1º ingest só carrega; o 2º traz o item novo, dono "ash".
    let corpo = |itens: &str| format!("{{\"raros\":{{\"itens\":{itens}}}}}");
    let (c1, _) = post(
        s.addr,
        &t_glob,
        "/api/raros/ingest",
        &corpo("[{\"id\":1,\"nome\":\"A\"}]"),
    )
    .await;
    assert_eq!(c1, 200);
    let (c2, _) = post(
        s.addr,
        &t_glob,
        "/api/raros/ingest",
        &corpo("[{\"id\":1,\"nome\":\"A\"},{\"id\":2,\"nome\":\"B\",\"contas\":{\"ash\":1}}]"),
    )
    .await;
    assert_eq!(c2, 200);
    let (c3, _) = post(
        s.addr,
        &t_glob,
        "/api/shiny/registrar",
        "{\"nick\":\"ash\",\"shiny\":1}",
    )
    .await;
    assert_eq!(c3, 200);
    let mut vistos_ash = Vec::new();
    while let Some(e) = tentar(&mut ash, Duration::from_millis(800)).await {
        vistos_ash.push(conta_tipo(&e.msg));
    }
    assert!(vistos_ash.contains(&"raro.novo"), "{vistos_ash:?}");
    assert!(vistos_ash.contains(&"shiny.novo"), "{vistos_ash:?}");
    let mut vistos_misty = Vec::new();
    while let Some(e) = tentar(&mut misty, Duration::from_millis(500)).await {
        vistos_misty.push(conta_tipo(&e.msg));
    }
    assert!(!vistos_misty.contains(&"raro.novo") && !vistos_misty.contains(&"shiny.novo"));
    let mut vistos_glob = Vec::new();
    while let Some(e) = tentar(&mut glob, Duration::from_millis(500)).await {
        vistos_glob.push(conta_tipo(&e.msg));
    }
    assert!(vistos_glob.contains(&"raro.novo"), "{vistos_glob:?}");
    s.parar.cancel();
}

fn conta_tipo(m: &MensagemServidor) -> &'static str {
    match m {
        MensagemServidor::RaroNovo(_) => "raro.novo",
        MensagemServidor::ShinyNovo(_) => "shiny.novo",
        MensagemServidor::ComandoEstado(_) => "comando.estado",
        _ => "outro",
    }
}

fn json_de(r: &(u16, String)) -> serde_json::Value {
    serde_json::from_str(&r.1).unwrap()
}

/// Idempotência, dead-letter, agendamento, histórico e limite de taxa HTTP.
async fn comandos_e_historico_corpo(db: PgPool) {
    let s = servir_com(&db, true, true, (0.0, 3.0)).await;
    let t_ash = criar_token(&db, "ash", &[Escopo::Relay], Some("ash"))
        .await
        .unwrap();
    let t_misty = criar_token(&db, "misty", &[Escopo::Relay], Some("misty"))
        .await
        .unwrap();
    let t_glob = criar_token(&db, "glob", &[Escopo::Relay, Escopo::Leitura], None)
        .await
        .unwrap();
    let ins = "{\"nick\":\"ash\",\"cmd\":\"hunt\"}";

    // 8. Idempotência: mesma chave = mesmo id; sem chave, ids novos; chave velha libera.
    let h = "Idempotency-Key: k1\r\n";
    let a = json_de(&http(s.addr, "POST", &t_glob, "/api/inserir-comando", h, ins).await);
    let b = json_de(&http(s.addr, "POST", &t_glob, "/api/inserir-comando", h, ins).await);
    assert_eq!(a["id"], b["id"]);
    assert_eq!(b["repetido"], json!(true));
    let c = json_de(&post(s.addr, &t_glob, "/api/inserir-comando", ins).await);
    let d = json_de(&post(s.addr, &t_glob, "/api/inserir-comando", ins).await);
    assert!(c["id"] != d["id"] && c["id"] != a["id"]);
    let por_campo = "{\"nick\":\"ash\",\"cmd\":\"hunt\",\"chave\":\"k2\"}";
    let e1 = json_de(&post(s.addr, &t_glob, "/api/inserir-comando", por_campo).await);
    let e2 = json_de(&post(s.addr, &t_glob, "/api/inserir-comando", por_campo).await);
    assert_eq!(e1["id"], e2["id"]);
    sqlx::query("UPDATE nexus.comando SET criado_em = now() - interval '25 hours' WHERE id = $1")
        .bind(a["id"].as_i64().unwrap())
        .execute(&db)
        .await
        .unwrap();
    let a2 = json_de(&http(s.addr, "POST", &t_glob, "/api/inserir-comando", h, ins).await);
    assert!(a2["id"] != a["id"]);
    sqlx::query("DELETE FROM nexus.comando")
        .execute(&db)
        .await
        .unwrap();

    // 9. Dead-letter: lista com escopo e reenvia.
    let id = novo_cmd(&db, "ash", agora_ms() + 600_000).await;
    sqlx::query(
        "UPDATE nexus.comando SET executado_em = now(), resultado = 'erro:sem confirmacao', tentativas = 5 WHERE id = $1",
    )
    .bind(id)
    .execute(&db)
    .await
    .unwrap();
    let f = json_de(&get(s.addr, &t_ash, "/api/comandos/falhos").await);
    assert_eq!(f["falhos"][0]["id"], json!(id));
    let f = json_de(&get(s.addr, &t_misty, "/api/comandos/falhos").await);
    assert_eq!(f["falhos"], json!([]));
    let caminho = format!("/api/comandos/{id}/reenviar");
    assert_eq!(post(s.addr, &t_misty, &caminho, "").await.0, 403);
    assert_eq!(post(s.addr, &t_ash, &caminho, "").await.0, 200);
    assert_eq!(post(s.addr, &t_ash, &caminho, "").await.0, 409);
    let (fim, tent): (bool, i32) = sqlx::query_as(
        "SELECT executado_em IS NOT NULL, tentativas FROM nexus.comando WHERE id = $1",
    )
    .bind(id)
    .fetch_one(&db)
    .await
    .unwrap();
    assert_eq!((fim, tent), (false, 0));
    sqlx::query("DELETE FROM nexus.comando")
        .execute(&db)
        .await
        .unwrap();

    // 10. Agendamento: não entrega antes da hora; ao chegar, avisa e entrega pelo WS.
    let mut ws = conectar_ok(&s, &t_ash).await;
    assert!(matches!(
        proxima(&mut ws).await.msg,
        MensagemServidor::SessaoPronta(_)
    ));
    let futuro = agora_ms() + 3_600_000;
    let corpo = format!("{{\"nick\":\"ash\",\"cmd\":\"hunt\",\"executar_em\":{futuro}}}");
    let ag = json_de(&post(s.addr, &t_glob, "/api/inserir-comando", &corpo).await);
    let agid = ag["id"].as_i64().unwrap();
    assert!(tentar(&mut ws, Duration::from_millis(800)).await.is_none());
    let http_lista = json_de(&get(s.addr, &t_ash, "/api/comandos?nick=ash").await);
    assert_eq!(http_lista["comandos"], json!([]));
    sqlx::query("UPDATE nexus.comando SET executar_em = now() - interval '1 second' WHERE id = $1")
        .bind(agid)
        .execute(&db)
        .await
        .unwrap();
    assert_eq!(
        persistencia::comandos::liberar_agendados(&db)
            .await
            .unwrap(),
        1
    );
    // Antes do `comando.novo` pode vir o `comando.estado` informativo.
    let mut recebido = proxima(&mut ws).await;
    while matches!(recebido.msg, MensagemServidor::ComandoEstado(_)) {
        recebido = proxima(&mut ws).await;
    }
    assert_eq!(id_do_novo(&recebido), agid);
    s.parar.cancel();

    // 11. Histórico com escopo (servidor novo: o limite de taxa anterior já se esgotou).
    let s = servir_com(&db, true, true, (50.0, 200.0)).await;
    let lote_ash = "{\"conta_id\":\"ash\",\"nick\":\"ash\",\"eventos\":[{\"k\":\"kill\",\"ts\":1800000000000,\"n\":1},{\"k\":\"kill\",\"ts\":1800000001000,\"n\":2}]}";
    let lote_misty = "{\"conta_id\":\"misty\",\"nick\":\"misty\",\"eventos\":[{\"k\":\"kill\",\"ts\":1800000000000,\"n\":1}]}";
    assert_eq!(post(s.addr, &t_ash, "/ingest", lote_ash).await.0, 200);
    assert_eq!(post(s.addr, &t_misty, "/ingest", lote_misty).await.0, 200);
    let bruto = json_de(&get(s.addr, &t_glob, "/api/eventos?tipo=kill").await);
    assert_eq!(bruto["eventos"].as_array().unwrap().len(), 3);
    let t_leitura_ash = criar_token(&db, "lash", &[Escopo::Leitura], Some("ash"))
        .await
        .unwrap();
    let so_ash = json_de(&get(s.addr, &t_leitura_ash, "/api/eventos").await);
    assert_eq!(so_ash["eventos"].as_array().unwrap().len(), 2);
    assert_eq!(
        get(s.addr, &t_leitura_ash, "/api/eventos?conta=misty")
            .await
            .0,
        403
    );
    let horas = json_de(&get(s.addr, &t_glob, "/api/eventos?agrupar=hora&conta=ash").await);
    assert_eq!(horas["horas"][0]["total"], json!(2));
    assert_eq!(
        get(s.addr, &t_glob, "/api/eventos?agrupar=dia").await.0,
        400
    );
    s.parar.cancel();

    // 12. Limite de taxa no HTTP: sem NEXUS_MELHORIAS nunca dá 429.
    let s = servir_com(&db, true, false, (0.0, 3.0)).await;
    let lote = "{\"conta_id\":\"ash\",\"nick\":\"ash\",\"eventos\":[{\"k\":\"kill\",\"ts\":1800000009000}]}";
    for _ in 0..5 {
        assert_eq!(post(s.addr, &t_ash, "/ingest", lote).await.0, 200);
    }
    s.parar.cancel();
    // Com NEXUS_MELHORIAS: rajada 3, sem reposição.
    let s = servir_com(&db, true, true, (0.0, 3.0)).await;
    let lote = "{\"conta_id\":\"ash\",\"nick\":\"ash\",\"eventos\":[{\"k\":\"kill\",\"ts\":1800000009000}]}";
    for _ in 0..3 {
        assert_eq!(post(s.addr, &t_ash, "/ingest", lote).await.0, 200);
    }
    assert_eq!(post(s.addr, &t_ash, "/ingest", lote).await.0, 429);
    s.parar.cancel();
}

#[tokio::test]
async fn comandos_e_historico() {
    com_limite(comandos_e_historico_corpo).await;
}

#[tokio::test]
async fn evento_lote() {
    com_limite(evento_lote_corpo).await;
}

#[tokio::test]
async fn push_ao_vivo() {
    com_limite(push_ao_vivo_corpo).await;
}

#[tokio::test]
async fn ingestao_e_insercao() {
    com_limite(ingestao_e_insercao_corpo).await;
}

#[tokio::test]
async fn confirmacao_segura() {
    com_limite(confirmacao_segura_corpo).await;
}

#[tokio::test]
async fn acesso_401_403_e_503() {
    com_limite(acesso_401_403_e_503_corpo).await;
}

#[tokio::test]
async fn fluxo_de_comandos() {
    com_limite(fluxo_de_comandos_corpo).await;
}

#[tokio::test]
async fn cliente_lento_e_fechado_com_1013() {
    com_limite(cliente_lento_e_fechado_com_1013_corpo).await;
}

/// Conecta oferecendo os subprotocolos dados; devolve o cliente e o que o servidor escolheu.
async fn conectar_sub(s: &Servidor, token: &str, oferta: &str) -> (Cli, String) {
    let mut req = format!("ws://{}/ws", s.addr).into_client_request().unwrap();
    req.headers_mut()
        .insert("Authorization", format!("Bearer {token}").parse().unwrap());
    req.headers_mut()
        .insert("Sec-WebSocket-Protocol", oferta.parse().unwrap());
    let (c, resp) = tokio_tungstenite::connect_async(req).await.unwrap();
    let escolhido = resp
        .headers()
        .get("Sec-WebSocket-Protocol")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    (c, escolhido)
}

/// Próximo frame de dados (ignora ping/pong).
async fn bruta(c: &mut Cli) -> Message {
    loop {
        let m = tokio::time::timeout(Duration::from_secs(10), c.next())
            .await
            .expect("timeout")
            .unwrap()
            .unwrap();
        if !matches!(m, Message::Ping(_) | Message::Pong(_)) {
            return m;
        }
    }
}

fn raro(nome: &str) -> MensagemServidor {
    MensagemServidor::RaroNovo(contrato::ws::RaroNovo {
        id: json!(1),
        nome: json!(nome),
        conta: None,
    })
}

async fn deflate_corpo(db: PgPool) {
    let s = servir_com(&db, true, true, (50.0, 200.0)).await;
    let t = criar_token(&db, "glob", &[Escopo::Relay, Escopo::Escrita], None)
        .await
        .unwrap();
    let grande = "x".repeat(5000);

    // Negociação: com a oferta dupla o servidor escolhe o deflate.
    let (mut c, escolhido) = conectar_sub(&s, &t, "nexus.v1.deflate, nexus.v1").await;
    assert_eq!(escolhido, SUBPROTOCOLO_DEFLATE);
    assert!(matches!(
        proxima(&mut c).await.msg,
        MensagemServidor::SessaoPronta(_)
    ));
    s.estado
        .ws
        .publicar_para(super::Destino::Global, raro(&grande));
    s.estado
        .ws
        .publicar_para(super::Destino::Global, raro("pequeno"));
    // Grande: frame binário deflate, íntegro.
    let Message::Binary(b) = bruta(&mut c).await else {
        panic!("esperava binário");
    };
    assert!(b.len() < 1024);
    let texto = compressao::descomprimir(&b, compressao::LIMITE_DESCOMPRIMIDO).unwrap();
    let env: Envelope<MensagemServidor> = serde_json::from_str(&texto).unwrap();
    assert_eq!(env.msg, raro(&grande));
    // Pequena: texto.
    let Message::Text(t2) = bruta(&mut c).await else {
        panic!("esperava texto");
    };
    assert!(t2.as_str().contains("pequeno"));

    // Cliente -> servidor em binário deflate: numa conexão nova (que abriu depois dos dois
    // pushes), `retomar` desde 0 reenvia ambos.
    let (mut c3, _) = conectar_sub(&s, &t, "nexus.v1.deflate").await;
    assert!(matches!(
        proxima(&mut c3).await.msg,
        MensagemServidor::SessaoPronta(_)
    ));
    let env = Envelope::novo(
        None,
        agora_ms(),
        MensagemCliente::Retomar(Retomar { desde: 0 }),
    );
    let bin = compressao::comprimir(&serde_json::to_string(&env).unwrap()).unwrap();
    c3.send(Message::binary(bin)).await.unwrap();
    assert!(matches!(bruta(&mut c3).await, Message::Binary(_)));
    assert!(matches!(bruta(&mut c3).await, Message::Text(_)));

    // Zip bomb: descomprimido passa do limite, a conexão é fechada.
    let bomba = compressao::comprimir(&"0".repeat(compressao::LIMITE_DESCOMPRIMIDO + 1)).unwrap();
    assert!(bomba.len() < 64 * 1024);
    c3.send(Message::binary(bomba)).await.unwrap();
    match bruta(&mut c3).await {
        Message::Close(Some(f)) => assert_eq!(u16::from(f.code), 1011),
        outro => panic!("esperava close, veio {outro:?}"),
    }

    // Sem o subprotocolo novo: tudo texto, como antes.
    let (mut c2, escolhido) = conectar_sub(&s, &t, "nexus.v1").await;
    assert_eq!(escolhido, SUBPROTOCOLO);
    assert!(matches!(
        proxima(&mut c2).await.msg,
        MensagemServidor::SessaoPronta(_)
    ));
    s.estado
        .ws
        .publicar_para(super::Destino::Global, raro(&grande));
    assert!(matches!(bruta(&mut c2).await, Message::Text(_)));
    s.parar.cancel();
}

#[tokio::test]
async fn deflate() {
    com_limite(deflate_corpo).await;
}

/// Proxy TCP que permite derrubar todas as conexões abertas, sem derrubar o servidor.
struct Proxy {
    addr: SocketAddr,
    corte: Arc<std::sync::Mutex<CancellationToken>>,
}

impl Proxy {
    async fn novo(destino: SocketAddr) -> Proxy {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        let corte = Arc::new(std::sync::Mutex::new(CancellationToken::new()));
        let c = corte.clone();
        tokio::spawn(async move {
            while let Ok((mut entrada, _)) = l.accept().await {
                let tok = c.lock().unwrap().clone();
                tokio::spawn(async move {
                    let Ok(mut saida) = TcpStream::connect(destino).await else {
                        return;
                    };
                    tokio::select! {
                        () = tok.cancelled() => {}
                        _ = tokio::io::copy_bidirectional(&mut entrada, &mut saida) => {}
                    }
                });
            }
        });
        Proxy { addr, corte }
    }

    fn derrubar(&self) {
        let mut g = self.corte.lock().unwrap();
        g.cancel();
        *g = CancellationToken::new();
    }
}

async fn esperar_sombra(db: &PgPool, ids: &[i64]) {
    for id in ids {
        esperar_executado(db, *id).await;
        let (_, r) = estado_do_cmd(db, *id).await;
        assert_eq!(r.as_deref(), Some("sombra"), "comando {id}");
    }
}

async fn cmd_espelho(db: &PgPool, expira_ms: i64) -> i64 {
    inserir(
        db,
        &NovoComando {
            conta: "conta1",
            tipo: "hunt",
            params: &json!({}),
            prioridade: 5,
            origem: "espelho",
            expira_ms,
        },
    )
    .await
    .unwrap()
}

async fn sombra_corpo(db: PgPool) {
    use crate::sombra::{relatorio, rodar, ConfigSombra, Contadores};
    use std::sync::atomic::Ordering;

    let s = servir(&db, true).await;
    let token = criar_token(&db, "sombra", &[Escopo::Relay], None)
        .await
        .unwrap();
    let proxy = Proxy::novo(s.addr).await;
    let cfg = ConfigSombra::nova(&format!("ws://{}/ws", proxy.addr), &token).unwrap();
    let cont = Arc::new(Contadores::default());
    let parar = CancellationToken::new();
    let tarefa = {
        let (c, p) = (cont.clone(), parar.clone());
        tokio::spawn(async move { rodar(&cfg, p, c).await })
    };
    let longe = agora_ms() + 600_000;

    // Comandos inseridos com a sombra conectada.
    tokio::time::sleep(Duration::from_millis(500)).await;
    let mut ids = Vec::new();
    for _ in 0..5 {
        ids.push(cmd_espelho(&db, longe).await);
    }
    esperar_sombra(&db, &ids).await;

    // Derruba a conexão; mais comandos entram enquanto ela está fora.
    proxy.derrubar();
    for _ in 0..5 {
        ids.push(cmd_espelho(&db, longe).await);
    }
    esperar_sombra(&db, &ids).await;

    assert_eq!(cont.duplicados.load(Ordering::Relaxed), 0);
    assert!(cont.reconexoes.load(Ordering::Relaxed) >= 1);
    let tent: i64 = sqlx::query_scalar("SELECT coalesce(sum(tentativas), 0) FROM nexus.comando")
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(tent, 0, "nenhuma redistribuição esperada");

    // SIGTERM: encerra limpo.
    parar.cancel();
    tokio::time::timeout(Duration::from_secs(5), tarefa)
        .await
        .expect("sombra não encerrou")
        .unwrap();

    // Um comando que expira sem ninguém para receber vira "perdido".
    cmd_espelho(&db, agora_ms() + 200).await;
    tokio::time::sleep(Duration::from_millis(400)).await;
    persistencia::comandos::expirar(&db).await.unwrap();

    let r = relatorio(&db, 24, "espelho").await.unwrap();
    assert_eq!(r.total, 11);
    assert_eq!(r.entregues, 10);
    assert_eq!(r.confirmados, 10);
    assert_eq!(r.perdidos, 1);
    assert_eq!(r.duplicados, 0);
    assert_eq!(r.sem_confirmacao, 0);
    assert!(r.p50_s.is_some() && r.p95_s.is_some());
    assert!(!r.aprovado());
    s.parar.cancel();
}

async fn sombra_gerador_corpo(db: PgPool) {
    use crate::sombra::{gerar, relatorio, rodar, ConfigSombra, Contadores};

    let s = servir(&db, true).await;
    let token = criar_token(&db, "sombra", &[Escopo::Relay], None)
        .await
        .unwrap();
    let cfg = ConfigSombra::nova(&format!("ws://{}/ws", s.addr), &token).unwrap();
    let cont = Arc::new(Contadores::default());
    let parar = CancellationToken::new();
    {
        let (c, p) = (cont.clone(), parar.clone());
        tokio::spawn(async move { rodar(&cfg, p, c).await });
    }
    tokio::spawn(gerar(db.clone(), Duration::from_secs(1), parar.clone()));
    tokio::time::sleep(Duration::from_secs(5)).await;
    parar.cancel();

    let r = relatorio(&db, 24, "sintetico").await.unwrap();
    assert!(r.total >= 3, "total {}", r.total);
    assert!(r.confirmados >= 3, "confirmados {}", r.confirmados);
    assert_eq!(r.duplicados, 0);
    let e = relatorio(&db, 24, "espelho").await.unwrap();
    assert_eq!(e.total, 0);
    s.parar.cancel();
}

#[tokio::test]
async fn sombra_gerador_sintetico() {
    com_limite(sombra_gerador_corpo).await;
}

#[tokio::test]
async fn sombra_confirma_reconecta_e_relata() {
    com_limite(sombra_corpo).await;
}
