//! Fila de comandos (`nexus.comando`).
//!
//! O estado de um comando é derivado das colunas: pendente (sem `reservado_em` nem
//! `executado_em`), reservado (com `reservado_em`) ou encerrado (com `executado_em`). A
//! reserva usa `FOR UPDATE SKIP LOCKED`: dois consumidores nunca pegam o mesmo comando.
//! Reservado e não confirmado dentro do prazo (lease) volta a pendente; depois de
//! [`MAX_TENTATIVAS`] devoluções, o comando é encerrado com erro.
//!
//! O WebSocket e as rotas HTTP usam as mesmas funções, então a entrega é igual nos dois.

use serde_json::Value;
use sqlx::{PgPool, Row};

/// Depois de tantas reservas sem confirmação, o comando sai da fila com erro.
pub const MAX_TENTATIVAS: i32 = 5;
/// Prazo padrão para confirmar um comando reservado.
pub const LEASE_PADRAO_S: i64 = 120;
/// Quantos comandos uma consulta reserva de uma vez (como o legado).
pub const LIMITE_PADRAO: i64 = 10;
/// Resultado gravado quando o lease estoura pela última vez.
pub const RESULTADO_SEM_CONFIRMACAO: &str = "erro:sem confirmacao";

/// Um comando entregue a um relay.
#[derive(Debug, Clone, PartialEq)]
pub struct Comando {
    pub id: i64,
    pub conta: String,
    pub tipo: String,
    pub params: Value,
    pub prioridade: i32,
    pub origem: String,
    /// Instante de expiração em ms, se o comando expira.
    pub expira_ms: Option<i64>,
    /// Esta entrega repete uma anterior ao mesmo consumidor (a primeira não foi confirmada).
    pub repetido: bool,
}

/// Como reservar.
#[derive(Debug, Clone, Copy)]
pub struct Reserva<'a> {
    /// Quem está reservando (o relay ou a conexão).
    pub por: &'a str,
    pub lease_s: i64,
    pub limite: i64,
    /// `true`: maior prioridade primeiro. `false`: menor número primeiro, como o legado.
    pub prioridade_desc: bool,
}

impl<'a> Reserva<'a> {
    pub fn nova(por: &'a str) -> Self {
        Reserva {
            por,
            lease_s: LEASE_PADRAO_S,
            limite: LIMITE_PADRAO,
            prioridade_desc: false,
        }
    }
}

/// O que aconteceu numa confirmação.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confirmacao {
    /// Primeira confirmação: gravou.
    Gravada,
    /// O comando já estava encerrado: nada mudou.
    Repetida,
    /// Não existe comando com esse id.
    Inexistente,
    /// O comando existe, mas nunca foi entregue (não está reservado): não pode ser confirmado.
    NaoReservado,
}

/// Um comando novo para a fila.
#[derive(Debug, Clone)]
pub struct NovoComando<'a> {
    pub conta: &'a str,
    pub tipo: &'a str,
    pub params: &'a Value,
    pub prioridade: i32,
    pub origem: &'a str,
    /// Instante de expiração, em ms desde 1970 (pode estar no passado).
    pub expira_ms: i64,
}

pub async fn inserir(db: &PgPool, c: &NovoComando<'_>) -> Result<i64, sqlx::Error> {
    inserir_ext(db, c, None, None).await.map(|(id, _)| id)
}

/// Insere com chave de idempotência e/ou agendamento. Devolve `(id, repetido)`: com a mesma
/// `chave` e conta nas últimas 24 h, não insere de novo e devolve o id já criado. Chave mais
/// velha que a janela é liberada e o comando entra como novo. `executar_ms`: instante (ms)
/// antes do qual o comando não é entregue.
pub async fn inserir_ext(
    db: &PgPool,
    c: &NovoComando<'_>,
    chave: Option<&str>,
    executar_ms: Option<i64>,
) -> Result<(i64, bool), sqlx::Error> {
    // Se a chave for liberada por outra transação entre o conflito e a leitura, tenta o
    // INSERT uma vez mais em vez de falhar.
    for _ in 0..2 {
        if let Some(r) = tentar_inserir(db, c, chave, executar_ms).await? {
            return Ok(r);
        }
    }
    Err(sqlx::Error::RowNotFound)
}

