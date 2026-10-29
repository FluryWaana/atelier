use atelier_app::infrastructure::{
    backups::BackupDirectory, database::Database, jira::Jira, jira_creation as workflow,
    jira_transition, store::Documents,
};
use atelier_core::{
    application::jira as domain,
    contracts::{
        Card, JiraConfiguration, JiraCreateRequest, JiraCreationIntent, JiraMode,
        JiraWriteState as Phase,
    },
    document::Document,
};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    routing::{get, post},
};
use serde_json::{Value, json};
fn assert_backup_blocked(
    store: &atelier_app::infrastructure::store::Store<'_>,
    path: &std::path::Path,
) -> atelier_app::Result<()> {
    let directory = BackupDirectory::open(path)?;
    let before = serde_json::to_value(directory.list()?)?;
    let cards = store.documents::<Value>(Documents::Cards)?;
    let error = directory
        .create(store)
        .err()
        .expect("Jira creation must block backup creation");
    assert_eq!(
        error.to_string(),
        "message.complete_or_reconcile_jira_operations_before_creating_a_backup"
    );
    assert_eq!(serde_json::to_value(directory.list()?)?, before);
    assert_eq!(store.documents::<Value>(Documents::Cards)?, cards);
    Ok(())
}
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[derive(Clone)]
struct Remote {
    cloud: bool,
    lost: bool,
    writes: Arc<AtomicUsize>,
}
fn auth(headers: &HeaderMap, cloud: bool) {
    assert_eq!(
        headers[header::AUTHORIZATION],
        if cloud {
            "Basic c3ludGhldGljQGV4YW1wbGUudGVzdDpzeW50aGV0aWMtdG9rZW4="
        } else {
            "Bearer synthetic-token"
        }
    );
}
async fn types(
    State(remote): State<Remote>,
    headers: HeaderMap,
    Query(query): Query<std::collections::BTreeMap<String, String>>,
) -> Json<Value> {
    auth(&headers, remote.cloud);
    assert_eq!(query["maxResults"], "100");
    let offset = query["startAt"].parse::<usize>().unwrap();
    assert!(offset < 2);
    let mut page = json!({"total":2,"startAt":offset,"maxResults":1,"isLast":offset==1});
    page[if remote.cloud { "issueTypes" } else { "values" }] = json!([{"id":(offset+1).to_string(),"name":if offset==0{"Task"}else{"Subtask"},"subtask":offset==1}]);
    Json(page)
}
async fn fields(
    State(remote): State<Remote>,
    headers: HeaderMap,
    Query(query): Query<std::collections::BTreeMap<String, String>>,
) -> Json<Value> {
    auth(&headers, remote.cloud);
    let offset = query["startAt"].parse::<usize>().unwrap();
    assert!(offset < 2);
    let mut page = json!({"total":2,"startAt":offset,"isLast":offset==1});
    page[if remote.cloud { "fields" } else { "values" }] = json!([{"fieldId":if offset==0{"summary"}else{"description"},"name":"Field","required":offset==0,"operations":["set"],"hasDefaultValue":false,"allowedValues":[{"secret":"synthetic-token"}],"defaultValue":"synthetic-token"}]);
    Json(page)
}
async fn create(
    State(remote): State<Remote>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    auth(&headers, remote.cloud);
    assert_eq!(body["fields"]["project"], json!({"key":"SYN"}));
    assert_eq!(body["fields"]["issuetype"], json!({"id":"1"}));
    assert_eq!(body["fields"]["summary"], "Synthetic 🦀");
    let text = domain::creation_text(&request("unused").expected).unwrap();
    if remote.cloud {
        assert_eq!(body["fields"]["description"]["type"], "doc");
        assert_eq!(
            body["fields"]["description"],
            json!({"type":"doc","version":1,"content":text.split('\n').map(|line|json!({"type":"paragraph","content":if line.is_empty(){vec![]}else{vec![json!({"type":"text","text":line})]}})).collect::<Vec<_>>() })
        );
    } else {
        assert_eq!(body["fields"]["description"], text);
    }
    assert_eq!(body["fields"].as_object().unwrap().len(), 4);
    remote.writes.fetch_add(1, Ordering::SeqCst);
    (
        if remote.lost {
            StatusCode::INTERNAL_SERVER_ERROR
        } else {
            StatusCode::CREATED
        },
        Json(json!({"id":"100","key":"SYN-1","self":"https://untrusted.example.test/ignored"})),
    )
}
async fn read_issue(
    State(remote): State<Remote>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Query(query): Query<std::collections::BTreeMap<String, String>>,
) -> Json<Value> {
    auth(&headers, remote.cloud);
    assert!(id == "SYN-1" || id == "100");
    assert_eq!(query["fields"], "summary,status");
    Json(
        json!({"id":"100","key":"SYN-1","fields":{"summary":"Synthetic 🦀","status":{"name":"Open"}}}),
    )
}
fn document(value: Value) -> Document {
    serde_json::from_str(&value.to_string()).unwrap()
}
fn request(_base: &str) -> JiraCreateRequest {
    serde_json::from_str(&json!({"revision":1,"expected":{"id":"card","title":"Synthetic 🦀","status":"todo","description":"Body","instructions":"Synthetic instruction","checklist":[{"text":"Human check","done":false}]},"projectKey":"SYN","issueType":{"id":"1","name":"Task","subtask":false},"confirmed":true}).to_string()).unwrap()
}

