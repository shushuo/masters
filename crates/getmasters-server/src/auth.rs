//! Per-launch bearer-token enforcement (docs/06 §3).
//!
//! Applied to every route except `/health` and `/openapi.json`. The token is accepted via
//! the `Authorization: Bearer <token>` header or, for the WebSocket upgrade (browser/webview
//! JS cannot set WS headers), a `?token=<token>` query parameter.

use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;

use getmasters_proto::ErrorDto;

use crate::state::AppState;

/// Constant-time byte comparison (avoids leaking the token via timing).
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Extract the presented token from the `Authorization` header or `?token=` query.
fn presented_token(req: &Request) -> Option<String> {
    if let Some(value) = req.headers().get(axum::http::header::AUTHORIZATION) {
        if let Ok(s) = value.to_str() {
            if let Some(tok) = s.strip_prefix("Bearer ") {
                return Some(tok.to_string());
            }
        }
    }
    // Fall back to a query parameter (used by the WS upgrade).
    req.uri().query().and_then(|q| {
        q.split('&').find_map(|kv| {
            let (k, v) = kv.split_once('=')?;
            if k == "token" {
                Some(v.to_string())
            } else {
                None
            }
        })
    })
}

/// Middleware: reject requests whose token does not match `state.token`.
pub async fn require_token(State(state): State<AppState>, req: Request, next: Next) -> Response {
    match presented_token(&req) {
        Some(tok) if ct_eq(tok.as_bytes(), state.token.as_bytes()) => next.run(req).await,
        _ => (
            StatusCode::UNAUTHORIZED,
            Json(ErrorDto::new("missing or invalid bearer token")),
        )
            .into_response(),
    }
}
