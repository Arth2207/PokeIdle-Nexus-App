//! ETag nas respostas de leitura: se o mercado não mudou, o cliente recebe `304` sem corpo.

use axum::body::{to_bytes, Body};
use axum::extract::Request;
use axum::http::{header, HeaderValue, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use sha2::{Digest, Sha256};

const MAX_CORPO: usize = 64 * 1024 * 1024;

pub async fn etag(req: Request, next: Next) -> Response {
    let pedido = (req.method() == Method::GET)
        .then(|| req.headers().get(header::IF_NONE_MATCH).cloned())
        .flatten();
    let eh_get = req.method() == Method::GET;
    let resposta = next.run(req).await;
    if !eh_get || resposta.status() != StatusCode::OK {
        return resposta;
    }
    let (mut partes, corpo) = resposta.into_parts();
    let Ok(bytes) = to_bytes(corpo, MAX_CORPO).await else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let hash = Sha256::digest(&bytes);
    let valor = format!("\"{}\"", hex::encode(&hash[..16]));
    let Ok(etag) = HeaderValue::from_str(&valor) else {
        return Response::from_parts(partes, Body::from(bytes));
    };
    if pedido.as_ref() == Some(&etag) {
        let mut r = StatusCode::NOT_MODIFIED.into_response();
        r.headers_mut().insert(header::ETAG, etag);
        return r;
    }
    partes.headers.insert(header::ETAG, etag);
    Response::from_parts(partes, Body::from(bytes))
}