#[tokio::test]
async fn malformed_creation_pages_never_return_partial_or_confidential_metadata() {
    for page in [
        json!({"startAt":0,"total":2,"values":[]}),
        json!({"startAt":1,"total":1,"values":[{"id":"1","name":"Task","subtask":false}]}),
        json!({"startAt":0,"total":1,"isLast":false,"values":[{"id":"1","name":"Task","subtask":false}]}),
        json!({"startAt":0,"total":2,"values":[{"id":"1","name":"Task","subtask":false},{"id":"1","name":"Duplicate","subtask":false}]}),
        json!({"startAt":0,"total":1,"values":[{"id":"1","name":"synthetic-token","subtask":false}]}),
        json!({"startAt":0,"total":1,"values":[{"id":"1","name":"Task"}]}),
        json!({"startAt":0,"total":1001,"values":[]}),
    ] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let app = Router::new().route(
            "/rest/api/2/issue/createmeta/SYN/issuetypes",
            get(move || {
                let page = page.clone();
                async move { Json(page) }
            }),
        );
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = Jira::synthetic(
            JiraConfiguration {
                mode: JiraMode::DataCenter,
                url: base,
                email: String::new(),
                query: String::new(),
                revision: 1,
            },
            "synthetic-token".into(),
        )
        .unwrap();
        assert!(client.issue_types("SYN").await.is_err());
        server.abort();
        let _ = server.await;
    }
}