async fn tentar_inserir(
    db: &PgPool,
    c: &NovoComando<'_>,
    chave: Option<&str>,
    executar_ms: Option<i64>,
) -> Result<Option<(i64, bool)>, sqlx::Error> {
    let mut tx = db.begin().await?;
    if let Some(k) = chave {
        // Chave fora da janela de 24 h: libera para o índice único aceitar a nova.
        sqlx::query(
            "UPDATE nexus.comando SET chave_idem = NULL
             WHERE conta = $1 AND chave_idem = $2
               AND criado_em <= now() - interval '24 hours'",
        )
        .bind(c.conta)
        .bind(k)
        .execute(&mut *tx)
        .await?;
    }
    let novo: Option<i64> = sqlx::query_scalar(
        "INSERT INTO nexus.comando
             (conta, tipo, params, prioridade, origem, expira_em, chave_idem,
              executar_em, agendado_avisado)
         VALUES ($1, $2, $3, $4, $5, to_timestamp($6 / 1000.0), $7,
                 to_timestamp($8 / 1000.0), $8::bigint IS NULL)
         ON CONFLICT (conta, chave_idem) WHERE chave_idem IS NOT NULL DO NOTHING
         RETURNING id",
    )
    .bind(c.conta)
    .bind(c.tipo)
    .bind(c.params)
    .bind(c.prioridade)
    .bind(c.origem)
    .bind(c.expira_ms)
    .bind(chave)
    .bind(executar_ms)
    .fetch_optional(&mut *tx)
    .await?;
    let resultado = match (novo, chave) {
        (Some(id), _) => (id, false),
        (None, Some(k)) => {
            let id: Option<i64> = sqlx::query_scalar(
                "SELECT id FROM nexus.comando WHERE conta = $1 AND chave_idem = $2",
            )
            .bind(c.conta)
            .bind(k)
            .fetch_optional(&mut *tx)
            .await?;
            match id {
                Some(id) => (id, true),
                None => return Ok(None),
            }
        }
        (None, None) => return Ok(None),
    };
    tx.commit().await?;
    Ok(Some(resultado))
}

/// Marca como liberados os agendados cuja hora chegou; o trigger avisa como comando novo.
/// Devolve quantos foram liberados.
pub async fn liberar_agendados(db: &PgPool) -> Result<u64, sqlx::Error> {
    sqlx::query(
        "UPDATE nexus.comando SET agendado_avisado = true
         WHERE NOT agendado_avisado AND executar_em <= now()",
    )
    .execute(db)
    .await
    .map(|r| r.rows_affected())
}

/// Um comando que esgotou as tentativas (dead-letter).
#[derive(Debug, Clone, PartialEq)]
pub struct Falho {
    pub id: i64,
    pub conta: String,
    pub tipo: String,
    pub params: Value,
    pub origem: String,
    pub tentativas: i32,
    pub criado_ms: i64,
}

/// Lista os comandos encerrados por falta de confirmação (mais recentes primeiro). `contas`
/// vazio = todas.
pub async fn falhos(
    db: &PgPool,
    contas: &[String],
    limite: i64,
) -> Result<Vec<Falho>, sqlx::Error> {
    let linhas = sqlx::query(
        "SELECT id, conta, tipo, params, origem, tentativas,
                (extract(epoch FROM criado_em) * 1000)::bigint AS criado_ms
         FROM nexus.comando
         WHERE executado_em IS NOT NULL AND resultado = $3
           AND (cardinality($1::text[]) = 0 OR conta = ANY($1))
         ORDER BY id DESC LIMIT $2",
    )
    .bind(contas)
    .bind(limite)
    .bind(RESULTADO_SEM_CONFIRMACAO)
    .fetch_all(db)
    .await?;
    linhas
        .iter()
        .map(|l| {
            Ok(Falho {
                id: l.try_get("id")?,
                conta: l.try_get("conta")?,
                tipo: l.try_get("tipo")?,
                params: l.try_get("params")?,
                origem: l.try_get("origem")?,
                tentativas: l.try_get("tentativas")?,
                criado_ms: l.try_get("criado_ms")?,
            })
        })
        .collect()
}

