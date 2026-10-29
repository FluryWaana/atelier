use atelier_app::infrastructure::forge::Forge;
use atelier_core::contracts::{
    ForgeCheckScope as Scope, ForgeCheckState as State, ForgeConfiguration, ForgeKind,
    ForgeReviewDecision as Decision,
};
use axum::{
    Json, Router,
    extract::{Request, State as AppState},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
#[derive(Clone)]
struct Remote {
    kind: ForgeKind,
    fault: &'static str,
    pr_reads: Arc<AtomicUsize>,
}
async fn serve(AppState(remote): AppState<Remote>, request: Request) -> Response {
    assert_eq!(request.method(), axum::http::Method::GET);
    assert!(request.headers().contains_key("authorization"));
    let path = request.uri().path();
    let source = "a".repeat(40);
    let target = "b".repeat(40);
    if remote.kind == ForgeKind::Gitlab && path.ends_with("/projects/group%2Frepo") {
        return Json(json!({"id":1,"path_with_namespace":"group/repo"})).into_response();
    }
    if remote.kind == ForgeKind::Azure && path.ends_with("/repositories/repo") {
        return Json(json!({"id":"synthetic-repository"})).into_response();
    }
    if ["/pulls/7", "/merge_requests/7", "/pullrequests/7"]
        .iter()
        .any(|suffix| path.ends_with(suffix))
    {
        let number = remote.pr_reads.fetch_add(1, Ordering::SeqCst);
        let head = if remote.fault == "move" && number > 0 {
            target.clone()
        } else {
            source.clone()
        };
        let row = match remote.kind {
            ForgeKind::Github => {
                json!({"number":7,"title":"Synthetic PR","draft":true,"state":"open","merged":false,"head":{"ref":"task","sha":head,"repo":{"full_name":"owner/repo"}},"base":{"ref":"main","sha":target,"repo":{"full_name":"owner/repo"}}})
            }
            // The canonical current SHA takes precedence over the previous diff head.
            ForgeKind::Gitlab => {
                json!({"iid":7,"title":"Synthetic PR","draft":true,"state":"opened","source_branch":"task","target_branch":"main","sha":head,"diff_refs":{"head_sha":"c".repeat(40),"start_sha":target},"source_project_id":1,"target_project_id":1})
            }
            ForgeKind::Azure => {
                json!({"pullRequestId":7,"title":"Synthetic PR","isDraft":true,"status":"active","sourceRefName":"refs/heads/task","targetRefName":"refs/heads/main","lastMergeSourceCommit":{"commitId":head},"lastMergeTargetCommit":{"commitId":target},"repository":{"id":"synthetic-repository","project":{"id":"00000000-0000-4000-8000-000000000001"}},"reviewers":if remote.fault=="reviewers"{Value::Null}else{json!([{"id":"synthetic-user","displayName":"Synthetic reviewer","vote":10}])}})
            }
        };
        return Json(row).into_response();
    }
    if remote.kind == ForgeKind::Azure {
        if path.ends_with("/policy/evaluations") {
            let url = reqwest::Url::parse(&format!("http://127.0.0.1{}", request.uri())).unwrap();
            let query = url
                .query_pairs()
                .collect::<std::collections::BTreeMap<_, _>>();
            assert_eq!(
                query.get("api-version").map(|value| value.as_ref()),
                Some("7.1-preview.1")
            );
            let artifact =
                "vstfs:///CodeReview/CodeReviewId/00000000-0000-4000-8000-000000000001/7";
            assert_eq!(
                query.get("artifactId").map(|value| value.as_ref()),
                Some(artifact)
            );
            if query
                .get("$skip")
                .is_some_and(|value| value.as_ref() != "0")
                && remote.fault != "policy-repeat"
            {
                return Json(json!({"count":0,"value":[]})).into_response();
            }
            return Json(json!({"count":1,"value":[{"evaluationId":"00000000-0000-4000-8000-000000000002","artifactId":if remote.fault=="policy-scope"{"wrong"}else{artifact},"configuration":{"isEnabled":true,"isBlocking":true,"type":{"displayName":"Synthetic branch policy"}},"status":"approved"}]})).into_response();
        }
        if path.ends_with("/iterations") {
            let rows = json!([{"id":1,"sourceRefCommit":{"commitId":target}},{"id":2,"sourceRefCommit":{"commitId":if remote.fault=="scope"{target}else{source}}}]);
            return Json(json!({"count":2,"value":rows})).into_response();
        }
        assert!(path.ends_with("/statuses"));
        let mut rows = vec![
            json!({"id":1,"iterationId":1,"context":{"genre":"ci","name":"Old"},"state":"succeeded"}),
            json!({"id":2,"iterationId":2,"context":{"genre":"ci","name":"Build"},"state":"failed"}),
            json!({"id":3,"iterationId":2,"context":{"genre":"ci","name":"Build"},"state":"succeeded","description":if remote.fault=="secret"{"synthetic-checks-token"}else{"Source result"}}),
            json!({"id":4,"iterationId":0,"context":{"genre":"ci","name":"Global"},"state":"succeeded"}),
        ];
        if remote.fault == "repeat" {
            rows.push(rows[3].clone());
        }
        let count = if remote.fault == "count" {
            rows.len() + 1
        } else {
            rows.len()
        };
        let response = Json(json!({"value":rows,"count":count}));
        return if remote.fault == "continuation" {
            ([("x-ms-continuationtoken", "synthetic-next")], response).into_response()
        } else {
            response.into_response()
        };
    }
    if remote.kind == ForgeKind::Gitlab && path.ends_with("/approvals") {
        if remote.fault == "reviewers" {
            return (StatusCode::FORBIDDEN, "synthetic-checks-token").into_response();
        }
        return Json(json!({"approved_by":[{"user":{"id":1,"username":"Synthetic reviewer"}}]}))
            .into_response();
    }
    let uri = reqwest::Url::parse(&format!("http://127.0.0.1{}", request.uri())).unwrap();
    let query = uri
        .query_pairs()
        .collect::<std::collections::BTreeMap<_, _>>();
    let page = query.get("page").unwrap().parse::<u64>().unwrap();
    assert_eq!(
        query.get("per_page").map(|value| value.as_ref()),
        Some("100")
    );
    if remote.kind == ForgeKind::Github {
        if path.ends_with("/statuses") {
            assert!(path.contains(&source));
            let offset = (page - 1) * 100;
            let mut rows=(offset+1..=(offset+100).min(101)).map(|id|{let id=if remote.fault=="repeat"&&page>1{1}else{id};json!({"id":id,"context":"Build","state":if id==101{"success"}else{"failure"},"description":if remote.fault=="secret"{"synthetic-checks-token"}else{"Historical status"}})}).collect::<Vec<_>>();
            if remote.fault == "oversize" && page == 1 {
                rows.push(json!({"id":102,"context":"Extra","state":"success"}));
            }
            return Json(rows).into_response();
        }
        if path.ends_with("/check-runs") {
            assert!(path.contains(&source));
            assert_eq!(
                query.get("filter").map(|value| value.as_ref()),
                Some("latest")
            );
            return Json(json!({"check_runs":if page==1{json!([{"id":1,"head_sha":if remote.fault=="scope"{target}else{source.clone()},"name":"Neutral","status":"completed","conclusion":"neutral","output":{"title":"Observed neutral"}},{"id":2,"head_sha":source,"name":"Running","status":"in_progress","conclusion":null}])}else{json!([])}})).into_response();
        }
        assert!(path.ends_with("/reviews"));
        if remote.fault == "reviewers" {
            return (StatusCode::FORBIDDEN, "synthetic-checks-token").into_response();
        }
        return Json(if page==1{json!([{"id":1,"user":{"login":"Current"},"state":"CHANGES_REQUESTED","commit_id":source,"submitted_at":"2026-10-25T08:30:00-02:00"},{"id":2,"user":{"login":"Current"},"state":"APPROVED","commit_id":target.clone(),"submitted_at":"2026-10-25T10:00:00Z"},{"id":3,"user":{"login":"Old"},"state":"APPROVED","commit_id":target,"submitted_at":"2026-10-25T10:00:00Z"}])}else{json!([])}).into_response();
    }
    assert!(path.ends_with("/pipelines"));
    let rows = if page == 1 {
        if remote.fault == "old" {
            json!([{"id":9,"sha":target,"status":"success","ref":"task"}])
        } else {
            json!([{"id":1,"sha":source.clone(),"status":"failed","ref":"task"},{"id":2,"sha":source,"status":"success","ref":if remote.fault=="secret"{"synthetic-checks-token"}else{"task"}},{"id":3,"sha":target,"status":"success","ref":"task"}])
        }
    } else if remote.fault == "repeat" && page == 2 {
        json!([{"id":1,"sha":source,"status":"success"}])
    } else {
        json!([])
    };
    Json(rows).into_response()
}
#[tokio::test]
async fn observed_checks_bind_commits_latest_decisions_and_explicit_unavailable_data_without_running_ci()
 {
    for kind in [ForgeKind::Github, ForgeKind::Gitlab, ForgeKind::Azure] {
        let faults = match kind {
            ForgeKind::Github => vec![
                "none",
                "reviewers",
                "secret",
                "repeat",
                "scope",
                "move",
                "oversize",
            ],
            ForgeKind::Gitlab => vec!["none", "old", "reviewers", "secret", "repeat", "move"],
            ForgeKind::Azure => vec![
                "none",
                "reviewers",
                "secret",
                "repeat",
                "scope",
                "move",
                "count",
                "continuation",
                "policy-repeat",
                "policy-scope",
            ],
        };
        for fault in faults {
            let remote = Remote {
                kind,
                fault,
                pr_reads: Arc::new(AtomicUsize::new(0)),
            };
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = format!("http://{}", listener.local_addr().unwrap());
            let app = Router::new()
                .fallback(axum::routing::any(serve))
                .with_state(remote);
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
                "synthetic-checks-token".into(),
                1,
            )
            .unwrap();
            let result = client.checks(repository, 7).await;
            if ["none", "reviewers", "old"].contains(&fault) {
                let report = result.unwrap();
                assert_eq!(report.pull_request.source_commit, Some("a".repeat(40)));
                assert_eq!(report.reviewers_available, fault != "reviewers");
                assert!(
                    !serde_json::to_string(&report)
                        .unwrap()
                        .contains("synthetic-checks-token")
                );
                if kind == ForgeKind::Github {
                    assert_eq!(report.checks.len(), 3);
                    assert!(
                        report
                            .checks
                            .iter()
                            .any(|check| check.name == "Build" && check.state == State::Succeeded)
                    );
                    assert!(
                        report
                            .checks
                            .iter()
                            .any(|check| check.state == State::Skipped)
                    );
                    assert!(
                        report
                            .checks
                            .iter()
                            .any(|check| check.state == State::Running)
                    );
                    if fault != "reviewers" {
                        assert!(
                            report
                                .reviewers
                                .iter()
                                .any(|reviewer| reviewer.name == "Current"
                                    && reviewer.decision == Decision::ChangesRequested
                                    && reviewer.scope == Scope::SourceCommit)
                        );
                        assert!(
                            report
                                .reviewers
                                .iter()
                                .any(|reviewer| reviewer.name == "Old"
                                    && reviewer.scope == Scope::OtherSnapshot)
                        );
                    }
                } else if kind == ForgeKind::Gitlab {
                    if fault == "old" {
                        assert!(report.checks.is_empty());
                    } else {
                        assert_eq!(report.checks.len(), 1);
                        assert_eq!(report.checks[0].state, State::Succeeded);
                    }
                    assert_eq!(report.other_snapshot_count, 1);
                    assert!(
                        report
                            .reviewers
                            .iter()
                            .all(|reviewer| reviewer.scope == Scope::Unscoped)
                    );
                } else {
                    assert_eq!(report.checks.len(), 3);
                    let policy = report
                        .checks
                        .iter()
                        .find(|check| check.id.starts_with("policy:"))
                        .unwrap();
                    assert_eq!(policy.scope, Scope::Unscoped);
                    assert_eq!(policy.commit, None);
                    assert_eq!(policy.state, State::Succeeded);
                    assert!(policy.description.is_empty());
                    assert_eq!(
                        policy.description_key.as_deref(),
                        Some("message.active_blocking_policy_pr_result_with_no_commit_provided")
                    );
                    assert_eq!(report.other_snapshot_count, 1);
                    assert!(
                        report
                            .checks
                            .iter()
                            .any(|check| check.scope == Scope::SourceCommit
                                && check.state == State::Succeeded)
                    );
                    assert!(
                        report
                            .checks
                            .iter()
                            .any(|check| check.scope == Scope::Unscoped && check.commit.is_none())
                    );
                    assert!(
                        report
                            .reviewers
                            .iter()
                            .all(|reviewer| reviewer.scope == Scope::Unscoped)
                    );
                }
            } else {
                let error = result.err().unwrap().to_string();
                assert!(!error.contains("synthetic-checks-token"), "{fault}");
            }
            server.abort();
            let _ = server.await;
        }
    }
}
