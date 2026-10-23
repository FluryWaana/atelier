use atelier_app::infrastructure::{local_http::LocalHttp, session};
use axum::{
    Json, Router,
    extract::{Query, State},
    http::{HeaderMap, Method, StatusCode},
    response::IntoResponse,
    routing::{get, post},
};
use serde_json::json;
use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::io::AsyncReadExt;

#[tokio::test]
async fn local_requests_preserve_errors_and_reject_redirects_oversized_bodies_and_timeouts() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    let router = Router::new()
        .route(
            "/echo",
            post(|headers: HeaderMap, body: String| async move {
                assert_eq!(headers["authorization"], "Bearer synthetic-session");
                Json(json!({"body":body}))
            }),
        )
        .route(
            "/conflict",
            get(|| async { (StatusCode::CONFLICT, Json(json!({"error":"Agent actif"}))) }),
        )
        .route(
            "/redirect",
            get(|| async { (StatusCode::FOUND, [("location", "https://example.invalid")]) }),
        )
        .route("/large", get(|| async { "x".repeat(65_537) }))
        .route(
            "/hang",
            get(|| async { std::future::pending::<String>().await }),
        );
    let server = tokio::spawn(async { axum::serve(listener, router).await.unwrap() });
    let client = LocalHttp::new().unwrap();
    let response = client
        .request(
            &format!("{address}/conflict"),
            Method::GET,
            HeaderMap::new(),
            None,
            Duration::from_secs(2),
        )
        .await
        .unwrap();
    assert_eq!(response.status, StatusCode::CONFLICT);
    assert_eq!(response.json().unwrap(), json!({"error":"Agent actif"}));
    let mut headers = HeaderMap::new();
    headers.insert("authorization", "Bearer synthetic-session".parse().unwrap());
    let response = client
        .request(
            &format!("{address}/echo"),
            Method::POST,
            headers,
            Some("{\"value\":\"é\"}"),
            Duration::from_secs(2),
        )
        .await
        .unwrap();
    assert_eq!(
        response.json().unwrap(),
        json!({"body":"{\"value\":\"é\"}"})
    );
    for path in ["/redirect", "/large", "/hang"] {
        assert!(
            client
                .request(
                    &format!("{address}{path}"),
                    Method::GET,
                    HeaderMap::new(),
                    None,
                    Duration::from_millis(100)
                )
                .await
                .is_err()
        );
    }
    for address in [
        "https://127.0.0.1:4317",
        "http://example.invalid",
        "http://user:secret@127.0.0.1:4317",
        "http://127.0.0.1/#session",
    ] {
        assert!(
            client
                .request(
                    address,
                    Method::GET,
                    HeaderMap::new(),
                    None,
                    Duration::from_secs(2)
                )
                .await
                .is_err()
        );
    }
    server.abort();
}

#[derive(Clone)]
struct HealthState {
    port: u16,
    token: String,
    wrong: Arc<AtomicBool>,
    answered: Arc<AtomicBool>,
    credentials: Arc<AtomicUsize>,
}

async fn health(
    State(state): State<HealthState>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> axum::response::Response {
    if let Some(challenge) = query.get("challenge") {
        assert!(!headers.contains_key("authorization"));
        state.answered.store(true, Ordering::SeqCst);
        let proof = if state.wrong.load(Ordering::SeqCst) {
            "wrong".into()
        } else {
            session::proof(&state.token, challenge, state.port).unwrap()
        };
        return Json(json!({"proof":proof})).into_response();
    }
    assert_eq!(headers["authorization"], format!("Bearer {}", state.token));
    state.credentials.fetch_add(1, Ordering::SeqCst);
    Json(json!({"app":"atelier-kanban", "pid":42})).into_response()
}

#[tokio::test]
async fn health_sends_credentials_only_after_proof_and_loads_the_session_after_the_response() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let address = format!("http://127.0.0.1:{port}");
    let state = HealthState {
        port,
        token: "ab".repeat(32),
        wrong: Arc::new(AtomicBool::new(true)),
        answered: Arc::new(AtomicBool::new(false)),
        credentials: Arc::new(AtomicUsize::new(0)),
    };
    let router = Router::new()
        .route("/api/health", get(health))
        .with_state(state.clone());
    let server = tokio::spawn(async { axum::serve(listener, router).await.unwrap() });
    let client = LocalHttp::new().unwrap();
    let load = || {
        assert!(state.answered.load(Ordering::SeqCst));
        Ok(Some(state.token.clone()))
    };
    assert!(client.authenticated_health(&address, load).await.is_err());
    assert_eq!(state.credentials.load(Ordering::SeqCst), 0);
    state.wrong.store(false, Ordering::SeqCst);
    let data = client
        .authenticated_health(&address, load)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(data, json!({"app":"atelier-kanban", "pid":42}));
    assert_eq!(state.credentials.load(Ordering::SeqCst), 1);
    for address in [
        "http://127.0.0.1:0",
        "http://127.0.0.1:65536",
        "http://127.0.0.1:4317/",
        "http://localhost:4317",
        "http://127.0.0.1:4317?x",
        "http://127.0.0.1:abc",
    ] {
        assert!(
            client
                .authenticated_health(address, || panic!(
                    "Invalid addresses must not load credentials"
                ))
                .await
                .is_err()
        );
    }
    server.abort();
}

#[tokio::test]
async fn a_closed_control_socket_never_requests_credentials() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut bytes = [0; 4096];
        assert!(socket.read(&mut bytes).await.unwrap() > 0);
    });
    let result = LocalHttp::new()
        .unwrap()
        .authenticated_health(&address, || {
            panic!("Closed sockets must not load credentials")
        })
        .await;
    assert_eq!(result.unwrap(), None);
    server.await.unwrap();
}

#[test]
fn browser_address_and_missing_session_keep_the_native_privacy_contract() {
    let directory = tempfile::tempdir().unwrap();
    let token = session::new_token().unwrap();
    assert!(session::read_optional(directory.path()).unwrap().is_none());
    session::write(directory.path(), &token).unwrap();
    assert_eq!(
        session::read_optional(directory.path()).unwrap(),
        Some(token.clone())
    );
    let address =
        session::browser_address(directory.path(), "http://127.0.0.1:4317", &token).unwrap();
    #[cfg(windows)]
    assert_eq!(address, format!("http://127.0.0.1:4317/#session={token}"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert!(!address.contains(&token));
        let path = reqwest::Url::parse(&address)
            .unwrap()
            .to_file_path()
            .unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o077,
            0
        );
        assert!(
            std::fs::read_to_string(path)
                .unwrap()
                .contains(&format!("/#session={token}"))
        );
    }
    assert!(
        session::browser_address(directory.path(), "http://127.0.0.1:4317", "\"injected").is_err()
    );
    assert!(
        session::browser_address(directory.path(), "http://example.invalid:4317", &token).is_err()
    );
}
