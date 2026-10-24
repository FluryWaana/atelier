use atelier_app::infrastructure::jira::Jira;
use atelier_core::contracts::{JiraConfiguration, JiraMode};
use axum::{
    Json, Router,
    body::Body,
    extract::State,
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::post,
};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[derive(Clone)]
struct Fixture {
    mode: &'static str,
    calls: Arc<AtomicUsize>,
}
async fn serve(
    State(fixture): State<Fixture>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let call = fixture.calls.fetch_add(1, Ordering::SeqCst);
    assert_eq!(body["jql"], "project = SYN");
    assert_eq!(body["fields"], json!(["summary", "status"]));
    if fixture.mode == "data-center" {
        assert_eq!(headers[header::AUTHORIZATION], "Bearer synthetic-token");
        assert_eq!(body["startAt"], call);
    } else {
        assert_eq!(
            headers[header::AUTHORIZATION],
            "Basic c3ludGhldGljQGV4YW1wbGUudGVzdDpzeW50aGV0aWMtdG9rZW4="
        );
    }
    match fixture.mode {
        "unauthorized" => {
            return (
                StatusCode::UNAUTHORIZED,
                "synthetic-token must never be echoed",
            )
                .into_response();
        }
        "forbidden" => {
            return (
                StatusCode::FORBIDDEN,
                "synthetic-token must never be echoed",
            )
                .into_response();
        }
        "rate-limit" => {
            return (
                StatusCode::TOO_MANY_REQUESTS,
                "synthetic-token must never be echoed",
            )
                .into_response();
        }
        "redirect" => {
            return (
                StatusCode::FOUND,
                [(header::LOCATION, "/jira/rest/api/3/search/jql")],
                "synthetic-token",
            )
                .into_response();
        }
        "oversized" => {
            return (
                [(header::CONTENT_TYPE, "application/json")],
                " ".repeat(4 * 1024 * 1024 + 1),
            )
                .into_response();
        }
        "chunked" => {
            let chunks = futures_util::stream::iter([
                Ok::<_, std::io::Error>(vec![b' '; 2 * 1024 * 1024]),
                Ok(vec![b' '; 2 * 1024 * 1024 + 1]),
            ]);
            return (
                [(header::CONTENT_TYPE, "application/json")],
                Body::from_stream(chunks),
            )
                .into_response();
        }
        "invalid-json" => {
            return (
                [(header::CONTENT_TYPE, "application/json")],
                "synthetic-token",
            )
                .into_response();
        }
        _ => {}
    }
    let id = if fixture.mode == "duplicate" {
        1
    } else {
        call + 1
    };
    let row = json!({"id":id.to_string(),"key":format!("SYN-{id}"),"fields":{"summary":if fixture.mode=="secret"{"synthetic-token"}else{"Synthetic é😀 <script>"},"status":{"name":"Open"}}});
    let rows = if fixture.mode == "empty-next" && call == 0 {
        json!([])
    } else {
        json!([row])
    };
    if fixture.mode == "data-center" {
        return Json(json!({"startAt":call,"total":2,"issues":rows})).into_response();
    }
    if fixture.mode == "invalid-end" {
        return Json(json!({"isLast":false,"issues":rows})).into_response();
    }
    if fixture.mode == "cycle" {
        return Json(json!({"isLast":false,"nextPageToken":"same","issues":rows})).into_response();
    }
    if call == 0 {
        Json(json!({"isLast":false,"nextPageToken":"next","issues":rows})).into_response()
    } else {
        assert_eq!(body["nextPageToken"], "next");
        Json(json!({"isLast":true,"issues":rows})).into_response()
    }
}

