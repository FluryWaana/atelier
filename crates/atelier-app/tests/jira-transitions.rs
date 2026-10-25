use atelier_app::infrastructure::{
    database::Database, jira::Jira, jira_transition as workflow, store::Documents,
};
use atelier_core::{
    contracts::{
        Card, JiraConfiguration, JiraMode, JiraStatus, JiraTransition, JiraTransitionIntent,
        JiraTransitionRequest, JiraTransitions, JiraWriteState as Phase,
    },
    document::Document,
};
use axum::{
    Json, Router,
    extract::{Query, State},
    http::{HeaderMap, StatusCode, header},
    routing::get,
};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
#[derive(Clone)]
struct Remote {
    status: Arc<AtomicUsize>,
    writes: Arc<AtomicUsize>,
    lost: bool,
}
fn auth(headers: &HeaderMap) {
    assert_eq!(headers[header::AUTHORIZATION], "Bearer synthetic-token");
}
async fn status(
    State(remote): State<Remote>,
    headers: HeaderMap,
    Query(query): Query<std::collections::BTreeMap<String, String>>,
) -> Json<Value> {
    auth(&headers);
    assert_eq!(query.get("fields").map(String::as_str), Some("status"));
    let id = remote.status.load(Ordering::SeqCst);
    Json(
        json!({"id":"1","fields":{"status":{"id":id.to_string(),"name":if id==1{"Open"}else{"Done"}}}}),
    )
}
async fn choices(
    headers: HeaderMap,
    Query(query): Query<std::collections::BTreeMap<String, String>>,
) -> Json<Value> {
    auth(&headers);
    assert_eq!(
        query.get("expand").map(String::as_str),
        Some("transitions.fields")
    );
    Json(
        json!({"transitions":[{"id":"11","name":"Complete","to":{"id":"2","name":"Done"},"fields":{}},{"id":"12","name":"Needs input","to":{"id":"2","name":"Done"},"fields":{"resolution":{"required":true,"hasDefaultValue":false}}}]}),
    )
}
async fn send(
    State(remote): State<Remote>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> StatusCode {
    auth(&headers);
    assert_eq!(body, json!({"transition":{"id":"11"}}));
    remote.writes.fetch_add(1, Ordering::SeqCst);
    remote.status.store(2, Ordering::SeqCst);
    if remote.lost {
        StatusCode::INTERNAL_SERVER_ERROR
    } else {
        StatusCode::NO_CONTENT
    }
}
fn status_value(id: &str, name: &str) -> JiraStatus {
    JiraStatus {
        id: id.into(),
        name: name.into(),
    }
}
fn transition() -> JiraTransition {
    JiraTransition {
        id: "11".into(),
        name: "Complete".into(),
        target: status_value("2", "Done"),
        requires_fields: false,
    }
}
fn document(value: Value) -> Document {
    serde_json::from_str(&value.to_string()).unwrap()
}
fn request(base: &str) -> JiraTransitionRequest {
    JiraTransitionRequest{revision:1,expected:serde_json::from_str(&json!({"id":"card","title":"Synthetic","status":"todo","ticket":{"connector":"jira","instance":base,"id":"1","key":"SYN-1","status":"Open"}}).to_string()).unwrap(),from:status_value("1","Open"),transition:transition()}
}

#[tokio::test]
async fn synthetic_writes_are_reserved_once_recovered_and_reconciled_without_resubmission() {
    for lost in [false, true] {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("synthetic.sqlite");
        let backups = temporary.path().to_owned();
        let remote = Remote {
            status: Arc::new(AtomicUsize::new(1)),
            writes: Arc::new(AtomicUsize::new(0)),
            lost,
        };
        let writes = remote.writes.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let base = format!("http://{address}");
        let app = Router::new()
            .route("/rest/api/2/issue/1", get(status))
            .route("/rest/api/2/issue/1/transitions", get(choices).post(send))
            .with_state(remote);
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let configuration = JiraConfiguration {
            mode: JiraMode::DataCenter,
            url: base.clone(),
            email: String::new(),
            query: "project = SYN".into(),
            revision: 1,
        };
        let initial = configuration.clone();
        let input = request(&base);
        let seeded = input.clone();
        let database = Database::open(&path).unwrap();
        let backup = database
            .call(move |store| {
                store.save_settings(&document(json!({"jiraConnection":initial})))?;
                store.put(Documents::Cards, &seeded.expected)?;
                let backup = atelier_app::infrastructure::backups::BackupDirectory::open(&backups)?
                    .create(store)?;
                workflow::reserve(store, &seeded, "operation", "synthetic-time")?;
                assert!(workflow::reserve(store, &seeded, "duplicate", "time").is_err());
                Ok(backup.name)
            })
            .await
            .unwrap();
        let client = Jira::synthetic(configuration, "synthetic-token".into()).unwrap();
        let options = client.transitions("1").await.unwrap();
        assert_eq!(options.transitions.len(), 2);
        assert!(options.transitions[1].requires_fields);
        assert!(
            client
                .send_transition("1", &options.transitions[1])
                .await
                .is_err()
        );
        assert_eq!(writes.load(Ordering::SeqCst), 0);
        database
            .call(move |store| workflow::sending(store, "card", "operation", &options))
            .await
            .unwrap();
        let sent = client.send_transition("1", &input.transition).await;
        assert_eq!(sent.is_err(), lost);
        assert_eq!(writes.load(Ordering::SeqCst), 1);
        if !lost {
            let observed = client.status("1").await.unwrap();
            database
                .call(move |store| {
                    workflow::complete(store, "card", "operation", Phase::Confirmed, Some(observed))
                })
                .await
                .unwrap();
        }
        database.close().await.unwrap();
        drop(database);
        let database = Database::open(&path).unwrap();
        database
            .call(move |store| {
                workflow::recover(store)?;
                let card = store.document::<Card>(Documents::Cards, "card")?.unwrap();
                let intent = card
                    .fields
                    .get::<JiraTransitionIntent>("jiraTransition")?
                    .unwrap();
                assert_eq!(
                    intent.state,
                    if lost {
                        Phase::Uncertain
                    } else {
                        Phase::Confirmed
                    }
                );
                assert_eq!(workflow::blocks(store, "/api/backups/restore")?, lost);
                assert_eq!(workflow::blocks(store, "/api/cards/card")?, lost);
                assert!(!workflow::blocks(store, "/api/cards/card/jira/reconcile")?);
                assert!(!workflow::blocks(store, "/api/jira/config")?);
                Ok(())
            })
            .await
            .unwrap();
        if lost {
            let observed = client.status("1").await.unwrap();
            database
                .call(move |store| {
                    workflow::complete(store, "card", "operation", Phase::Confirmed, Some(observed))
                })
                .await
                .unwrap();
        }
        let backups = temporary.path().to_owned();
        database
            .call(move |store| {
                let directory =
                    atelier_app::infrastructure::backups::BackupDirectory::open(&backups)?;
                assert!(directory.restore(store, &backup).is_err());
                let card = store.document::<Card>(Documents::Cards, "card")?.unwrap();
                assert!(
                    card.fields
                        .get::<JiraTransitionIntent>("jiraTransition")?
                        .is_some_and(|intent| intent.state == Phase::Confirmed)
                );
                assert!(card.fields.string_equals("status", "todo")?);
                assert_eq!(
                    card.fields
                        .get::<Document>("ticket")?
                        .unwrap()
                        .get::<String>("status")?
                        .as_deref(),
                    Some("Done")
                );
                Ok(())
            })
            .await
            .unwrap();
        assert_eq!(writes.load(Ordering::SeqCst), 1);
        database.close().await.unwrap();
        server.abort();
        let _ = server.await;
    }
}

#[tokio::test]
async fn preparation_staleness_required_fields_and_self_transition_recovery_fail_closed() {
    let database = Database::open(std::path::Path::new(":memory:")).unwrap();
    database.call(|store|{
    let input=request("https://synthetic.example.test");store.save_settings(&document(json!({"jiraConnection":{"mode":"dataCenter","url":"https://synthetic.example.test","email":"","query":"project = SYN","revision":1}})))?;store.put(Documents::Cards,&input.expected)?;
    let mut stale=input.clone();stale.expected.fields.set("instructions",&"stale")?;assert!(workflow::reserve(store,&stale,"stale","time").is_err());
    let mut required=input.clone();required.transition.requires_fields=true;assert!(workflow::reserve(store,&required,"required","time").is_err());
    workflow::reserve(store,&input,"preparing","time")?;workflow::recover(store)?;let card=store.document::<Card>(Documents::Cards,"card")?.unwrap();assert!(card.fields.get::<JiraTransitionIntent>("jiraTransition")?.is_some_and(|intent|intent.state==Phase::Failed));
    let mut repeated=input.clone();repeated.expected=card;repeated.transition.target=repeated.from.clone();workflow::reserve(store,&repeated,"self","time")?;
    let mut options=JiraTransitions{connection_revision:1,current:repeated.from.clone(),transitions:vec![repeated.transition.clone()]};options.current.id="3".into();assert!(workflow::sending(store,"card","self",&options).is_err());options.current=repeated.from.clone();workflow::sending(store,"card","self",&options)?;workflow::recover(store)?;
    assert!(workflow::complete(store,"card","self",Phase::Confirmed,Some(repeated.from.clone())).is_err());workflow::complete(store,"card","self",Phase::Dismissed,Some(repeated.from))?;assert!(!workflow::blocks(store,"/api/cards/card")?);Ok(())
 }).await.unwrap();
    database.close().await.unwrap();
}

#[tokio::test]
async fn malformed_transition_metadata_is_refused_without_any_remote_write() {
    for transitions in [
        json!(null),
        json!([]),
        json!([{"id":"11","name":"Missing fields","to":{"id":"2","name":"Done"}}]),
        json!([{"id":"11","name":"synthetic-token","to":{"id":"2","name":"Done"},"fields":{}}]),
        json!([{"id":"11","name":"Bad metadata","to":{"id":"2","name":"Done"},"fields":{"resolution":{}}}]),
        json!(vec![
            json!({"id":"11","name":"Duplicate","to":{"id":"2","name":"Done"},"fields":{}});
            2
        ]),
    ] {
        let empty = transitions.as_array().is_some_and(|rows| rows.is_empty());
        let remote = Remote {
            status: Arc::new(AtomicUsize::new(1)),
            writes: Arc::new(AtomicUsize::new(0)),
            lost: false,
        };
        let writes = remote.writes.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = Router::new()
            .route("/rest/api/2/issue/1", get(status))
            .route(
                "/rest/api/2/issue/1/transitions",
                get(move || {
                    let value = json!({"transitions":transitions});
                    async move { Json(value) }
                }),
            )
            .with_state(remote);
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = Jira::synthetic(
            JiraConfiguration {
                mode: JiraMode::DataCenter,
                url: format!("http://{address}"),
                email: String::new(),
                query: String::new(),
                revision: 1,
            },
            "synthetic-token".into(),
        )
        .unwrap();
        let result = client.transitions("1").await;
        if empty {
            assert_eq!(result.unwrap().transitions.len(), 0);
        } else {
            assert!(
                !result
                    .err()
                    .unwrap()
                    .to_string()
                    .contains("synthetic-token")
            );
        }
        assert_eq!(writes.load(Ordering::SeqCst), 0);
        server.abort();
        let _ = server.await;
    }
}
