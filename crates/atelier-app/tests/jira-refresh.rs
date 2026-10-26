use atelier_app::infrastructure::{
    database::Database, jira_refresh as workflow, jira_transition, store::Documents,
};
use atelier_core::{
    contracts::{
        Card, JiraIssue, JiraIssueDetail, JiraRefreshApply, JiraRefreshPreview, JiraRefreshRequest,
        JiraRefreshState,
    },
    document::Document,
};
use serde_json::json;
fn document(value: serde_json::Value) -> Document {
    serde_json::from_str(&value.to_string()).unwrap()
}
fn detail() -> JiraIssueDetail {
    JiraIssueDetail {
        connection_revision: 1,
        description: "Remote description é😀".into(),
        issue: JiraIssue {
            id: "1".into(),
            key: "SYN-1".into(),
            title: "Remote title".into(),
            status: "Open".into(),
            status_id: "1".into(),
            url: "https://synthetic.example.test/browse/SYN-1".into(),
        },
    }
}
fn seed(store: &atelier_app::infrastructure::store::Store<'_>) -> atelier_app::Result<Card> {
    store.save_settings(&document(json!({"jiraConnection":{"mode":"cloud","url":"https://synthetic.example.test","email":"synthetic@example.test","query":"project = SYN","revision":1}})))?;
    let card:Card=serde_json::from_str(&json!({"id":"card","title":"Local title","description":"Local description","instructions":"Keep instructions","status":"review","checklist":[{"text":"Keep criterion","done":true}],"automation":{"state":"ready"},"ticket":{"connector":"jira","instance":"https://synthetic.example.test","id":"1","key":"SYN-1","status":"Open"}}).to_string())?;
    store.put(Documents::Cards, &card)?;
    store.put(
        Documents::Runs,
        &document(json!({"id":"review","cardId":"card","kind":"pr-review","status":"completed"})),
    )?;
    store.put(
        Documents::Runs,
        &document(json!({"id":"review2","cardId":"card","kind":"pr-review","status":"completed"})),
    )?;
    Ok(card)
}
fn ready(
    store: &atelier_app::infrastructure::store::Store<'_>,
    expected: Card,
    id: &str,
) -> atelier_app::Result<Card> {
    workflow::reserve(
        store,
        &JiraRefreshRequest {
            revision: 1,
            expected,
        },
        id,
    )?;
    workflow::ready(store, "card", id, detail())
}