#[tokio::test]
async fn local_doubles_cover_authentication_pagination_redirects_limits_and_secret_safe_errors() {
    for mode in [
        "cloud",
        "data-center",
        "empty-next",
        "duplicate",
        "cycle",
        "invalid-end",
        "unauthorized",
        "forbidden",
        "rate-limit",
        "redirect",
        "oversized",
        "chunked",
        "invalid-json",
        "secret",
    ] {
        let fixture = Fixture {
            mode,
            calls: Arc::new(AtomicUsize::new(0)),
        };
        let calls = fixture.calls.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = Router::new()
            .route("/jira/rest/api/3/search/jql", post(serve))
            .route("/jira/rest/api/2/search", post(serve))
            .with_state(fixture);
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let configuration = JiraConfiguration {
            mode: if mode == "data-center" {
                JiraMode::DataCenter
            } else {
                JiraMode::Cloud
            },
            url: format!("http://{address}/jira"),
            email: "synthetic@example.test".into(),
            query: "project = SYN".into(),
            revision: 7,
        };
        assert!(Jira::new(configuration.clone(), "synthetic-token".into()).is_err());
        assert!(Jira::synthetic(configuration.clone(), "real-credentials-refused".into()).is_err());
        let client = Jira::synthetic(configuration, "synthetic-token".into()).unwrap();
        let result = client.search().await;
        if ["cloud", "data-center", "empty-next"].contains(&mode) {
            let result = result.unwrap();
            assert_eq!(result.connection_revision, 7);
            assert_eq!(
                result.issues.len(),
                if mode == "empty-next" { 1 } else { 2 }
            );
            assert!(result.issues.iter().all(|issue| {
                issue
                    .url
                    .starts_with(&format!("http://{address}/jira/browse/SYN-"))
            }));
            assert_eq!(calls.load(Ordering::SeqCst), 2);
        } else {
            let message = result
                .err()
                .expect("Synthetic malformed response must fail")
                .to_string();
            assert!(!message.contains("synthetic-token"));
            if mode == "redirect" {
                assert_eq!(calls.load(Ordering::SeqCst), 1);
            }
        }
        server.abort();
        let _ = server.await;
    }
}

#[tokio::test]
async fn issue_details_are_explicit_bounded_and_reject_mismatched_ids_and_reflected_secrets() {
    use axum::extract::{Path, Query};
    use std::collections::BTreeMap;
    async fn detail(
        Path(version): Path<String>,
        Query(query): Query<BTreeMap<String, String>>,
        headers: HeaderMap,
        State(mode): State<&'static str>,
    ) -> Json<Value> {
        assert_eq!(
            query.get("fields").map(String::as_str),
            Some("summary,status,description")
        );
        if version == "2" {
            assert_eq!(headers[header::AUTHORIZATION], "Bearer synthetic-token");
        } else {
            assert_eq!(
                headers[header::AUTHORIZATION],
                "Basic c3ludGhldGljQGV4YW1wbGUudGVzdDpzeW50aGV0aWMtdG9rZW4="
            );
        }
        Json(
            json!({"id":if mode=="mismatch"{"2"}else{"1"},"key":"SYN-1","fields":{"summary":"Synthetic","status":{"name":"Open"},"description":if mode=="secret"{json!("synthetic-token")}else if mode=="large"{json!("x".repeat(20001))}else if version=="2"{json!("Plain é😀")}else{json!({"type":"doc","version":1,"content":[{"type":"paragraph","content":[{"type":"text","text":"Plain é😀"}]}]})}}}),
        )
    }
    for mode in ["cloud", "data-center", "mismatch", "secret", "large"] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = Router::new()
            .route(
                "/jira/rest/api/{version}/issue/1",
                axum::routing::get(detail),
            )
            .with_state(mode);
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = Jira::synthetic(
            JiraConfiguration {
                mode: if mode == "data-center" {
                    JiraMode::DataCenter
                } else {
                    JiraMode::Cloud
                },
                url: format!("http://{address}/jira"),
                email: "synthetic@example.test".into(),
                query: String::new(),
                revision: 9,
            },
            "synthetic-token".into(),
        )
        .unwrap();
        assert!(client.detail("../1").await.is_err());
        let result = client.detail("1").await;
        if ["cloud", "data-center"].contains(&mode) {
            let result = result.unwrap();
            assert_eq!(result.description, "Plain é😀");
            assert_eq!(result.connection_revision, 9);
            assert_eq!(
                result.issue.url,
                format!("http://{address}/jira/browse/SYN-1")
            );
        } else {
            assert!(
                !result
                    .err()
                    .unwrap()
                    .to_string()
                    .contains("synthetic-token")
            );
        }
        server.abort();
        let _ = server.await;
    }
}