/// Volta um comando falho para pendente (zera tentativas). Se o prazo já passou, ganha 5
/// minutos para não expirar na hora. `false` se o id não é um falho.
pub async fn reenviar(db: &PgPool, id: i64) -> Result<bool, sqlx::Error> {
    sqlx::query(
        "UPDATE nexus.comando SET
             executado_em = NULL, resultado = NULL, reservado_em = NULL, reservado_por = NULL,
             tentativas = 0,
             expira_em = CASE WHEN expira_em IS NOT NULL AND expira_em < now() + interval '5 minutes'
                              THEN now() + interval '5 minutes' ELSE expira_em END
         WHERE id = $1 AND executado_em IS NOT NULL AND resultado = $2",
    )
    .bind(id)
    .bind(RESULTADO_SEM_CONFIRMACAO)
    .execute(db)
    .await
    .map(|r| r.rows_affected() > 0)
}

/// Devolve à fila os comandos reservados cujo lease venceu e encerra os que esgotaram as
/// tentativas. Devolve quantos voltaram e quantos foram encerrados.
pub async fn devolver_vencidos(db: &PgPool, lease_s: i64) -> Result<(u64, u64), sqlx::Error> {
    let linhas = sqlx::query(
        "UPDATE nexus.comando SET
             tentativas = tentativas + 1,
             reservado_em = CASE WHEN tentativas + 1 >= $2 THEN reservado_em END,
             executado_em = CASE WHEN tentativas + 1 >= $2 THEN now() END,
             resultado = CASE WHEN tentativas + 1 >= $2 THEN $3 END
         WHERE executado_em IS NULL AND reservado_em IS NOT NULL
           AND reservado_em < now() - make_interval(secs => $1)
         RETURNING (executado_em IS NOT NULL) AS encerrado",
    )
    .bind(lease_s as f64)
    .bind(MAX_TENTATIVAS)
    .bind(RESULTADO_SEM_CONFIRMACAO)
    .fetch_all(db)
    .await?;
    let encerrados = linhas
        .iter()
        .filter(|l| l.try_get::<bool, _>("encerrado").unwrap_or(false))
        .count() as u64;
    Ok((linhas.len() as u64 - encerrados, encerrados))
}

/// Reserva os comandos pendentes de uma conta (nick já normalizado).
pub async fn reservar(
    db: &PgPool,
    conta: &str,
    r: &Reserva<'_>,
) -> Result<Vec<Comando>, sqlx::Error> {
    pendentes(db, &[conta.to_owned()], r).await
}

/// Reserva os comandos pendentes de várias contas (o que o WebSocket faz ao conectar).
/// Lista vazia significa todas as contas (token global).
/// Antes, devolve à fila os reservados com lease vencido.
pub async fn pendentes(
    db: &PgPool,
    contas: &[String],
    r: &Reserva<'_>,
) -> Result<Vec<Comando>, sqlx::Error> {
    devolver_vencidos(db, r.lease_s).await?;
    // O `ORDER BY` do `RETURNING` não é garantido: a ordem final é refeita abaixo.
    let linhas = sqlx::query(
        "WITH alvo AS (
             SELECT id, reservado_por AS antes, tentativas FROM nexus.comando
             WHERE (cardinality($1::text[]) = 0 OR conta = ANY($1))
               AND executado_em IS NULL AND reservado_em IS NULL
               AND (expira_em IS NULL OR expira_em > now())
               AND (executar_em IS NULL OR executar_em <= now())
             ORDER BY CASE WHEN $4 THEN -prioridade ELSE prioridade END, id
             LIMIT $3
             FOR UPDATE SKIP LOCKED)
         UPDATE nexus.comando c SET reservado_em = now(), reservado_por = $2
         FROM alvo WHERE c.id = alvo.id
         RETURNING c.id, c.conta, c.tipo, c.params, c.prioridade, c.origem,
                   (extract(epoch FROM c.expira_em) * 1000)::bigint AS expira_ms,
                   (alvo.antes = $2 AND alvo.tentativas > 0) AS repetido",
    )
    .bind(contas)
    .bind(r.por)
    .bind(r.limite)
    .bind(r.prioridade_desc)
    .fetch_all(db)
    .await?;
    let mut v = linhas
        .iter()
        .map(|l| {
            Ok(Comando {
                id: l.try_get("id")?,
                conta: l.try_get("conta")?,
                tipo: l.try_get("tipo")?,
                params: l.try_get("params")?,
                prioridade: l.try_get("prioridade")?,
                origem: l.try_get("origem")?,
                expira_ms: l.try_get("expira_ms")?,
                repetido: l.try_get::<Option<bool>, _>("repetido")?.unwrap_or(false),
            })
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()?;
    v.sort_by_key(|c| {
        (
            if r.prioridade_desc {
                -i64::from(c.prioridade)
            } else {
                i64::from(c.prioridade)
            },
            c.id,
        )
    });
    Ok(v)
}

/// A conta dona do comando (`None` se não existe).
pub async fn conta_de(db: &PgPool, id: i64) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar("SELECT conta FROM nexus.comando WHERE id = $1")
        .bind(id)
        .fetch_optional(db)
        .await
}

