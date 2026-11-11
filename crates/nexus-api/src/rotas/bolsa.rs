//! Rotas `/api/bolsa/*`. Mesmo contrato JSON do sistema legado.
//!
//! Os handlers só traduzem HTTP ↔ consultas: a lógica fica em [`crate::bolsa::consultas`].

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use contrato::bolsa::{
    MeusItensEntrada, MeusItensGravados, OfertasEntrada, OfertasGravadas, Params, ResumoEntrada,
    ResumoGravado,
};
use contrato::Erro;
use nucleo::bolsa::ingestao::{self, RepoIngestao};

use crate::bolsa::{agora_ms, consultas};
use crate::estado::Estado;

/// Pega o snapshot ou devolve 503. `let ... else` com `return` dentro de uma macro deixa
/// cada handler com uma linha só para isso.
macro_rules! snapshot {
    ($estado:expr) => {
        match $estado.bolsa.snapshot().await {
            Ok(s) => s,
            Err(erro) => return indisponivel(&*erro),
        }
    };
}

pub async fn mercado(State(estado): State<Estado>, Query(p): Query<Params>) -> Response {
    let s = snapshot!(estado);
    Json(consultas::mercado(&s, &p)).into_response()
}

pub async fn serie(State(estado): State<Estado>, Query(p): Query<Params>) -> Response {
    let s = snapshot!(estado);
    consultas::serie(&s, &p).map_or_else(nao_encontrado, |r| Json(r).into_response())
}

pub async fn ticker(State(estado): State<Estado>, Query(p): Query<Params>) -> Response {
    let s = snapshot!(estado);
    match consultas::ticker_completo(&estado.bolsa, &s, &p).await {
        Ok(Some(r)) => Json(r).into_response(),
        Ok(None) => nao_encontrado(),
        Err(erro) => indisponivel(&erro),
    }
}

pub async fn balao(State(estado): State<Estado>, Query(p): Query<Params>) -> Response {
    let s = snapshot!(estado);
    consultas::balao(&s, &p).map_or_else(nao_encontrado, |r| Json(r).into_response())
}

pub async fn feedback(State(estado): State<Estado>, Query(p): Query<Params>) -> Response {
    let s = snapshot!(estado);
    Json(consultas::feedback(&estado.bolsa, &s, &p)).into_response()
}

pub async fn dicas(State(estado): State<Estado>, Query(p): Query<Params>) -> Response {
    let s = snapshot!(estado);
    // Como no legado: as dicas aquecem em segundo plano o cálculo da equipe padrão.
    let (tipo, moeda) = (
        p.tipo.as_deref().filter(|t| !t.is_empty()),
        p.moeda.as_deref().filter(|m| !m.is_empty()),
    );
    if tipo.is_none_or(|t| t == "pokemon") && moeda.is_none_or(|m| m == "orb") {
        estado.equipe_bolsa.trabalho(
            &estado,
            crate::equipes::bolsa::Pedido {
                boss: None,
                moeda: "orb".into(),
                nick: None,
                idade_min: 90.0,
                n: 5,
                orcamento: None,
            },
        );
    }
    match consultas::dicas(&estado.bolsa, &s, &p).await {
        Ok(r) => Json(r).into_response(),
        Err(erro) => indisponivel(&erro),
    }
}

pub async fn ofertas(State(estado): State<Estado>, Query(p): Query<Params>) -> Response {
    let s = snapshot!(estado);
    match consultas::ofertas(&estado.bolsa, &s, &p).await {
        Ok(r) => Json(r).into_response(),
        Err(erro) => indisponivel(&erro),
    }
}

pub async fn meus_itens(State(estado): State<Estado>, Query(p): Query<Params>) -> Response {
    let s = snapshot!(estado);
    match estado.bolsa.repo.meus_itens().await {
        Ok(inv) => Json(consultas::meus_itens(&s, &p, &inv)).into_response(),
        Err(erro) => indisponivel(&erro),
    }
}

/// Rotas de escrita: só com `NEXUS_ESCRITA_BOLSA=true`. Até o cutover, quem grava nessas
/// tabelas é o legado.
fn escrita_desligada(estado: &Estado) -> Option<Response> {
    (!estado.escrita_bolsa).then(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(Erro::novo(
                "escrita da bolsa desligada neste servidor (NEXUS_ESCRITA_BOLSA)",
            )),
        )
            .into_response()
    })
}

pub async fn gravar_ofertas(
    State(estado): State<Estado>,
    Json(corpo): Json<OfertasEntrada>,
) -> Response {
    if let Some(r) = escrita_desligada(&estado) {
        return r;
    }
    let s = snapshot!(estado);
    let lote = ingestao::validar_ofertas(&corpo.ofertas, &corpo.completo, &s.mercado.catalogo);
    let gravadas = estado
        .bolsa
        .repo
        .gravar_ofertas(&lote.ofertas, agora_ms(), lote.escopo_completo.as_ref())
        .await;
    match gravadas {
        Ok(gravadas) => Json(OfertasGravadas {
            ok: true,
            recebidas: lote.recebidas as u64,
            validas: lote.validas as u64,
            gravadas,
        })
        .into_response(),
        Err(erro) => indisponivel(&erro),
    }
}

pub async fn gravar_resumo(
    State(estado): State<Estado>,
    Json(corpo): Json<ResumoEntrada>,
) -> Response {
    if let Some(r) = escrita_desligada(&estado) {
        return r;
    }
    let (itens, linhas) = ingestao::validar_resumo(&corpo.resumo);
    match estado.bolsa.repo.gravar_resumo(&linhas, agora_ms()).await {
        Ok(gravadas) => Json(ResumoGravado {
            ok: true,
            itens: itens as u64,
            gravadas,
        })
        .into_response(),
        Err(erro) => indisponivel(&erro),
    }
}

pub async fn gravar_meus_itens(
    State(estado): State<Estado>,
    Json(corpo): Json<MeusItensEntrada>,
) -> Response {
    if let Some(r) = escrita_desligada(&estado) {
        return r;
    }
    // Lê, aplica e grava sob um lock: dois POSTs simultâneos não perdem atualização.
    let _vez = estado.meus_itens_lock.lock().await;
    let resultado = async {
        let atual = estado.bolsa.repo.meus_itens().await?;
        let novo = ingestao::aplicar_meus_itens(
            atual,
            &corpo.itens,
            &corpo.limpar,
            &corpo.somar,
            agora_ms(),
        );
        estado.bolsa.repo.gravar_meus_itens(&novo).await?;
        Ok::<_, nucleo::bolsa::repo::ErroRepo>(novo.itens.len())
    }
    .await;
    match resultado {
        Ok(n) => Json(MeusItensGravados {
            ok: true,
            itens: n as u64,
        })
        .into_response(),
        Err(erro) => indisponivel(&erro),
    }
}

fn nao_encontrado() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(Erro::novo("ticker não encontrado")),
    )
        .into_response()
}

fn indisponivel(erro: &dyn std::error::Error) -> Response {
    tracing::warn!(%erro, "bolsa indisponível");
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(Erro::novo("dados do mercado indisponíveis agora")),
    )
        .into_response()
}
