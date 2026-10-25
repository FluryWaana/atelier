use atelier_app::infrastructure::tempo::Tempo;
use atelier_core::contracts::TempoPayload;
use axum::{
    Json, Router,
    extract::{Request, State},
    http::{Method, StatusCode},
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[derive(Clone)]
struct Remote {
    payload: TempoPayload,
    fault: &'static str,
    writes: Arc<AtomicUsize>,
    reads: Arc<AtomicUsize>,
}
fn row(payload: &TempoPayload, id: u64) -> Value {
    json!({"tempoWorklogId":id,"issue":{"id":payload.issue_id},"author":{"accountId":payload.author_account_id},"timeSpentSeconds":payload.time_spent_seconds,"startDate":payload.start_date,"startTime":payload.start_time,"description":payload.description})
}
async fn serve(State(remote): State<Remote>, request: Request) -> Response {
    assert_eq!(
        request.headers()["authorization"],
        "Bearer synthetic-tempo-token"
    );
    let path = request.uri().path().to_owned();
    if request.method() == Method::POST {
        remote.writes.fetch_add(1, Ordering::SeqCst);
        assert_eq!(path, "/4/worklogs");
        let bytes = axum::body::to_bytes(request.into_body(), 64 * 1024)
            .await
            .unwrap();
        assert_eq!(
            serde_json::from_slice::<TempoPayload>(&bytes).unwrap(),
            remote.payload
        );
        if remote.fault == "lost" {
            return (
                StatusCode::BAD_GATEWAY,
                "Synthetic lost response after write",
            )
                .into_response();
        }
        if remote.fault == "redirect" {
            return (
                StatusCode::TEMPORARY_REDIRECT,
                [("location", "/must-not-follow")],
            )
                .into_response();
        }
        return (StatusCode::CREATED, Json(row(&remote.payload, 7))).into_response();
    }
    assert_eq!(request.method(), Method::GET);
    remote.reads.fetch_add(1, Ordering::SeqCst);
    assert_ne!(path, "/must-not-follow");
    let mut matched = row(&remote.payload, 7);
    match remote.fault {
        "wrong" => matched["timeSpentSeconds"] = json!(remote.payload.time_spent_seconds + 1),
        "secret" => matched["description"] = json!("synthetic-tempo-token"),
        _ => {}
    }
    if path == "/4/worklogs/7" {
        return Json(matched).into_response();
    }
    assert_eq!(path, "/4/worklogs/user/synthetic-account");
    let url = reqwest::Url::parse(&format!("http://127.0.0.1{}", request.uri())).unwrap();
    let query = url
        .query_pairs()
        .collect::<std::collections::BTreeMap<_, _>>();
    assert_eq!(query.get("from").unwrap(), "2026-10-25");
    assert_eq!(query.get("to").unwrap(), "2026-10-25");
    assert_eq!(query.get("limit").unwrap(), "100");
    let offset = query.get("offset").unwrap().parse::<usize>().unwrap();
    let rows = if offset == 0 && remote.fault != "missing" {
        vec![matched]
    } else if offset == 1 && remote.fault == "duplicate" {
        vec![row(&remote.payload, 8)]
    } else if offset == 1 && remote.fault == "repeat" {
        vec![matched]
    } else if remote.fault == "oversized" {
        (1..=101).map(|id| row(&remote.payload, id)).collect()
    } else {
        vec![]
    };
    Json(json!({"results":rows,"metadata":{}})).into_response()
}
fn payload(id: &str) -> TempoPayload {
    TempoPayload {
        issue_id: 123,
        author_account_id: "synthetic-account".into(),
        time_spent_seconds: 3600,
        start_date: "2026-10-25".into(),
        start_time: "09:30:00".into(),
        description: format!("Synthetic human time\n[atelier:{id}]"),
    }
}

#[tokio::test]
async fn one_write_then_exact_identity_read_or_full_marker_reconciliation_without_replay() {
    for fault in [
        "success",
        "lost",
        "redirect",
        "wrong",
        "secret",
        "duplicate",
        "repeat",
        "missing",
        "oversized",
    ] {
        let id = uuid::Uuid::new_v4().to_string();
        let payload = payload(&id);
        let writes = Arc::new(AtomicUsize::new(0));
        let reads = Arc::new(AtomicUsize::new(0));
        let remote = Remote {
            payload: payload.clone(),
            fault,
            writes: writes.clone(),
            reads: reads.clone(),
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/4", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(listener, Router::new().fallback(serve).with_state(remote))
                .await
                .unwrap();
        });
        let client = Tempo::synthetic(&base, "synthetic-tempo-token".into()).unwrap();
        let created = client.create(&payload).await;
        if ["lost", "redirect"].contains(&fault) {
            assert!(created.is_err());
        } else {
            assert_eq!(created.unwrap(), 7);
        }
        assert_eq!(writes.load(Ordering::SeqCst), 1);
        let mut secret_payload = payload.clone();
        secret_payload.description = "Do not send synthetic-tempo-token".into();
        assert!(client.create(&secret_payload).await.is_err());
        assert_eq!(writes.load(Ordering::SeqCst), 1);
        if !["lost", "redirect"].contains(&fault) {
            let read = client.read(7, &payload).await;
            if ["wrong", "secret"].contains(&fault) {
                assert!(read.is_err());
            } else {
                assert_eq!(read.unwrap(), 7);
            }
        }
        let found = client.find(&id, &payload).await;
        match fault {
            "wrong" | "secret" | "duplicate" | "repeat" | "oversized" => {
                assert!(found.is_err(), "{fault}")
            }
            "missing" => assert_eq!(found.unwrap(), None),
            _ => assert_eq!(found.unwrap(), Some(7)),
        }
        assert_eq!(
            writes.load(Ordering::SeqCst),
            1,
            "reconciliation must not POST"
        );
        if ["success", "lost"].contains(&fault) {
            assert!(
                reads.load(Ordering::SeqCst) >= 2,
                "short page must not hide a later duplicate"
            );
        }
        server.abort();
        let _ = server.await;
    }
}

#[test]
fn transport_requires_explicit_tempo_site_and_synthetic_loopback_guard() {
    for base in [
        "http://api.tempo.io/4",
        "https://api.tempo.io.evil.test/4",
        "https://api.tempo.io/4?token=bad",
    ] {
        assert!(Tempo::new(base, "synthetic-token".into()).is_err());
    }
    for base in [
        "http://localhost:1234/4",
        "http://192.0.2.1/4",
        "http://user@127.0.0.1:1234/4",
    ] {
        assert!(Tempo::synthetic(base, "synthetic-token".into()).is_err());
    }
    assert!(Tempo::synthetic("http://127.0.0.1:1234/4", "non-synthetic-token".into()).is_err());
}
