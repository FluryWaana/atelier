use super::{CSP, Shared, events, failure};
use crate::infrastructure::session;
use axum::{
    Json,
    extract::{Query, Request, State},
    http::{Method, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use serde_json::json;
fn header_value<'a>(request: &'a Request, name: &str) -> Option<&'a str> {
    request
        .headers()
        .get(name)
        .and_then(|value| value.to_str().ok())
}

pub(super) async fn security(
    State(state): State<Shared>,
    request: Request,
    next: Next,
) -> Response {
    let mut response = authorize(state, request, next).await;
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        CSP.parse().expect("constant header"),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        "nosniff".parse().expect("constant header"),
    );
    headers.insert(
        header::CACHE_CONTROL,
        "no-store".parse().expect("constant header"),
    );
    response
}

async fn authorize(state: Shared, mut request: Request, next: Next) -> Response {
    let host = header_value(&request, "host").unwrap_or("");
    if host != format!("127.0.0.1:{}", state.port) && host != format!("localhost:{}", state.port) {
        return failure(StatusCode::FORBIDDEN, "message.host_not_allowed");
    }
    if request.headers().contains_key("origin")
        && header_value(&request, "origin") != Some(format!("http://{host}").as_str())
    {
        return failure(StatusCode::FORBIDDEN, "message.origin_not_allowed");
    }
    let path = request.uri().path();
    let challenge = Query::<HealthQuery>::try_from_uri(request.uri())
        .ok()
        .and_then(|query| query.0.challenge);
    let public_proof = request.method() == Method::GET
        && path == "/api/health"
        && challenge.as_deref().is_some_and(valid_token);
    if path.starts_with("/api/") && !public_proof {
        let bearer = header_value(&request, "authorization")
            .map(|value| value.strip_prefix("Bearer ").unwrap_or(value))
            .filter(|value| !value.is_empty());
        let authenticated = if let Some(token) = bearer {
            session::same_secret(token, &state.token)
        } else {
            path == "/api/events"
                && request.method() == Method::GET
                && events::authenticated(&request, state.port, &state.event_token)
        };
        if !authenticated {
            return failure(
                StatusCode::UNAUTHORIZED,
                "message.session_expired_or_missing_reopen_atelier_using_its_launcher",
            );
        }
        if request.method() != Method::GET
            && (header_value(&request, "x-local-request") != Some("1")
                || header_value(&request, "content-type") != Some("application/json"))
        {
            return failure(
                StatusCode::FORBIDDEN,
                "message.a_local_json_request_is_required",
            );
        }
        if request.method() != Method::GET && *state.shutdown.borrow() {
            return failure(StatusCode::CONFLICT, "message.atelier_is_shutting_down");
        }
    }
    request.extensions_mut().insert(state);
    next.run(request).await
}

fn valid_token(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

#[derive(Deserialize)]
pub(super) struct HealthQuery {
    challenge: Option<String>,
}

pub(super) async fn health(
    State(state): State<Shared>,
    Query(query): Query<HealthQuery>,
) -> Response {
    if let Some(challenge) = query.challenge.filter(|value| valid_token(value)) {
        return match session::proof(&state.token, &challenge, state.port) {
            Ok(proof) => Json(json!({"proof":proof})).into_response(),
            Err(_) => failure(
                StatusCode::INTERNAL_SERVER_ERROR,
                "message.server_identity_unavailable",
            ),
        };
    }
    Json(json!({"app":"atelier-kanban", "protocol":"atelier-local/1", "version":env!("CARGO_PKG_VERSION"), "pid":std::process::id(), "dataDirectory":state.prototype_paths[0]})).into_response()
}
