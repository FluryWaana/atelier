use atelier_app::infrastructure::{
    database::Database,
    forge::{Forge, same_pull_request},
    forge_draft as workflow,
    store::Documents,
};
use atelier_core::{
    contracts::{
        Card, ForgeConfiguration, ForgeDraftIntent, ForgeDraftRequest, ForgeKind,
        ForgeWriteState as Phase,
    },
    document::Document,
};
use axum::{
    Json, Router,
    extract::{Request, State},
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

#[derive(Clone)]
struct Remote {
    kind: ForgeKind,
    draft: Arc<AtomicBool>,
    writes: Arc<AtomicUsize>,
    fault: &'static str,
}
async fn remote(State(remote): State<Remote>, request: Request) -> Response {
    assert!(request.headers().contains_key("authorization"));
    let path = request.uri().path().to_owned();
    let number = 7;
    if request.method() != axum::http::Method::GET {
        let body = axum::body::to_bytes(request.into_body(), 16384)
            .await
            .unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert!(!body.to_string().contains("synthetic-write-token"));
        remote.writes.fetch_add(1, Ordering::SeqCst);
        let draft = match remote.kind {
            ForgeKind::Github => {
                assert_eq!(path, "/api/graphql");
                assert_eq!(
                    body["variables"]["input"]["pullRequestId"],
                    "synthetic-node"
                );
                body["query"]
                    .as_str()
                    .unwrap()
                    .contains("convertPullRequestToDraft")
            }
            ForgeKind::Gitlab => {
                assert_eq!(path, "/prefix/api/graphql");
                assert_eq!(body["variables"]["input"]["projectPath"], "group/repo");
                assert_eq!(body["variables"]["input"]["iid"], "7");
                body["variables"]["input"]["draft"].as_bool().unwrap()
            }
            ForgeKind::Azure => {
                assert_eq!(
                    path,
                    "/org/project/_apis/git/repositories/repo/pullrequests/7"
                );
                assert_eq!(body.as_object().unwrap().len(), 1);
                body["isDraft"].as_bool().unwrap()
            }
        };
        remote.draft.store(draft, Ordering::SeqCst);
        if remote.fault == "lost" {
            return (
                axum::http::StatusCode::BAD_GATEWAY,
                "synthetic-write-token must never escape",
            )
                .into_response();
        }
        if remote.fault == "redirect" {
            return (
                axum::http::StatusCode::TEMPORARY_REDIRECT,
                [("location", "/must-not-follow")],
                "synthetic-write-token",
            )
                .into_response();
        }
        if remote.fault == "graphql-error" && remote.kind != ForgeKind::Azure {
            return Json(json!({"errors":[{"message":"synthetic-write-token"}],"data":null}))
                .into_response();
        }
        let operation = body["variables"]["input"]["clientMutationId"].clone();
        let mutation = if draft {
            "convertPullRequestToDraft"
        } else {
            "markPullRequestReadyForReview"
        };
        return Json(match remote.kind {
            ForgeKind::Github=>json!({"data":{mutation:{"clientMutationId":operation,"pullRequest":{"isDraft":draft}}}}),
            ForgeKind::Gitlab=>json!({"data":{"mergeRequestSetDraft":{"clientMutationId":operation,"errors":[],"mergeRequest":{"draft":draft}}}}),
            ForgeKind::Azure=>json!({"isDraft":draft}),
        }).into_response();
    }
    if remote.kind == ForgeKind::Gitlab && path.ends_with("/projects/group%2Frepo") {
        return Json(json!({"id":1,"path_with_namespace":"group/repo"})).into_response();
    }
    if remote.kind == ForgeKind::Azure && path.ends_with("/repositories/repo") {
        return Json(json!({"id":"synthetic-repository"})).into_response();
    }
    let draft = remote.draft.load(Ordering::SeqCst);
    let source = "a".repeat(40);
    let target = "b".repeat(40);
    Json(match remote.kind {
        ForgeKind::Github=>json!({"number":number,"node_id":"synthetic-node","title":"Synthetic title","state":"open","merged":false,"draft":draft,"head":{"ref":"task","sha":source,"repo":{"full_name":"owner/repo"}},"base":{"ref":"main","sha":target,"repo":{"full_name":"owner/repo"}}}),
        ForgeKind::Gitlab=>json!({"iid":number,"title":"Synthetic title","state":"opened","draft":draft,"source_branch":"task","target_branch":"main","source_project_id":1,"target_project_id":1,"diff_refs":{"head_sha":source,"start_sha":target}}),
        ForgeKind::Azure=>json!({"pullRequestId":number,"title":"Synthetic title","status":"active","isDraft":draft,"sourceRefName":"refs/heads/task","targetRefName":"refs/heads/main","repository":{"id":"synthetic-repository"},"lastMergeSourceCommit":{"commitId":source},"lastMergeTargetCommit":{"commitId":target}}),
    }).into_response()
}
#[tokio::test]
async fn draft_writes_are_single_attempt_and_lost_responses_can_be_observed_without_replay() {
    for kind in [ForgeKind::Github, ForgeKind::Gitlab, ForgeKind::Azure] {
        for fault in ["none", "lost", "redirect", "graphql-error"] {
            let remote = Remote {
                kind,
                draft: Arc::new(AtomicBool::new(true)),
                writes: Arc::new(AtomicUsize::new(0)),
                fault,
            };
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = format!("http://{}", listener.local_addr().unwrap());
            let app = Router::new()
                .fallback(axum::routing::any(self::remote))
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
                "synthetic-write-token".into(),
                1,
            )
            .unwrap();
            let expected = client.pull_request(repository, 7).await.unwrap();
            let mut stale = expected.clone();
            stale.source_commit = Some("c".repeat(40));
            assert!(
                client
                    .prepare_draft(&stale, false, &uuid::Uuid::new_v4().to_string())
                    .await
                    .is_err()
            );
            assert_eq!(remote.writes.load(Ordering::SeqCst), 0);
            let prepared = client
                .prepare_draft(&expected, false, &uuid::Uuid::new_v4().to_string())
                .await
                .unwrap();
            assert_eq!(remote.writes.load(Ordering::SeqCst), 0);
            let result = client.send_draft(prepared).await;
            if fault != "none" && !(fault == "graphql-error" && kind == ForgeKind::Azure) {
                let error = result.err().unwrap().to_string();
                assert!(!error.contains("synthetic-write-token"));
            } else {
                assert!(!result.unwrap().draft);
            }
            assert_eq!(remote.writes.load(Ordering::SeqCst), 1);
            let observed = client.pull_request(repository, 7).await.unwrap();
            assert!(!observed.draft);
            assert!(!same_pull_request(&expected, &observed));
            assert_eq!(remote.writes.load(Ordering::SeqCst), 1);
            // Converting back uses the dedicated mutation and never replaces title/body.
            if fault == "none" {
                let prepared = client
                    .prepare_draft(&observed, true, &uuid::Uuid::new_v4().to_string())
                    .await
                    .unwrap();
                assert!(client.send_draft(prepared).await.unwrap().draft);
                assert_eq!(remote.writes.load(Ordering::SeqCst), 2);
            }
            server.abort();
            let _ = server.await;
        }
    }
}