/// Confirma a execução. Idempotente: a segunda confirmação do mesmo id não muda nada. Só
/// confirma comando que foi entregue (reservado): um comando nunca entregue não pode ser dado
/// como executado.
pub async fn confirmar(db: &PgPool, id: i64, resultado: &str) -> Result<Confirmacao, sqlx::Error> {
    let gravou = sqlx::query(
        "UPDATE nexus.comando SET executado_em = now(), resultado = $2
         WHERE id = $1 AND executado_em IS NULL AND reservado_em IS NOT NULL",
    )
    .bind(id)
    .bind(resultado)
    .execute(db)
    .await?
    .rows_affected();
    if gravou > 0 {
        return Ok(Confirmacao::Gravada);
    }
    let encerrado: Option<bool> =
        sqlx::query_scalar("SELECT executado_em IS NOT NULL FROM nexus.comando WHERE id = $1")
            .bind(id)
            .fetch_optional(db)
            .await?;
    Ok(match encerrado {
        None => Confirmacao::Inexistente,
        Some(true) => Confirmacao::Repetida,
        Some(false) => Confirmacao::NaoReservado,
    })
}

/// Devolve à fila, sem contar tentativa, os comandos reservados que não puderam ser enviados.
pub async fn devolver(db: &PgPool, ids: &[i64]) -> Result<u64, sqlx::Error> {
    sqlx::query(
        "UPDATE nexus.comando SET reservado_em = NULL
         WHERE id = ANY($1) AND executado_em IS NULL AND reservado_em IS NOT NULL",
    )
    .bind(ids)
    .execute(db)
    .await
    .map(|r| r.rows_affected())
}

/// Devolve à fila os comandos reservados por `por` e não confirmados (a conexão anterior caiu).
/// Conta como tentativa; ao esgotar, encerra com erro como no lease. Devolve quantos voltaram.
pub async fn devolver_do_consumidor(db: &PgPool, por: &str) -> Result<u64, sqlx::Error> {
    sqlx::query(
        "UPDATE nexus.comando SET
             tentativas = tentativas + 1,
             reservado_em = CASE WHEN tentativas + 1 >= $2 THEN reservado_em END,
             executado_em = CASE WHEN tentativas + 1 >= $2 THEN now() END,
             resultado = CASE WHEN tentativas + 1 >= $2 THEN $3 END
         WHERE executado_em IS NULL AND reservado_em IS NOT NULL AND reservado_por = $1",
    )
    .bind(por)
    .bind(MAX_TENTATIVAS)
    .bind(RESULTADO_SEM_CONFIRMACAO)
    .execute(db)
    .await
    .map(|r| r.rows_affected())
}

/// Encerra, como `expirado`, os pendentes cujo prazo passou. Devolve, para cada um, se algum
/// consumidor chegou a recebê-lo.
pub async fn expirar(db: &PgPool) -> Result<Vec<bool>, sqlx::Error> {
    let linhas = sqlx::query(
        "UPDATE nexus.comando SET executado_em = now(), resultado = 'expirado'
         WHERE executado_em IS NULL AND reservado_em IS NULL
           AND expira_em IS NOT NULL AND expira_em <= now()
         RETURNING (reservado_por IS NOT NULL) AS entregue",
    )
    .fetch_all(db)
    .await?;
    linhas
        .iter()
        .map(|l| l.try_get::<bool, _>("entregue"))
        .collect()
}

/// Quantos ids para trás do maior já visto o espelho reprocessa, para pegar commits fora de ordem.
const JANELA_ESPELHO: i64 = 200;

