//! Testes do WebSocket contra o servidor de verdade (axum numa porta local) e um Postgres
//! TimescaleDB em container descartável. Precisa de Docker.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use contrato::ws::{
    ComandoResultado, Envelope, MensagemCliente, MensagemServidor, Retomar, SUBPROTOCOLO,
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
    sqlx::migrate!("../../migrations").run(&pool).await.unwrap();
    (container, pool)
}

struct Servidor {
    addr: SocketAddr,
    estado: Estado,
    parar: CancellationToken,
}

/// Sobe a API numa porta livre. `ligado`: NEXUS_ESCRITA_COMANDOS (com o listener).
async fn servir(db: &PgPool, ligado: bool) -> Servidor {
    let parar = CancellationToken::new();
    let mut estado = Estado::com_pool(db.clone());
    estado.escrita_comandos = ligado;
    estado.escrita_ingestao = ligado;
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
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let req = format!(
        "POST {caminho} HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{corpo}",
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
