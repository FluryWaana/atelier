use atelier_app::infrastructure::claude_account::Client;
use atelier_core::application::agent_account::State as AccountState;
use axum::{
    Json, Router,
    extract::{Request, State},
    response::{IntoResponse, Response},
};
use serde_json::json;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
#[derive(Clone)]
struct Remote {
    calls: Arc<AtomicUsize>,
    fault: &'static str,
    token: String,
}
async fn serve(State(remote): State<Remote>, request: Request) -> Response {
    remote.calls.fetch_add(1, Ordering::SeqCst);
    assert_eq!(request.method(), axum::http::Method::GET);
    assert_eq!(request.uri().path(), "/api/oauth/usage");
    assert!(request.uri().query().is_none());
    assert_eq!(
        request.headers()["authorization"].to_str().unwrap(),
        format!("Bearer {}", remote.token)
    );
    assert_eq!(request.headers()["anthropic-beta"], "oauth-2025-04-20");
    assert!(
        axum::body::to_bytes(request.into_body(), 1)
            .await
            .unwrap()
            .is_empty()
    );
    match remote.fault {
        "rate"=>(axum::http::StatusCode::TOO_MANY_REQUESTS,[("retry-after","3600")],"private-error-body").into_response(),
        "rate-date"=>{let date=(time::OffsetDateTime::now_utc()+time::Duration::hours(2)).format(time::macros::format_description!("[weekday repr:short], [day] [month repr:short] [year] [hour]:[minute]:[second] GMT")).unwrap();(axum::http::StatusCode::TOO_MANY_REQUESTS,[("retry-after",date)],"private-error-body").into_response()},
        "expired"=>(axum::http::StatusCode::UNAUTHORIZED,"private-error-body").into_response(),
        "redirect"=>(axum::http::StatusCode::FOUND,[("location","/forbidden-synthetic-only")]).into_response(),
        "large"=>"x".repeat(65_537).into_response(),
        "secret"=>Json(json!({"five_hour":{"utilization":20},"extra":{"value":remote.token}})).into_response(),
        "invalid"=>Json(json!({"five_hour":{"utilization":20},"seven_day":{"utilization":101}})).into_response(),
        _=>Json(json!({"five_hour":{"utilization":27.5,"resets_at":"2099-01-01T12:00:00Z"},"seven_day":{"utilization":90,"resets_at":"2099-01-07T12:00:00Z"},"seven_day_sonnet":{"utilization":0,"resets_at":null},"seven_day_opus":null,"extra_usage":{"utilization":10},"accessToken":"private-unused-field"})).into_response()
    }
}
#[tokio::test]
async fn scoped_read_only_quota_requests_deduplicate_aliases_and_never_expose_credentials_or_error_bodies()
 {
    for (fault, expected) in [
        ("valid", AccountState::Available),
        ("rate", AccountState::RateLimited),
        ("rate-date", AccountState::RateLimited),
        ("expired", AccountState::Disconnected),
        ("redirect", AccountState::Unavailable),
        ("large", AccountState::Unavailable),
        ("secret", AccountState::Unavailable),
        ("invalid", AccountState::Unavailable),
    ] {
        let remote = Remote {
            calls: Arc::new(AtomicUsize::new(0)),
            fault,
            token: format!("synthetic-claude-{}", uuid::Uuid::new_v4()),
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/api/oauth/usage", listener.local_addr().unwrap());
        let app = Router::new()
            .fallback(axum::routing::any(serve))
            .with_state(remote.clone());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let first = Client::synthetic(&url).unwrap();
        let alias = Client::synthetic(&url).unwrap();
        let (first, alias) = tokio::join!(first.read(&remote.token), alias.read(&remote.token));
        let first = first.unwrap();
        let alias = alias.unwrap();
        assert_eq!(first.state, expected);
        assert_eq!(alias.state, expected);
        assert_eq!(remote.calls.load(Ordering::SeqCst), 1);
        assert_eq!(first.observed_at, alias.observed_at);
        if fault == "valid" {
            assert_eq!(first.windows.len(), 3);
            assert_eq!(first.windows[0].remaining_tenths, 725);
            assert_eq!(first.windows[2].name, "sonnet");
        }
        if matches!(fault, "rate" | "rate-date") {
            let retry = time::OffsetDateTime::parse(
                first.retry_at.as_deref().unwrap(),
                &time::format_description::well_known::Rfc3339,
            )
            .unwrap();
            assert!(
                (retry - time::OffsetDateTime::now_utc()).whole_seconds()
                    >= if fault == "rate-date" { 7199 } else { 3599 }
            );
        }
        let public = serde_json::to_string(&first).unwrap();
        for secret in [
            remote.token.as_str(),
            "private-error-body",
            "private-unused-field",
        ] {
            assert!(!public.contains(secret));
        }
        server.abort();
    }
    assert!(Client::synthetic("http://synthetic.example.test/api/oauth/usage").is_err());
    assert!(Client::synthetic("http://127.0.0.1/api/oauth/usage?token=synthetic").is_err());
}