/// Copia para `nexus.comando` as linhas novas de `public.comando_fila` (o espelho da fila do
/// legado). Só lê a tabela do legado. `marca` é (piso, maior id do legado já visto): no primeiro ciclo
/// (`None`) parte do maior id existente (não copia o histórico) ou do que já foi espelhado antes.
/// Depois reprocessa a janela `id > marca - 200` (`ON CONFLICT DO NOTHING`). Devolve quantas
/// linhas foram lidas do legado (o chamador repete enquanto for >= `limite`) e quantas entraram.
pub async fn espelhar(
    db: &PgPool,
    limite: i64,
    marca: &mut Option<(i64, i64)>,
) -> Result<(u64, u64), sqlx::Error> {
    let (piso, atual) = match *marca {
        Some(m) => m,
        None => {
            let ja: Option<i64> = sqlx::query_scalar("SELECT max(id_legado) FROM nexus.comando")
                .fetch_one(db)
                .await?;
            let m = match ja {
                Some(m) => m,
                None => {
                    sqlx::query_scalar::<_, Option<i32>>("SELECT max(id) FROM public.comando_fila")
                        .fetch_one(db)
                        .await?
                        .map_or(0, i64::from)
                }
            };
            // O piso impede que a janela reprocesse o histórico anterior ao primeiro ciclo.
            let piso = if ja.is_none() { m } else { 0 };
            *marca = Some((piso, m));
            // Primeiro ciclo sem espelho anterior: só fixa o ponto de partida.
            if ja.is_none() {
                return Ok((0, 0));
            }
            (piso, m)
        }
    };
    let linha = sqlx::query(
        "WITH lote AS (
             SELECT * FROM public.comando_fila WHERE id > greatest($1 - $3, $4) ORDER BY id LIMIT $2),
         ins AS (
             INSERT INTO nexus.comando
                 (conta, tipo, params, prioridade, origem, criado_em, expira_em, executado_em,
                  resultado, id_legado)
             SELECT lower(btrim(nick)), cmd, params, coalesce(prioridade, 5), 'espelho',
                    to_timestamp(ts / 1000.0), to_timestamp(ts_expira / 1000.0),
                    to_timestamp(ts_executado / 1000.0),
                    CASE WHEN ts_executado IS NOT NULL THEN 'legado:' || coalesce(resultado, '') END,
                    id
             FROM lote
             ON CONFLICT (id_legado) DO NOTHING
             RETURNING 1)
         SELECT (SELECT count(*) FROM lote) AS lidas, (SELECT count(*) FROM ins) AS novas,
                (SELECT max(id) FROM lote)::bigint AS maior",
    )
    .bind(atual)
    .bind(limite)
    .bind(JANELA_ESPELHO)
    .bind(piso)
    .fetch_one(db)
    .await?;
    let lidas: i64 = linha.try_get("lidas")?;
    let novas: i64 = linha.try_get("novas")?;
    if let Some(m) = linha.try_get::<Option<i64>, _>("maior")? {
        *marca = Some((piso, m.max(atual)));
    }
    Ok((lidas as u64, novas as u64))
}

/// Fila de comandos para a tela de Automação: os mais recentes primeiro, com o estado
/// (`pendente`, `executando`, `feito`, `falhou`, `expirado`, `agendado`).
pub async fn fila(
    db: &PgPool,
    conta: Option<&str>,
    limite: i64,
) -> Result<Vec<serde_json::Value>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT jsonb_build_object(
                  'id', id, 'conta', conta, 'tipo', tipo, 'params', params, 'origem', origem,
                  'criado', (extract(epoch FROM criado_em) * 1000)::bigint,
                  'executado', (extract(epoch FROM executado_em) * 1000)::bigint,
                  'resultado', resultado, 'tentativas', tentativas,
                  'estado', CASE
                    WHEN executado_em IS NOT NULL AND resultado ILIKE 'erro%' THEN 'falhou'
                    WHEN executado_em IS NOT NULL THEN 'feito'
                    WHEN expira_em IS NOT NULL AND expira_em < now() THEN 'expirado'
                    WHEN executar_em IS NOT NULL AND executar_em > now() THEN 'agendado'
                    WHEN reservado_em IS NOT NULL THEN 'executando'
                    ELSE 'pendente' END)
           FROM nexus.comando
          WHERE ($1::text IS NULL OR conta = $1)
          ORDER BY criado_em DESC
          LIMIT $2",
    )
    .bind(conta)
    .bind(limite)
    .fetch_all(db)
    .await
}