#[tokio::test]
async fn content_recovery_requires_the_requested_text_and_preserves_human_card_fields() {
    let database = Database::open(std::path::Path::new(":memory:")).unwrap();
    database.call(|store| {
        store.save_settings(&document(json!({"forgeRevision":1,"forgeConnections":{"github":{"kind":"github","base":"https://github.example.test"}}})))?;
        let pr=serde_json::from_value::<atelier_core::contracts::PullRequest>(json!({"revision":1,"kind":"github","repository":"owner/repo","number":7,"title":"Old title","body":"Human body","url":"https://github.example.test/owner/repo/pull/7","status":"active","draft":true,"sourceBranch":"task","targetBranch":"main","sourceCommit":"a".repeat(40),"targetCommit":"b".repeat(40),"fork":false,"baseUrl":"https://github.example.test","checkedAt":"2026-10-25T10:00:00Z"}))?;
        let card:Card=serde_json::from_str(&json!({"id":"task","title":"Human local title","description":"Human local description","pullRequest":pr}).to_string())?;
        store.put(Documents::Cards,&card)?;
        let input=ForgeDraftRequest{expected:card,revision:1,draft:true,confirmed:true,content:Some(atelier_core::contracts::ForgeContent{title:"New title".into(),body:"Human body".into()})};
        let id=uuid::Uuid::new_v4().to_string();
        let reserved=workflow::reserve(store,&input,&id)?;
        workflow::sending(store,&reserved,&id)?;
        workflow::recover(store)?;
        let unchanged=workflow::record(store,"task",&id,pr.clone(),true)?;
        assert_eq!(unchanged.fields.get::<ForgeDraftIntent>("forgeDraft")?.unwrap().state,Phase::Uncertain);
        let mut observed=pr;observed.title="New title".into();
        let result=workflow::record(store,"task",&id,observed,true)?;
        assert_eq!(result.fields.get::<ForgeDraftIntent>("forgeDraft")?.unwrap().state,Phase::Reconciled);
        assert_eq!(result.title,atelier_core::document::JsonString::from("Human local title"));
        assert!(result.fields.string_equals("description","Human local description")?);
        Ok(())
    }).await.unwrap();
}
fn document(value: Value) -> Document {
    serde_json::from_str(&value.to_string()).unwrap()
}
#[tokio::test]
async fn persistent_intents_protect_work_recover_uncertainty_and_never_repeat_a_write() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().to_owned();
    let database = Database::open(std::path::Path::new(":memory:")).unwrap();
    database.call(move|store| {
        let backups=atelier_app::infrastructure::backups::BackupDirectory::open(&path)?;
        store.save_settings(&document(json!({"forgeRevision":1,"forgeConnections":{"github":{"kind":"github","base":"https://github.example.test"}}})))?;
        let pr=serde_json::from_value::<atelier_core::contracts::PullRequest>(json!({"revision":1,"kind":"github","repository":"owner/repo","number":7,"title":"Synthetic title","url":"https://github.example.test/owner/repo/pull/7","status":"active","draft":true,"sourceBranch":"task","targetBranch":"main","sourceCommit":"a".repeat(40),"targetCommit":"b".repeat(40),"fork":false,"baseUrl":"https://github.example.test","checkedAt":"2026-10-25T10:00:00Z"}))?;
        let card:Card=serde_json::from_str(&json!({"id":"task","title":"Human title","description":"Human text","checklist":[{"text":"Human criterion","done":true}],"pullRequest":pr}).to_string())?;
        store.put(Documents::Cards,&card)?;
        let old_backup=backups.create(store)?;
        store.put(Documents::Runs,&document(json!({"id":"review","cardId":"task","kind":"pr-review","status":"completed"})))?;
        let mut input=ForgeDraftRequest{expected:card,revision:1,draft:false,confirmed:false,content:None};
        let operation=uuid::Uuid::new_v4().to_string();
        assert!(workflow::reserve(store,&input,&operation).is_err());input.confirmed=true;
        workflow::reserve(store,&input,&operation)?;
        assert!(backups.restore(store,&old_backup.name).is_err());
        assert!(workflow::reserve(store,&input,&operation).is_err());
        for path in ["/api/cards/task","/api/cards/task/runs/start","/api/cards/task/worktree/remove","/api/cards/task/forge/detach","/api/projects","/api/backups/restore"]{assert!(workflow::blocks(store,path)?);}
        assert!(!workflow::blocks(store,"/api/cards/task/forge/draft-reconcile")?);assert!(!workflow::blocks(store,"/api/cards/other")?);
        workflow::recover(store)?;
        let recovered=store.document::<Card>(Documents::Cards,"task")?.unwrap();assert_eq!(recovered.fields.get::<ForgeDraftIntent>("forgeDraft")?.unwrap().state,Phase::Failed);
        input.expected=recovered;let reserved=workflow::reserve(store,&input,&operation)?;
        workflow::sending(store,&reserved,&operation)?;
        assert!(store.document::<Document>(Documents::Runs,"review")?.unwrap().raw("reviewStaleReason").is_some());
        workflow::recover(store)?;
        let recovered=store.document::<Card>(Documents::Cards,"task")?.unwrap();assert_eq!(recovered.fields.get::<ForgeDraftIntent>("forgeDraft")?.unwrap().state,Phase::Uncertain);
        let unchanged=workflow::record(store,"task",&operation,pr.clone(),true)?;
        assert_eq!(unchanged.fields.get::<ForgeDraftIntent>("forgeDraft")?.unwrap().state,Phase::Uncertain);
        let mut wrong=pr.clone();wrong.repository="other/repo".into();assert!(workflow::record(store,"task",&operation,wrong,true).is_err());
        let mut observed=pr;observed.draft=false;
        let resolved=workflow::record(store,"task",&operation,observed,true)?;
        assert_eq!(resolved.fields.get::<ForgeDraftIntent>("forgeDraft")?.unwrap().state,Phase::Reconciled);
        assert!(!workflow::blocks(store,"/api/cards/task")?);
        assert!(resolved.fields.string_equals("description","Human text")?);
        assert!(resolved.fields.get::<Vec<atelier_core::domain::checklist::Criterion>>("checklist")?.unwrap()[0].done);
        assert!(workflow::sending(store,&reserved,&operation).is_err());
        assert!(backups.restore(store,&old_backup.name).is_err());
        let effect=resolved.fields.get::<Value>("forgeDraftEffect")?.unwrap();
        input.expected=resolved;input.draft=true;
        let next=uuid::Uuid::new_v4().to_string();
        workflow::reserve(store,&input,&next)?;
        let failed=workflow::interrupted(store,"task",&next)?;
        assert_eq!(failed.fields.get::<Value>("forgeDraftEffect")?,Some(effect));
        assert!(backups.restore(store,&old_backup.name).is_err());
        Ok(())
    }).await.unwrap();
    database.close().await.unwrap();
}
