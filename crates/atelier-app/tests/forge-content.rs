use atelier_app::infrastructure::forge::Forge;
use atelier_core::contracts::{ForgeConfiguration, ForgeContent, ForgeKind};
use axum::{
    Json, Router,
    extract::{Request, State},
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
#[derive(Clone)]
struct Remote {
    kind: ForgeKind,
    content: Arc<Mutex<ForgeContent>>,
    writes: Arc<AtomicUsize>,
    lost: bool,
}
async fn serve(State(remote): State<Remote>, request: Request) -> Response {
    assert!(request.headers().contains_key("authorization"));
    let path = request.uri().path().to_owned();
    if request.method() != axum::http::Method::GET {
        let expected_method = if remote.kind == ForgeKind::Gitlab {
            axum::http::Method::PUT
        } else {
            axum::http::Method::PATCH
        };
        assert_eq!(request.method(), expected_method);
        let bytes = axum::body::to_bytes(request.into_body(), 100_000)
            .await
            .unwrap();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body.as_object().unwrap().len(), 1);
        assert!(body["title"].is_string());
        remote.content.lock().unwrap().title = body["title"].as_str().unwrap().into();
        remote.writes.fetch_add(1, Ordering::SeqCst);
        if remote.lost {
            return (
                axum::http::StatusCode::BAD_GATEWAY,
                "synthetic-content-token",
            )
                .into_response();
        }
        return Json(json!({})).into_response();
    }
    if remote.kind == ForgeKind::Gitlab && path.ends_with("/projects/group%2Frepo") {
        return Json(json!({"id":1,"path_with_namespace":"group/repo"})).into_response();
    }
    if remote.kind == ForgeKind::Azure && path.ends_with("/repositories/repo") {
        return Json(json!({"id":"synthetic-repository"})).into_response();
    }
    let content = remote.content.lock().unwrap().clone();
    let source = "a".repeat(40);
    let target = "b".repeat(40);
    Json(match remote.kind {
        ForgeKind::Github=>json!({"number":7,"title":content.title,"body":content.body,"state":"open","merged":false,"draft":true,"head":{"ref":"task","sha":source,"repo":{"full_name":"owner/repo"}},"base":{"ref":"main","sha":target,"repo":{"full_name":"owner/repo"}}}),
        ForgeKind::Gitlab=>json!({"iid":7,"title":content.title,"description":content.body,"state":"opened","draft":true,"source_branch":"task","target_branch":"main","source_project_id":1,"target_project_id":1,"diff_refs":{"head_sha":source,"start_sha":target}}),
        ForgeKind::Azure=>json!({"pullRequestId":7,"title":content.title,"description":content.body,"status":"active","isDraft":true,"sourceRefName":"refs/heads/task","targetRefName":"refs/heads/main","repository":{"id":"synthetic-repository"},"lastMergeSourceCommit":{"commitId":source},"lastMergeTargetCommit":{"commitId":target}}),
    }).into_response()
}
#[tokio::test]
async fn content_updates_preserve_unedited_fields_refuse_stale_reads_and_never_replay() {
    for kind in [ForgeKind::Github, ForgeKind::Gitlab, ForgeKind::Azure] {
        for lost in [false, true] {
            let remote = Remote {
                kind,
                content: Arc::new(Mutex::new(ForgeContent {
                    title: "Original".into(),
                    body: "Human body".into(),
                })),
                writes: Arc::new(AtomicUsize::new(0)),
                lost,
            };
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = format!("http://{}", listener.local_addr().unwrap());
            let app = Router::new()
                .fallback(axum::routing::any(serve))
                .with_state(remote.clone());
            let server = tokio::spawn(async move {
                axum::serve(listener, app).await.unwrap();
            });
            let (prefix, repository) = match kind {
                ForgeKind::Github => ("", "owner/repo"),
                ForgeKind::Gitlab => ("/prefix", "group/repo"),
                ForgeKind::Azure => ("/org", "project/repo"),
            };
            let client = Forge::synthetic(
                ForgeConfiguration {
                    kind,
                    base: format!("{address}{prefix}"),
                },
                "synthetic-content-token".into(),
                1,
            )
            .unwrap();
            let old = client.pull_request(repository, 7).await.unwrap();
            remote.content.lock().unwrap().body = "Human changed body".into();
            let mut content = ForgeContent {
                title: "New title".into(),
                body: old.body.clone(),
            };
            assert!(
                client
                    .prepare_content(&old, &content, &uuid::Uuid::new_v4().to_string())
                    .await
                    .is_err()
            );
            assert_eq!(remote.writes.load(Ordering::SeqCst), 0);
            let fresh = client.pull_request(repository, 7).await.unwrap();
            content.body = fresh.body.clone();
            let prepared = client
                .prepare_content(&fresh, &content, &uuid::Uuid::new_v4().to_string())
                .await
                .unwrap();
            let result = client.send_draft(prepared).await;
            if lost {
                assert!(
                    !result
                        .err()
                        .unwrap()
                        .to_string()
                        .contains("synthetic-content-token")
                );
            } else {
                assert_eq!(result.unwrap().title, "New title");
            }
            assert_eq!(remote.writes.load(Ordering::SeqCst), 1);
            let observed = client.pull_request(repository, 7).await.unwrap();
            assert_eq!(observed.body, "Human changed body");
            assert_eq!(observed.title, "New title");
            assert!(observed.draft);
            assert_eq!(remote.writes.load(Ordering::SeqCst), 1);
            server.abort();
            let _ = server.await;
        }
    }
}
