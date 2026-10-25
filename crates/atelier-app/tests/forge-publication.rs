use atelier_app::infrastructure::forge::{Forge, publication_marker};
use atelier_core::contracts::{
    ForgeConfiguration, ForgeKind, ForgePublicationPreview, Worktree, WorktreeState,
};
use axum::{
    Json, Router,
    extract::{Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
#[derive(Clone)]
struct Remote {
    kind: ForgeKind,
    fault: &'static str,
    exists: Arc<AtomicBool>,
    writes: Arc<AtomicUsize>,
    body: Arc<Mutex<String>>,
}
async fn serve(State(remote): State<Remote>, request: Request) -> Response {
    assert!(request.headers().contains_key("authorization"));
    let path = request.uri().path().to_owned();
    let uri = request.uri().clone();
    if request.method() == axum::http::Method::POST {
        let body = axum::body::to_bytes(request.into_body(), 32768)
            .await
            .unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert!(!body.to_string().contains("synthetic-publication-token"));
        remote.writes.fetch_add(1, Ordering::SeqCst);
        let description = match remote.kind {
            ForgeKind::Github => {
                assert_eq!(path, "/api/v3/repos/owner/repo/pulls");
                assert_eq!(body["head"], "task");
                assert_eq!(body["base"], "main");
                assert_eq!(body["maintainer_can_modify"], false);
                &body["body"]
            }
            ForgeKind::Gitlab => {
                assert_eq!(path, "/prefix/api/v4/projects/group%2Frepo/merge_requests");
                assert_eq!(body["source_branch"], "task");
                assert_eq!(body["target_branch"], "main");
                assert_eq!(body["title"], "Draft: Synthetic title");
                assert_eq!(body["remove_source_branch"], false);
                &body["description"]
            }
            ForgeKind::Azure => {
                assert_eq!(
                    path,
                    "/org/project/_apis/git/repositories/repo/pullrequests"
                );
                assert_eq!(body["sourceRefName"], "refs/heads/task");
                assert_eq!(body["targetRefName"], "refs/heads/main");
                &body["description"]
            }
        };
        *remote.body.lock().unwrap() = description.as_str().unwrap().to_owned();
        remote
            .exists
            .store(remote.fault != "missing", Ordering::SeqCst);
        if remote.fault == "secret" {
            *remote.body.lock().unwrap() = "synthetic-publication-token".into();
        }
        if remote.fault == "removed" {
            *remote.body.lock().unwrap() = "Human removed the marker".into();
        }
        if remote.fault == "redirect" {
            return (
                StatusCode::TEMPORARY_REDIRECT,
                [("location", "/unselected")],
            )
                .into_response();
        }
        if ["lost", "closed", "duplicate", "missing", "removed"].contains(&remote.fault) {
            return (StatusCode::BAD_GATEWAY, "synthetic-publication-token").into_response();
        }
        return (
            StatusCode::CREATED,
            Json(match remote.kind {
                ForgeKind::Github => json!({"number":7}),
                ForgeKind::Gitlab => json!({"iid":7}),
                ForgeKind::Azure => json!({"pullRequestId":7}),
            }),
        )
            .into_response();
    }
    assert_eq!(request.method(), axum::http::Method::GET);
    if remote.kind == ForgeKind::Gitlab && path.ends_with("/projects/group%2Frepo") {
        return Json(json!({"id":1,"path_with_namespace":"group/repo"})).into_response();
    }
    if remote.kind == ForgeKind::Azure && path.ends_with("/repositories/repo") {
        return Json(json!({"id":"synthetic-repository"})).into_response();
    }
    let collection = ["/pulls", "/merge_requests", "/pullrequests"]
        .iter()
        .any(|suffix| path.ends_with(suffix));
    let closed = remote.fault == "closed";
    let row = |number| {
        let description = remote.body.lock().unwrap().clone();
        match remote.kind {
            ForgeKind::Github => {
                json!({"number":number,"title":"Synthetic title","body":description,"draft":true,"state":if closed{"closed"}else{"open"},"merged":false,"head":{"ref":"task","sha":"a".repeat(40),"repo":{"full_name":"owner/repo"}},"base":{"ref":"main","sha":"b".repeat(40),"repo":{"full_name":"owner/repo"}}})
            }
            ForgeKind::Gitlab => {
                json!({"iid":number,"title":"Draft: Synthetic title","description":description,"draft":true,"state":if closed{"closed"}else{"opened"},"source_project_id":1,"target_project_id":1,"source_branch":"task","target_branch":"main","diff_refs":{"head_sha":"a".repeat(40),"start_sha":"b".repeat(40)}})
            }
            ForgeKind::Azure => {
                json!({"pullRequestId":number,"title":"Synthetic title","description":description,"isDraft":true,"status":if closed{"abandoned"}else{"active"},"repository":{"id":"synthetic-repository"},"sourceRefName":"refs/heads/task","targetRefName":"refs/heads/main","lastMergeSourceCommit":{"commitId":"a".repeat(40)},"lastMergeTargetCommit":{"commitId":"b".repeat(40)}})
            }
        }
    };
    if collection {
        let url = reqwest::Url::parse(&format!("http://127.0.0.1{uri}")).unwrap();
        let pairs = url
            .query_pairs()
            .collect::<std::collections::BTreeMap<_, _>>();
        let first = if remote.kind == ForgeKind::Azure {
            pairs.get("$skip").map(|value| value.as_ref()) == Some("0")
        } else {
            pairs.get("page").map(|value| value.as_ref()) == Some("1")
        };
        if remote.exists.load(Ordering::SeqCst) {
            assert_eq!(
                pairs
                    .get(if remote.kind == ForgeKind::Azure {
                        "searchCriteria.status"
                    } else {
                        "state"
                    })
                    .map(|value| value.as_ref()),
                Some("all")
            );
        }
        let rows = if first && remote.exists.load(Ordering::SeqCst) {
            if remote.fault == "duplicate" {
                vec![row(7), row(8)]
            } else {
                vec![row(7)]
            }
        } else {
            vec![]
        };
        return Json(if remote.kind == ForgeKind::Azure {
            json!({"value":rows})
        } else {
            json!(rows)
        })
        .into_response();
    }
    Json(row(path
        .rsplit('/')
        .next()
        .unwrap()
        .parse::<u64>()
        .unwrap()))
    .into_response()
}
#[tokio::test]
async fn publication_creation_records_one_attempt_and_reconciliation_requires_a_unique_marker_even_when_closed()
 {
    for kind in [ForgeKind::Github, ForgeKind::Gitlab, ForgeKind::Azure] {
        for fault in [
            "none",
            "lost",
            "closed",
            "duplicate",
            "missing",
            "secret",
            "removed",
            "redirect",
        ] {
            let remote = Remote {
                kind,
                fault,
                exists: Arc::new(AtomicBool::new(false)),
                writes: Arc::new(AtomicUsize::new(0)),
                body: Arc::new(Mutex::new(String::new())),
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
            let configuration = ForgeConfiguration {
                kind,
                base: format!("{address}{prefix}"),
            };
            let client = Forge::synthetic(
                configuration.clone(),
                "synthetic-publication-token".into(),
                1,
            )
            .unwrap();
            let preview = ForgePublicationPreview {
                configuration,
                revision: 1,
                repository: repository.into(),
                worktree: Worktree {
                    path: "synthetic-worktree".into(),
                    source: "synthetic-source".into(),
                    branch: "task".into(),
                    base: "main".into(),
                    base_commit: "b".repeat(40),
                    state: WorktreeState::Ready,
                },
                target: "main".into(),
                source_commit: "a".repeat(40),
                target_commit: "b".repeat(40),
                title: if kind == ForgeKind::Gitlab {
                    "Draft: Synthetic title"
                } else {
                    "Synthetic title"
                }
                .into(),
                body: "Human description é🦀".into(),
                draft: true,
            };
            let operation = uuid::Uuid::new_v4().to_string();
            let prepared = client
                .prepare_publication(&preview, &operation)
                .await
                .unwrap();
            assert_eq!(remote.writes.load(Ordering::SeqCst), 0);
            let created = client.create_publication(prepared).await;
            let known = if ["none", "secret"].contains(&fault) {
                Some(created.unwrap())
            } else {
                let error = created.err().unwrap().to_string();
                assert!(!error.contains("synthetic-publication-token"));
                None
            };
            assert_eq!(remote.writes.load(Ordering::SeqCst), 1);
            assert!(
                remote
                    .body
                    .lock()
                    .unwrap()
                    .contains(&publication_marker(&operation).unwrap())
                    || ["secret", "removed"].contains(&fault)
            );
            let found = client
                .reconcile_publication(&preview, &operation, known)
                .await;
            if ["duplicate", "secret"].contains(&fault) {
                let error = found.err().unwrap().to_string();
                assert!(!error.contains("synthetic-publication-token"));
            } else if ["missing", "removed"].contains(&fault) {
                assert!(found.unwrap().is_none());
            } else {
                let pr = found.unwrap().unwrap();
                assert_eq!(pr.number, 7);
                assert_eq!(
                    pr.status,
                    if fault == "closed" {
                        "abandoned"
                    } else {
                        "active"
                    }
                );
            }
            assert_eq!(remote.writes.load(Ordering::SeqCst), 1);
            server.abort();
            let _ = server.await;
        }
    }
}
