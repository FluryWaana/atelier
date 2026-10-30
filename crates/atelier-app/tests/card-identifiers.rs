use atelier_app::infrastructure::{database::Database, store::Documents, time_tracking::Clock};
use atelier_core::{
    application::{task_templates, time_tracking},
    document::Document,
};
use serde_json::json;

#[tokio::test]
async fn time_and_templates_reject_nontext_ids_without_writing_and_keep_text_ids() {
    let database = Database::open(std::path::Path::new(":memory:")).unwrap();
    database
        .call(|store| {
            store.put(Documents::Cards, &json!({"id":"1", "title":"Synthetic"}))?;
            let settings = store.settings::<Document>()?;
            for value in [
                None,
                Some(json!(null)),
                Some(json!(true)),
                Some(json!(false)),
                Some(json!(1)),
                Some(json!(1.5)),
                Some(json!([])),
                Some(json!(["1"])),
                Some(json!({})),
                Some(json!({"id":"1"})),
                Some(json!("")),
                Some(json!("  ")),
            ] {
                let mut body: Document = serde_json::from_value(json!({
                    "name":"Synthetic", "seconds":60, "date":"2026-10-30",
                    "startTime":"10:00:00", "description":"Synthetic work"
                }))?;
                if let Some(value) = value {
                    body.set("cardId", &value)?;
                }
                assert_eq!(
                    task_templates::create(store, &body, || panic!(
                        "invalid ID must not create a template"
                    ))
                    .unwrap_err()
                    .to_string(),
                    "message.invalid_card_id"
                );
                assert_eq!(
                    time_tracking::execute(
                        store,
                        &Clock,
                        "create",
                        "",
                        &body,
                        || panic!("invalid ID must not create time"),
                        |_| false
                    )
                    .unwrap_err()
                    .to_string(),
                    "message.invalid_card_id"
                );
                assert!(store.documents::<Document>(Documents::Times)?.is_empty());
                assert_eq!(
                    serde_json::to_value(store.settings::<Document>()?)?,
                    serde_json::to_value(&settings)?
                );
            }
            let mut body: Document = serde_json::from_value(json!({
                "cardId":"1", "name":"Synthetic", "seconds":60, "date":"2026-10-30",
                "startTime":"10:00:00", "description":"Synthetic work"
            }))?;
            let entry = time_tracking::execute(
                store,
                &Clock,
                "create",
                "",
                &body,
                || "time".into(),
                |_| false,
            )?;
            assert_eq!(entry.get::<String>("cardId")?.as_deref(), Some("1"));
            task_templates::create(store, &body, || "template".into())?;
            body.set("cardId", &"missing")?;
            assert_eq!(
                time_tracking::execute(store, &Clock, "create", "", &body, String::new, |_| false)
                    .unwrap_err()
                    .to_string(),
                "message.card_not_found"
            );
            assert!(task_templates::create(store, &body, String::new).is_err());
            Ok(())
        })
        .await
        .unwrap();
    database.close().await.unwrap();
}
