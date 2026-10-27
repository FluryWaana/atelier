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

#[tokio::test]
async fn a_kernel_confirmed_free_port_returns_absence_without_reading_the_session() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);
    let result = tokio::time::timeout(
        Duration::from_secs(1),
        LocalHttp::new()
            .unwrap()
            .authenticated_health(&address, || panic!("A free port must not load the session")),
    )
    .await
    .expect("A free port must not wait for HTTP timeouts")
    .unwrap();
    assert_eq!(result, None);
}

#[derive(Clone)]
struct TimedHealth {
    port: u16,
    token: String,
    timeouts: usize,
    proofs: Arc<AtomicUsize>,
    credentials: Arc<AtomicUsize>,
    answered: Arc<AtomicBool>,
    release: Arc<tokio::sync::Notify>,
}
async fn timed_health(
    State(state): State<TimedHealth>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> axum::response::Response {
    if let Some(challenge) = query.get("challenge") {
        assert!(!headers.contains_key("authorization"));
        if state.proofs.fetch_add(1, Ordering::SeqCst) < state.timeouts {
            state.release.notified().await;
            return StatusCode::REQUEST_TIMEOUT.into_response();
        }
        state.answered.store(true, Ordering::SeqCst);
        return Json(json!({"proof":session::proof(&state.token,challenge,state.port).unwrap()}))
            .into_response();
    }
    assert!(state.answered.load(Ordering::SeqCst));
    assert_eq!(headers["authorization"], format!("Bearer {}", state.token));
    state.credentials.fetch_add(1, Ordering::SeqCst);
    Json(json!({"app":"atelier-kanban","pid":42})).into_response()
}

#[tokio::test]
async fn an_initial_proof_timeout_retries_without_credentials_and_authenticates_only_once() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let state = TimedHealth {
        port,
        token: "cd".repeat(32),
        timeouts: 1,
        proofs: Arc::default(),
        credentials: Arc::default(),
        answered: Arc::default(),
        release: Arc::default(),
    };
    let router = Router::new()
        .route("/api/health", get(timed_health))
        .with_state(state.clone());
    let server = tokio::spawn(async { axum::serve(listener, router).await.unwrap() });
    let loads = AtomicUsize::new(0);
    let result = LocalHttp::new()
        .unwrap()
        .authenticated_health(&format!("http://127.0.0.1:{port}"), || {
            assert!(state.answered.load(Ordering::SeqCst));
            loads.fetch_add(1, Ordering::SeqCst);
            Ok(Some(state.token.clone()))
        })
        .await
        .unwrap()
        .unwrap();
    assert_eq!(result, json!({"app":"atelier-kanban","pid":42}));
    assert_eq!(state.proofs.load(Ordering::SeqCst), 2);
    assert_eq!(loads.load(Ordering::SeqCst), 1);
    assert_eq!(state.credentials.load(Ordering::SeqCst), 1);
    state.release.notify_waiters();
    server.abort();
}

#[tokio::test]
async fn persistent_proof_timeouts_are_bounded_errors_and_never_read_or_send_credentials() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let state = TimedHealth {
        port,
        token: "ef".repeat(32),
        timeouts: usize::MAX,
        proofs: Arc::default(),
        credentials: Arc::default(),
        answered: Arc::default(),
        release: Arc::default(),
    };
    let router = Router::new()
        .route("/api/health", get(timed_health))
        .with_state(state.clone());
    let server = tokio::spawn(async { axum::serve(listener, router).await.unwrap() });
    let loads = AtomicUsize::new(0);
    let result = tokio::time::timeout(
        Duration::from_secs(10),
        LocalHttp::new()
            .unwrap()
            .authenticated_health(&format!("http://127.0.0.1:{port}"), || {
                loads.fetch_add(1, Ordering::SeqCst);
                Ok(Some(state.token.clone()))
            }),
    )
    .await
    .expect("Three two-second proof attempts must remain bounded");
    let error = result.expect_err("Timeout is an unknown server state, never absence");
    assert!(
        error
            .downcast_ref::<reqwest::Error>()
            .is_some_and(reqwest::Error::is_timeout)
    );
    assert_eq!(state.proofs.load(Ordering::SeqCst), 3);
    assert_eq!(loads.load(Ordering::SeqCst), 0);
    assert_eq!(state.credentials.load(Ordering::SeqCst), 0);
    state.release.notify_waiters();
    server.abort();
}

#[tokio::test]
async fn failed_or_invalid_proof_responses_are_not_retried_and_do_not_read_the_token() {
    for (status, body) in [
        (StatusCode::FORBIDDEN, "{\"proof\":\"forbidden\"}"),
        (StatusCode::OK, "not JSON"),
        (StatusCode::OK, "{\"proof\":42}"),
    ] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(AtomicUsize::new(0));
        let seen = requests.clone();
        let router = Router::new().route(
            "/api/health",
            get(move |headers: HeaderMap| {
                let seen = seen.clone();
                async move {
                    assert!(!headers.contains_key("authorization"));
                    seen.fetch_add(1, Ordering::SeqCst);
                    (status, body)
                }
            }),
        );
        let server = tokio::spawn(async { axum::serve(listener, router).await.unwrap() });
        assert!(
            LocalHttp::new()
                .unwrap()
                .authenticated_health(&address, || panic!(
                    "Invalid proof responses must not read the token"
                ))
                .await
                .is_err()
        );
        assert_eq!(requests.load(Ordering::SeqCst), 1);
        server.abort();
    }
}

#[tokio::test]
async fn a_mutation_with_a_lost_response_is_never_sent_again() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    let effects = Arc::new(AtomicUsize::new(0));
    let seen = effects.clone();
    let release = Arc::new(tokio::sync::Notify::new());
    let pending = release.clone();
    let router = Router::new().route(
        "/mutation",
        post(move |headers: HeaderMap, body: String| {
            let seen = seen.clone();
            let pending = pending.clone();
            async move {
                assert_eq!(headers["authorization"], "Bearer synthetic-mutation");
                assert_eq!(body, "synthetic-effect");
                seen.fetch_add(1, Ordering::SeqCst);
                pending.notified().await;
                StatusCode::NO_CONTENT
            }
        }),
    );
    let server = tokio::spawn(async { axum::serve(listener, router).await.unwrap() });
    let mut headers = HeaderMap::new();
    headers.insert(
        "authorization",
        "Bearer synthetic-mutation".parse().unwrap(),
    );
    let error = LocalHttp::new()
        .unwrap()
        .request(
            &format!("{address}/mutation"),
            Method::POST,
            headers,
            Some("synthetic-effect"),
            Duration::from_secs(2),
        )
        .await
        .err()
        .expect("The reply is deliberately withheld after the effect");
    assert!(
        error
            .downcast_ref::<reqwest::Error>()
            .is_some_and(reqwest::Error::is_timeout)
    );
    assert_eq!(effects.load(Ordering::SeqCst), 1);
    release.notify_waiters();
    server.abort();
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