#[tokio::test]
async fn synthetic_creation_sends_once_and_recovers_lost_response_without_resubmission() {
    for cloud in [false, true] {
        for lost in [false, true] {
            let temporary = tempfile::tempdir().unwrap();
            let path = temporary.path().join("synthetic.sqlite");
            let remote = Remote {
                cloud,
                lost,
                writes: Arc::new(AtomicUsize::new(0)),
            };
            let writes = remote.writes.clone();
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let base = format!("http://{}", listener.local_addr().unwrap());
            let version = if cloud { "3" } else { "2" };
            let app = Router::new()
                .route(
                    &format!("/rest/api/{version}/issue/createmeta/SYN/issuetypes"),
                    get(types),
                )
                .route(
                    &format!("/rest/api/{version}/issue/createmeta/SYN/issuetypes/1"),
                    get(fields),
                )
                .route(&format!("/rest/api/{version}/issue"), post(create))
                .route(
                    &format!("/rest/api/{version}/issue/{{id}}"),
                    get(read_issue),
                )
                .with_state(remote);
            let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
            let configuration = JiraConfiguration {
                mode: if cloud {
                    JiraMode::Cloud
                } else {
                    JiraMode::DataCenter
                },
                url: base.clone(),
                email: "synthetic@example.test".into(),
                query: "project = SYN".into(),
                revision: 1,
            };
            let initial = configuration.clone();
            let input = request(&base);
            let seeded = input.clone();
            let backups = temporary.path().to_owned();
            let database = Database::open(&path).unwrap();
            let backup = database
                .call(move |store| {
                    store.save_settings(&document(json!({"jiraConnection":initial})))?;
                    store.put(Documents::Cards, &seeded.expected)?;
                    let backup = BackupDirectory::open(&backups)?.create(store)?;
                    workflow::reserve(store, &seeded, "operation")?;
                    assert!(workflow::reserve(store, &seeded, "repeat").is_err());
                    assert!(jira_transition::blocks(store, "/api/cards/card")?);
                    assert!(!jira_transition::blocks(
                        store,
                        "/api/cards/card/jira/creation-reconcile"
                    )?);
                    assert_backup_blocked(store, &backups)?;
                    Ok(backup.name)
                })
                .await
                .unwrap();
            let client = Jira::synthetic(configuration, "synthetic-token".into()).unwrap();
            let kinds = client.issue_types("SYN").await.unwrap();
            assert_eq!(kinds.len(), 2);
            assert!(kinds[1].subtask);
            let metadata = client.creation_metadata("SYN", "1").await.unwrap();
            assert_eq!(metadata.fields.len(), 2);
            assert!(
                !serde_json::to_string(&metadata)
                    .unwrap()
                    .contains("synthetic-token")
            );
            assert!(client.creation_metadata("SYN", "2").await.is_err());
            let original = input.expected.clone();
            let backups = temporary.path().to_owned();
            let source = database
                .call(move |store| {
                    let source = workflow::sending(store, &original, "operation")?;
                    assert_backup_blocked(store, &backups)?;
                    Ok(source)
                })
                .await
                .unwrap();
            let result = client.create_issue(&metadata, &source).await;
            assert_eq!(result.is_err(), lost);
            assert_eq!(writes.load(Ordering::SeqCst), 1);
            if let Ok(issue) = result {
                assert!(issue.status.is_empty());
                assert_eq!(issue.url, format!("{base}/browse/SYN-1"));
                database
                    .call(move |store| workflow::record(store, "card", "operation", issue))
                    .await
                    .unwrap();
            }
            database.close().await.unwrap();
            drop(database);
            let database = Database::open(&path).unwrap();
            let backups = temporary.path().to_owned();
            database
                .call(move |store| {
                    workflow::recover(store)?;
                    let card = store.document::<Card>(Documents::Cards, "card")?.unwrap();
                    assert!(
                        card.fields
                            .get::<JiraCreationIntent>("jiraCreation")?
                            .is_some_and(|intent| intent.state
                                == if lost {
                                    Phase::Uncertain
                                } else {
                                    Phase::Confirmed
                                })
                    );
                    if lost {
                        assert_backup_blocked(store, &backups)?;
                    }
                    Ok(())
                })
                .await
                .unwrap();
            if lost {
                let issue = client.issue_by_key("SYN-1").await.unwrap();
                let known = issue.clone();
                // A second card must not consume the already-created identity silently.
                database
                    .call(move |store| {
                        let mut other = request("unused").expected;
                        other.id = "other".into();
                        other.fields.set(
                            "ticket",
                            &json!({"connector":"jira","instance":base,"id":"100","key":"SYN-1"}),
                        )?;
                        store.put(Documents::Cards, &other)?;
                        let card = workflow::record(store, "card", "operation", known)?;
                        assert!(!card.fields.truthy("ticket")?);
                        assert!(
                            card.fields
                                .get::<JiraCreationIntent>("jiraCreation")?
                                .is_some_and(|intent| intent.state == Phase::Uncertain
                                    && intent.result.is_some())
                        );
                        store.delete(Documents::Cards, "other")?;
                        Ok(())
                    })
                    .await
                    .unwrap();
                let observed = client.issue_by_id(&issue.id).await.unwrap();
                database
                    .call(move |store| workflow::record(store, "card", "operation", observed))
                    .await
                    .unwrap();
            }
            let backups = temporary.path().to_owned();
            database
                .call(move |store| {
                    let card = store.document::<Card>(Documents::Cards, "card")?.unwrap();
                    assert_eq!(card.title.utf8().unwrap(), "Synthetic 🦀");
                    assert!(card.fields.string_equals("status", "todo")?);
                    assert!(
                        card.fields
                            .string_equals("instructions", "Synthetic instruction")?
                    );
                    assert!(!domain::unresolved(&card.fields)?);
                    assert!(
                        BackupDirectory::open(&backups)?
                            .restore(store, &backup)
                            .is_err()
                    );
                    assert!(
                        workflow::record(
                            store,
                            "card",
                            "operation",
                            card.fields
                                .get::<JiraCreationIntent>("jiraCreation")?
                                .unwrap()
                                .result
                                .unwrap()
                        )
                        .is_err()
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
}

#[tokio::test]
async fn stale_preparation_recovery_and_explicit_uncertainty_close_preserve_preconditions() {
    let database = Database::open(std::path::Path::new(":memory:")).unwrap();
    database.call(|store|{
        store.save_settings(&document(json!({"jiraConnection":{"mode":"dataCenter","url":"https://synthetic.example.test","email":"","query":"project = SYN","revision":1}})))?;
        let mut input=request("unused");store.put(Documents::Cards,&input.expected)?;
        let mut stale=input.clone();stale.expected.title="Stale".into();assert!(workflow::reserve(store,&stale,"stale").is_err());
        let mut unconfirmed=input.clone();unconfirmed.confirmed=false;assert!(workflow::reserve(store,&unconfirmed,"unconfirmed").is_err());
        workflow::reserve(store,&input,"preparing")?;workflow::recover(store)?;
        let card=store.document::<Card>(Documents::Cards,"card")?.unwrap();assert!(card.fields.get::<JiraCreationIntent>("jiraCreation")?.is_some_and(|intent|intent.state==Phase::Failed));
        input.expected=card;workflow::reserve(store,&input,"sending")?;workflow::sending(store,&input.expected,"sending")?;workflow::recover(store)?;
        assert!(workflow::close_uncertainty(store,&input.expected,1).is_err());
        let card=store.document::<Card>(Documents::Cards,"card")?.unwrap();assert!(domain::unresolved(&card.fields)?);
        assert!(workflow::close_uncertainty(store,&card,2).is_err());
        let card=workflow::close_uncertainty(store,&card,1)?;assert!(!domain::unresolved(&card.fields)?);
        assert!(card.fields.get::<JiraCreationIntent>("jiraCreation")?.is_some_and(|intent|intent.state==Phase::Dismissed));
        assert!(workflow::reserve(store,&input,"replayed").is_err());
        input.expected=card;workflow::reserve(store,&input,"new-explicit-action")?;Ok(())
    }).await.unwrap();
    database.close().await.unwrap();
}
