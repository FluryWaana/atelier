use atelier_app::infrastructure::{database::Database, jira::Jira, jira_import, store::Documents};
use atelier_core::{
    contracts::{Card, JiraConfiguration, JiraImport, JiraMode},
    document::Document,
};
use serde_json::json;
fn document(value: serde_json::Value) -> Document {
    serde_json::from_str(&value.to_string()).unwrap()
}
fn input() -> JiraImport {
    JiraImport {
        revision: 1,
        issue_id: "1".into(),
        project_id: "project".into(),
    }
}
#[tokio::test]
async fn a_real_synthetic_read_imports_once_and_preserves_local_edits_on_retry() {
    let temporary = tempfile::tempdir().unwrap();
    let repo = temporary.path().to_string_lossy().into_owned();
    let database = Database::open(&temporary.path().join("synthetic.sqlite")).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let app=axum::Router::new().route("/rest/api/3/issue/1",axum::routing::get(||async {axum::Json(json!({"id":"1","key":"SYN-1","fields":{"summary":"Synthetic issue","status":{"name":"Open"},"description":{"type":"doc","version":1,"content":[{"type":"paragraph","content":[{"type":"text","text":"Description é😀"}]}]}}}))}));
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let connection = JiraConfiguration {
        mode: JiraMode::Cloud,
        url: format!("http://{address}"),
        email: "synthetic@example.test".into(),
        query: "project = SYN".into(),
        revision: 1,
    };
    let initial = connection.clone();
    database
        .call(move |store| {
            store.save_settings(&document(json!({"jiraConnection":initial})))?;
            store.put(
                Documents::Projects,
                &document(json!({"id":"project","name":"Synthetic","repo":repo})),
            )?;
            jira_import::reserve(store, input(), "first".into())?;
            assert!(jira_import::reserve(store, input(), "collision".into()).is_err());
            let public = atelier_core::application::settings::public_config(
                &store.settings::<Document>()?,
                &Document::default(),
                "synthetic",
            )?;
            assert!(public.raw("jiraImportReservation").is_none());
            Ok(())
        })
        .await
        .unwrap();
    let client = Jira::synthetic(connection, "synthetic-token".into()).unwrap();
    let detail = client.detail("1").await.unwrap();
    let invalid = detail.clone();
    database
        .call(move |store| {
            let mut invalid = invalid;
            invalid.issue.title = "x".repeat(301);
            assert!(jira_import::finish(store, "first", invalid).is_err());
            assert!(store.documents::<Card>(Documents::Cards)?.is_empty());
            assert!(
                store
                    .settings::<Document>()?
                    .raw("jiraImportReservation")
                    .is_some()
            );
            Ok(())
        })
        .await
        .unwrap();
    let first = database
        .call(move |store| jira_import::finish(store, "first", detail))
        .await
        .unwrap();
    assert!(first.created);
    let id = first.card.id.clone();
    database
        .call(move |store| {
            let mut card = store.document::<Card>(Documents::Cards, &id)?.unwrap();
            card.fields.set("description", &"Local changes")?;
            store.put(Documents::Cards, &card)?;
            jira_import::reserve(store, input(), "retry".into())
        })
        .await
        .unwrap();
    let detail = client.detail("1").await.unwrap();
    let retried = database
        .call(move |store| jira_import::finish(store, "retry", detail))
        .await
        .unwrap();
    assert!(!retried.created);
    assert_eq!(retried.card.id, first.card.id);
    assert_eq!(
        retried
            .card
            .fields
            .get::<String>("description")
            .unwrap()
            .as_deref(),
        Some("Local changes")
    );
    database
        .call(|store| {
            assert_eq!(store.documents::<Card>(Documents::Cards)?.len(), 1);
            jira_import::reserve(store, input(), "stale".into())?;
            let mut settings = store.settings::<Document>()?;
            let mut config = settings
                .get::<JiraConfiguration>("jiraConnection")?
                .unwrap();
            config.revision += 1;
            settings.set("jiraConnection", &config)?;
            store.save_settings(&settings)
        })
        .await
        .unwrap();
    let detail = client.detail("1").await.unwrap();
    database
        .call(move |store| {
            assert!(jira_import::finish(store, "stale", detail).is_err());
            jira_import::release(store, "wrong-id")?;
            assert!(
                store
                    .settings::<Document>()?
                    .raw("jiraImportReservation")
                    .is_some()
            );
            jira_import::release(store, "stale")?;
            Ok(())
        })
        .await
        .unwrap();
    server.abort();
    let _ = server.await;
    database.close().await.unwrap();
}

#[tokio::test]
async fn restart_discards_read_only_reservations_without_creating_a_partial_card() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("synthetic.sqlite");
    let database = Database::open(&path).unwrap();
    database.call(|store| {
        store.save_settings(&document(json!({"jiraConnection":{"mode":"cloud","url":"https://synthetic.example.test","email":"synthetic@example.test","query":"project = SYN","revision":1}})))?;
        store.put(Documents::Projects,&document(json!({"id":"project","name":"Synthetic","repo":"synthetic"})))?;
        jira_import::reserve(store,input(),"interrupted".into())
    }).await.unwrap();
    database.close().await.unwrap();
    drop(database);
    let database = Database::open(&path).unwrap();
    database
        .call(|store| {
            jira_import::recover(store)?;
            jira_import::recover(store)?;
            assert!(store.documents::<Card>(Documents::Cards)?.is_empty());
            assert!(
                store
                    .settings::<Document>()?
                    .raw("jiraImportReservation")
                    .is_none()
            );
            Ok(())
        })
        .await
        .unwrap();
    database.close().await.unwrap();
}
