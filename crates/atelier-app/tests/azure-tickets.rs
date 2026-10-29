use atelier_app::infrastructure::azure_tickets::{Azure, manifest};
use atelier_core::application::extensions as protocol;
use axum::{
    Json, Router,
    extract::{Request, State},
    response::{IntoResponse, Response},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
const PROJECT: &str = "00000000-0000-4000-8000-000000000001";
#[test]
fn azure_manifest_uses_translatable_keys() {
    let manifest = manifest();
    protocol::validate_manifest(&manifest).unwrap();
    assert_eq!(manifest.description, "description");
    assert_eq!(
        manifest.translations["fr-FR"]["description"],
        "Recherche, lecture et création confirmée de tickets Azure DevOps."
    );
    assert_eq!(
        manifest.translations["en-US"]["description"],
        "Search, read and confirm creation of Azure DevOps tickets."
    );
    let error = Azure::new("invalid url", PROJECT, "synthetic".into())
        .err()
        .unwrap()
        .to_string();
    assert_eq!(error, "azure-invalid-https-azure-organization");
    assert_eq!(
        manifest.translations["fr-FR"][&error],
        "Organisation Azure HTTPS invalide."
    );
}
#[derive(Clone)]
struct Remote {
    fault: &'static str,
    queries: Arc<AtomicUsize>,
}
async fn serve(State(remote): State<Remote>, request: Request) -> Response {
    assert_eq!(
        request.headers()["authorization"].to_str().unwrap(),
        format!("Basic {}", STANDARD.encode(":synthetic-azure-token"))
    );
    let uri = request.uri().clone();
    if uri.path().ends_with(&format!("/_apis/projects/{PROJECT}")) {
        assert_eq!(request.method(), axum::http::Method::GET);
        return Json(json!({"id":PROJECT,"name":"Synthetic project"})).into_response();
    }
    assert_eq!(request.method(), axum::http::Method::POST);
    let body = axum::body::to_bytes(request.into_body(), 128 * 1024)
        .await
        .unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();
    assert!(!body.to_string().contains("synthetic-azure-token"));
    if uri.path().ends_with("/wiql") {
        remote.queries.fetch_add(1, Ordering::SeqCst);
        let query = body["query"].as_str().unwrap();
        assert!(query.contains("[System.TeamProject] = @project"));
        assert!(query.contains("ORDER BY [System.Id] ASC"));
        assert!(query.contains("CONTAINS 'O''Brien'"));
        let after = query
            .split("[System.Id] > ")
            .nth(1)
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap()
            .parse::<u64>()
            .unwrap();
        let mut ids = (after + 1..=21)
            .take(9)
            .map(|id| json!({"id":id}))
            .collect::<Vec<_>>();
        if remote.fault == "repeat" && !ids.is_empty() {
            ids.push(ids[0].clone());
        }
        return Json(json!({"queryType":"flat","workItems":ids})).into_response();
    }
    assert!(uri.path().ends_with("/workitemsbatch"));
    assert_eq!(body["errorPolicy"], "Fail");
    let rows=body["ids"].as_array().unwrap().iter().map(|id|json!({"id":id,"fields":{"System.Id":id,"System.TeamProject":if remote.fault=="project"{"Other project"}else{"Synthetic project"},"System.Title":if remote.fault=="secret"{"synthetic-azure-token"}else{"Synthetic Azure ticket"},"System.Description":r#"<div>Hello &amp; <b>world</b></div><script>must not import</script><p title="a>b">&#128512;</p>"#,"System.State":"Active"}})).collect::<Vec<_>>();
    Json(json!({"count":rows.len(),"value":rows})).into_response()
}
#[tokio::test]
async fn azure_read_pages_are_scoped_ordered_bounded_and_do_not_modify_remote_tickets() {
    protocol::validate_manifest(&manifest()).unwrap();
    for fault in ["none", "repeat", "project", "secret"] {
        let remote = Remote {
            fault,
            queries: Arc::new(AtomicUsize::new(0)),
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/org", listener.local_addr().unwrap());
        let app = Router::new()
            .fallback(axum::routing::any(serve))
            .with_state(remote.clone());
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let client = Azure::synthetic(&base, PROJECT, "synthetic-azure-token".into()).unwrap();
        let result = client.search("O'Brien", None).await;
        if fault != "none" {
            assert!(
                !result
                    .err()
                    .unwrap()
                    .to_string()
                    .contains("synthetic-azure-token")
            );
        } else {
            let first = result.unwrap();
            assert_eq!(first.items.len(), 8);
            assert_eq!(first.next.as_deref(), Some("8"));
            assert_eq!(first.items[0].description, "Hello & world\n😀");
            assert!(!first.items[0].description.contains("must not import"));
            let second = client
                .search("O'Brien", first.next.as_deref())
                .await
                .unwrap();
            assert_eq!(second.items.len(), 8);
            assert_eq!(second.next.as_deref(), Some("16"));
            let third = client
                .search("O'Brien", second.next.as_deref())
                .await
                .unwrap();
            assert_eq!(third.items.len(), 5);
            assert!(third.next.is_none());
            assert_eq!(second.items[0].id, "9");
            let queries = remote.queries.load(Ordering::SeqCst);
            let ticket = client
                .read(&format!("{base}/{PROJECT}"), "1")
                .await
                .unwrap();
            assert_eq!(ticket.id, "1");
            assert!(client.read(&format!("{base}/other"), "1").await.is_err());
            assert!(
                client
                    .read(&format!("{base}/{PROJECT}"), "01")
                    .await
                    .is_err()
            );
            assert_eq!(remote.queries.load(Ordering::SeqCst), queries);
        }
        server.abort();
        let _ = server.await;
    }
}
#[test]
fn azure_organization_and_project_inputs_cannot_redirect_credentials() {
    for url in [
        "http://dev.azure.com/org",
        "https://elsewhere.example.test/org",
        "https://dev.azure.com/org/../other",
        "https://dev.azure.com/org%2fother",
        "https://dev.azure.com/org\n",
        "https://user@dev.azure.com/org",
    ] {
        assert!(
            Azure::new(url, PROJECT, "synthetic-token".into()).is_err(),
            "{url:?}"
        );
    }
    assert!(
        Azure::new(
            "https://dev.azure.com/org",
            "not-a-project",
            "synthetic-token".into()
        )
        .is_err()
    );
}

#[derive(Clone)]
struct CreationRemote {
    writes: Arc<AtomicUsize>,
    fault: &'static str,
}
async fn creation_remote(State(remote): State<CreationRemote>, request: Request) -> Response {
    assert_eq!(
        request.headers()["authorization"],
        format!("Basic {}", STANDARD.encode(":synthetic-azure-token"))
    );
    let path = request.uri().path().to_owned();
    if path.ends_with(&format!("/_apis/projects/{PROJECT}")) {
        return Json(json!({"id":PROJECT,"name":"Synthetic project"})).into_response();
    }
    assert_eq!(request.method(), axum::http::Method::POST);
    let content_type = request
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    let body = axum::body::to_bytes(request.into_body(), 128 * 1024)
        .await
        .unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();
    assert!(!body.to_string().contains("synthetic-azure-token"));
    if path.ends_with("/wiql") {
        assert!(body["query"].as_str().unwrap().contains(
            "[System.TeamProject] = @project AND [System.Tags] CONTAINS 'atelier-effect-synthetic'"
        ));
        let ids = if remote.fault == "duplicate" {
            vec![json!({"id":1}), json!({"id":2})]
        } else if remote.writes.load(Ordering::SeqCst) > 0 {
            vec![json!({"id":1})]
        } else {
            vec![]
        };
        return Json(json!({"queryType":"flat","workItems":ids})).into_response();
    }
    if path.ends_with("/workitems/$Task") || path.ends_with("/workitems/%24Task") {
        assert_eq!(content_type, "application/json-patch+json");
        assert_eq!(
            body,
            json!([
                {"op":"add","path":"/fields/System.Title","value":"Synthetic title"},
                {"op":"add","path":"/fields/System.Description","value":"<div>Safe &lt;script&gt;<br>second line</div>"},
                {"op":"add","path":"/fields/System.Tags","value":"atelier-effect-synthetic"}
            ])
        );
        assert_eq!(remote.writes.fetch_add(1, Ordering::SeqCst), 0);
        return ([("content-type", "application/json")], "{lost reply").into_response();
    }
    assert!(path.ends_with("/workitemsbatch"));
    assert_eq!(body["ids"], json!([1]));
    Json(json!({"count":1,"value":[{"id":1,"fields":{"System.Id":1,"System.TeamProject":if remote.fault=="foreign"{"Foreign project"}else{"Synthetic project"},"System.WorkItemType":"Task","System.Tags":if remote.fault=="tag"{"another-tag"}else{"atelier-effect-synthetic"},"System.Title":if remote.fault=="edited"{"Human edit"}else{"Synthetic title"},"System.Description":"<div>Safe &lt;script&gt;<br>second line</div>","System.State":"New"}}]})).into_response()
}
#[tokio::test]
async fn lost_azure_creation_reply_is_observed_without_a_second_write_and_foreign_results_fail() {
    for fault in ["none", "duplicate", "foreign", "tag", "edited"] {
        let remote = CreationRemote {
            writes: Arc::new(AtomicUsize::new(0)),
            fault,
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/org", listener.local_addr().unwrap());
        let app = Router::new()
            .fallback(axum::routing::any(creation_remote))
            .with_state(remote.clone());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = Azure::synthetic(&base, PROJECT, "synthetic-azure-token".into()).unwrap();
        let input = json!({"kind":"Task","title":"Synthetic title","description":"Safe <script>\nsecond line"});
        if fault != "duplicate" {
            assert_eq!(
                client
                    .create("effect-synthetic", &input, true)
                    .await
                    .unwrap()["receipt"]["outcome"],
                "unknown"
            );
            assert_eq!(remote.writes.load(Ordering::SeqCst), 0);
        }
        assert!(
            client
                .create("effect-synthetic", &input, false)
                .await
                .is_err()
        );
        let result = client.create("effect-synthetic", &input, true).await;
        if fault == "none" {
            let result = result.unwrap();
            assert_eq!(result["receipt"]["operationId"], "effect-synthetic");
            assert_eq!(result["receipt"]["outcome"], "applied");
            assert_eq!(result["ticket"]["id"], "1");
        } else {
            assert!(result.is_err());
        }
        assert_eq!(
            remote.writes.load(Ordering::SeqCst),
            usize::from(fault != "duplicate")
        );
        server.abort();
    }
}