#[tokio::test]
async fn mapped_columns_require_explicit_approval_and_the_same_mapping_snapshot() {
    let database = Database::open(std::path::Path::new(":memory:")).unwrap();
    database
        .call(|store| {
            let initial = seed(store)?;
            let mut settings = store.settings::<Document>()?;
            settings.set("jiraAutoTransition", &true)?;
            settings.set("jiraStatusMap", &json!({"doing":"1"}))?;
            store.save_settings(&settings)?;
            let card = ready(store, initial, "mapped")?;
            let preview = card
                .fields
                .get::<JiraRefreshPreview>("jiraRefresh")?
                .unwrap();
            assert_eq!(preview.mapped_status.as_deref(), Some("doing"));
            settings.set("jiraStatusMap", &json!({"todo":"1"}))?;
            store.save_settings(&settings)?;
            let request = JiraRefreshApply {
                revision: 1,
                expected: card.clone(),
                preview_id: "mapped".into(),
                apply_status: true,
            };
            assert!(workflow::apply(store, &request).is_err());
            assert!(
                store
                    .document::<Card>(Documents::Cards, "card")?
                    .unwrap()
                    .fields
                    .string_equals("status", "review")?
            );
            settings.set("jiraStatusMap", &json!({"doing":"1"}))?;
            store.save_settings(&settings)?;
            let applied = workflow::apply(store, &request)?;
            assert!(applied.fields.string_equals("status", "doing")?);
            assert!(
                applied
                    .fields
                    .string_equals("instructions", "Keep instructions")?
            );
            Ok(())
        })
        .await
        .unwrap();
}
#[tokio::test]
async fn previews_are_bound_to_the_saved_card_expire_and_preserve_local_criteria_and_instructions()
{
    let database = Database::open(std::path::Path::new(":memory:")).unwrap();
    database
        .call(|store| {
            let initial = seed(store)?;
            workflow::reserve(
                store,
                &JiraRefreshRequest {
                    revision: 1,
                    expected: initial,
                },
                "interrupted",
            )?;
            assert!(jira_transition::blocks(store, "/api/cards/card")?);
            workflow::recover(store)?;
            let current = store.document::<Card>(Documents::Cards, "card")?.unwrap();
            assert!(
                current
                    .fields
                    .get::<JiraRefreshPreview>("jiraRefresh")?
                    .is_some_and(|preview| preview.state == JiraRefreshState::Interrupted)
            );
            let preview = ready(store, current, "preview")?;
            let saved = preview
                .fields
                .get::<JiraRefreshPreview>("jiraRefresh")?
                .unwrap();
            assert!(saved.expected.fields.raw("jiraRefresh").is_none());
            assert!(
                workflow::apply(
                    store,
                    &JiraRefreshApply {
                        revision: 1,
                        expected: preview.clone(),
                        preview_id: "wrong".into(),
                        apply_status: false,
                    }
                )
                .is_err()
            );
            let mut changed = preview.clone();
            changed
                .fields
                .set("instructions", &"New local instructions")?;
            store.put(Documents::Cards, &changed)?;
            assert!(
                workflow::apply(
                    store,
                    &JiraRefreshApply {
                        revision: 1,
                        expected: changed.clone(),
                        preview_id: "preview".into(),
                        apply_status: false,
                    }
                )
                .is_err()
            );
            assert!(
                store
                    .document::<Card>(Documents::Cards, "card")?
                    .unwrap()
                    .fields
                    .string_equals("instructions", "New local instructions")?
            );
            store.put(Documents::Cards, &preview)?;
            let mut expired = preview.clone();
            let mut old = saved;
            old.created_at = "1970-01-01T00:00:00Z".into();
            expired.fields.set("jiraRefresh", &old)?;
            store.put(Documents::Cards, &expired)?;
            assert!(
                workflow::apply(
                    store,
                    &JiraRefreshApply {
                        revision: 1,
                        expected: expired,
                        preview_id: "preview".into(),
                        apply_status: false,
                    }
                )
                .is_err()
            );
            store.put(Documents::Cards, &preview)?;
            let applied = workflow::apply(
                store,
                &JiraRefreshApply {
                    revision: 1,
                    expected: preview,
                    preview_id: "preview".into(),
                    apply_status: false,
                },
            )?;
            assert_eq!(applied.title.utf8().unwrap(), "Remote title");
            assert!(
                applied
                    .fields
                    .string_equals("description", "Remote description é😀")?
            );
            assert!(
                applied
                    .fields
                    .string_equals("instructions", "Keep instructions")?
            );
            assert!(applied.fields.string_equals("status", "review")?);
            assert!(applied.fields.raw("jiraRefresh").is_none());
            let checklist = applied
                .fields
                .get::<serde_json::Value>("checklist")?
                .unwrap();
            assert_eq!(checklist, json!([{"text":"Keep criterion","done":true}]));
            assert!(
                applied
                    .fields
                    .get::<Document>("automation")?
                    .unwrap()
                    .string_equals("state", "interrupted")?
            );
            assert!(
                store
                    .document::<Document>(Documents::Runs, "review")?
                    .unwrap()
                    .raw("reviewStaleReason")
                    .is_some()
            );
            Ok(())
        })
        .await
        .unwrap();
    database.close().await.unwrap();
}
#[tokio::test]
async fn failing_review_invalidation_rolls_back_the_card_preview_and_review_together() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("synthetic.sqlite");
    let database = Database::open(&path).unwrap();
    database
        .call(|store| {
            let card = seed(store)?;
            ready(store, card, "rollback")?;
            assert!(
                store
                    .document::<Document>(Documents::Runs, "review2")?
                    .unwrap()
                    .raw("reviewStaleReason")
                    .is_none()
            );
            Ok(())
        })
        .await
        .unwrap();
    database.close().await.unwrap();
    drop(database);
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection.execute_batch("CREATE TRIGGER synthetic_review_failure BEFORE UPDATE ON runs WHEN NEW.id='review' BEGIN SELECT RAISE(ABORT,'synthetic failure'); END;").unwrap();
    drop(connection);
    let database = Database::open(&path).unwrap();
    database
        .call(|store| {
            let expected = store.document::<Card>(Documents::Cards, "card")?.unwrap();
            let before = serde_json::to_value(&expected)?;
            assert!(
                workflow::apply(
                    store,
                    &JiraRefreshApply {
                        revision: 1,
                        expected,
                        preview_id: "rollback".into(),
                        apply_status: false,
                    }
                )
                .is_err()
            );
            assert_eq!(
                serde_json::to_value(store.document::<Card>(Documents::Cards, "card")?.unwrap())?,
                before
            );
            assert!(
                store
                    .document::<Document>(Documents::Runs, "review")?
                    .unwrap()
                    .raw("reviewStaleReason")
                    .is_none()
            );
            Ok(())
        })
        .await
        .unwrap();
    database.close().await.unwrap();
}

#[tokio::test]
async fn full_length_card_text_does_not_resend_the_nested_read_cache() {
    let database = Database::open(std::path::Path::new(":memory:")).unwrap();
    database
        .call(|store| {
            let mut card = seed(store)?;
            card.fields.set("description", &"d".repeat(20_000))?;
            card.fields.set("instructions", &"i".repeat(20_000))?;
            card.fields.set(
                "checklist",
                &vec![json!({"text":"c".repeat(500),"done":true}); 20],
            )?;
            store.put(Documents::Cards, &card)?;
            workflow::reserve(
                store,
                &JiraRefreshRequest {
                    revision: 1,
                    expected: card,
                },
                "large",
            )?;
            let mut remote = detail();
            remote.description = "r".repeat(20_000);
            let preview = workflow::ready(store, "card", "large", remote)?;
            let snapshot = atelier_core::application::jira::card_snapshot(&preview);
            let body = JiraRefreshApply {
                revision: 1,
                expected: snapshot,
                preview_id: "large".into(),
                apply_status: false,
            };
            assert!(serde_json::to_string(&preview)?.encode_utf16().count() > 100_000);
            assert!(serde_json::to_string(&body)?.encode_utf16().count() < 100_000);
            let applied = workflow::apply(store, &body)?;
            assert_eq!(
                applied.fields.get::<String>("description")?.unwrap().len(),
                20_000
            );
            assert_eq!(
                applied.fields.get::<String>("instructions")?.unwrap().len(),
                20_000
            );
            Ok(())
        })
        .await
        .unwrap();
    database.close().await.unwrap();
}
