use atelier_app::infrastructure::forge::Forge;
use atelier_core::contracts::{ForgeConfiguration, ForgeKind};
use axum::{
    Json, Router,
    extract::{Request, State},
    response::{IntoResponse, Response},
};
use serde_json::json;
#[derive(Clone)]
struct Remote {
    kind: ForgeKind,
    fault: &'static str,
}
async fn serve(State(remote): State<Remote>, request: Request) -> Response {
    assert_eq!(request.method(), "GET");
    assert!(request.headers().contains_key("authorization"));
    let path = request.uri().path();
    if remote.kind == ForgeKind::Gitlab && path.ends_with("/projects/group%2Fnested%2Frepo") {
        return Json(json!({"id":1,"path_with_namespace":"group/nested/repo"})).into_response();
    }
    if remote.kind == ForgeKind::Azure && path.ends_with("/repositories/repo") {
        return Json(json!({"id":"synthetic-repository"})).into_response();
    }
    let url = reqwest::Url::parse(&format!("http://127.0.0.1{}", request.uri())).unwrap();
    let query = url
        .query_pairs()
        .collect::<std::collections::BTreeMap<_, _>>();
    let page_size = if remote.fault == "short" { 20 } else { 100 };
    let offset = if remote.kind == ForgeKind::Azure {
        assert_eq!(
            query
                .get("searchCriteria.status")
                .map(|value| value.as_ref()),
            Some("active")
        );
        assert_eq!(query.get("$top").map(|value| value.as_ref()), Some("100"));
        query.get("$skip").unwrap().parse::<u64>().unwrap()
    } else {
        assert_eq!(
            query.get("per_page").map(|value| value.as_ref()),
            Some("100")
        );
        (query.get("page").unwrap().parse::<u64>().unwrap() - 1) * page_size
    };
    let range = offset + 1..=(offset + page_size).min(101);
    if offset >= 101 {
        return Json(if remote.kind == ForgeKind::Azure {
            json!({"value":[]})
        } else {
            json!([])
        })
        .into_response();
    }
    let rows=range.map(|number|{
        let number=if remote.fault=="repeat"&&offset>0{1}else{number};
        let title=if remote.fault=="secret"{"synthetic-list-token".into()}else{format!("Synthetic PR {number} é🦀")};
        match remote.kind {
            ForgeKind::Github=>{assert_eq!(path,"/api/v3/repos/owner/repo/pulls");json!({"number":number,"title":title,"draft":true,"state":"open","head":{"ref":"task"},"base":{"ref":"main","repo":{"full_name":if remote.fault=="identity"{"other/repo"}else{"owner/repo"}}}})},
            ForgeKind::Gitlab=>{assert_eq!(path,"/prefix/api/v4/projects/group%2Fnested%2Frepo/merge_requests");json!({"iid":number,"title":title,"draft":true,"state":"opened","source_branch":"task","target_branch":"main","target_project_id":if remote.fault=="identity"{2}else{1}})},
            ForgeKind::Azure=>{assert_eq!(path,"/org/project/_apis/git/repositories/repo/pullrequests");json!({"pullRequestId":number,"title":title,"isDraft":true,"status":"active","sourceRefName":"refs/heads/task","targetRefName":"refs/heads/main","repository":{"id":if remote.fault=="identity"{"other"}else{"synthetic-repository"}}})},
        }
    }).collect::<Vec<_>>();
    if remote.fault == "shape" {
        return Json(json!({"invalid":"page"})).into_response();
    }
    Json(if remote.kind == ForgeKind::Azure {
        json!({"value":rows})
    } else {
        json!(rows)
    })
    .into_response()
}
#[tokio::test]
async fn open_pull_request_lists_validate_pages_identity_secrets_and_complete_results() {
    for kind in [ForgeKind::Github, ForgeKind::Gitlab, ForgeKind::Azure] {
        for fault in ["none", "short", "repeat", "identity", "secret", "shape"] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = format!("http://{}", listener.local_addr().unwrap());
            let app = Router::new()
                .fallback(serve)
                .with_state(Remote { kind, fault });
            let server = tokio::spawn(async move {
                axum::serve(listener, app).await.unwrap();
            });
            let (prefix, repo) = match kind {
                ForgeKind::Github => ("", "owner/repo"),
                ForgeKind::Gitlab => ("/prefix", "group/nested/repo"),
                ForgeKind::Azure => ("/org", "project/repo"),
            };
            let client = Forge::synthetic(
                ForgeConfiguration {
                    kind,
                    base: format!("{address}{prefix}"),
                },
                "synthetic-list-token".into(),
                1,
            )
            .unwrap();
            let result = client.list_pull_requests(repo).await;
            if ["none", "short"].contains(&fault) {
                let result = result.unwrap();
                assert_eq!(result.len(), 101);
                assert_eq!(result[100].number, 101);
                assert_eq!(result[100].source_branch, "task");
                assert!(
                    !serde_json::to_string(&result)
                        .unwrap()
                        .contains("synthetic-list-token")
                );
                assert!(result.iter().all(|pr| pr.draft));
            } else {
                assert!(result.is_err(), "{fault}");
                assert!(
                    !result
                        .err()
                        .unwrap()
                        .to_string()
                        .contains("synthetic-list-token")
                );
            }
            server.abort();
            let _ = server.await;
        }
    }
}
