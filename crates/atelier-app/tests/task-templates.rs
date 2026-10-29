use atelier_app::infrastructure::{database::Database, store::Documents};
use atelier_core::{
    application::task_templates,
    document::{Document, JsonString},
};

#[tokio::test]
async fn templates_keep_settings_and_utf16_copy_only_editable_content_and_enforce_limits() {
    let database = Database::open(std::path::Path::new(":memory:")).unwrap();
    database.call(|store| {
        let card: Document = serde_json::from_str(r#"{"id":"card","title":"title","description":"\ud800","instructions":"keep","mode":"analyze","checklist":[{"text":"\udc00","done":true,"extra":1}],"ticket":{"key":"TEST-1"},"repo":"ignored"}"#)?;
        let settings: Document = serde_json::from_str(r#"{"future":{"\ud800":"\udc00"},"taskQueue":{"paused":true,"ids":["card"]}}"#)?;
        store.put(Documents::Cards, &card)?; store.save_settings(&settings)?;
        let body: Document = serde_json::from_str(r#"{"name":" \ud800 ","cardId":"card"}"#)?;
        let template = store.transaction(|store| task_templates::create(store, &body, || "first".into()))?;
        assert_eq!(template.get::<JsonString>("name")?, Some(JsonString(vec![0xd800])));
        assert_eq!(template.get::<JsonString>("description")?, Some(JsonString(vec![0xd800])));
        let criteria = template.get::<Vec<Document>>("checklist")?.unwrap();
        assert_eq!(criteria[0].get::<JsonString>("text")?, Some(JsonString(vec![0xdc00])));
        assert_eq!(criteria[0].get::<bool>("done")?, Some(false));
        assert!(criteria[0].raw("extra").is_none());
        assert!(template.raw("repo").is_none()); assert!(template.raw("ticket").is_none());
        for index in 1..50 {
            store.transaction(|store| task_templates::create(store, &body, || index.to_string()))?;
        }
        assert_eq!(task_templates::create(store, &body, || "overflow".into()).unwrap_err().to_string(), "message.remove_a_template_before_adding_another_50_maximum");
        store.transaction(|store| task_templates::remove(store, "missing"))?;
        let loaded = store.settings::<Document>()?;
        assert_eq!(loaded.get::<Vec<Document>>("taskTemplates")?.unwrap().len(), 50);
        for key in ["future", "taskQueue"] { assert_eq!(loaded.raw(key).unwrap().get(), settings.raw(key).unwrap().get()); }
        store.transaction(|store| task_templates::remove(store, "first"))?;
        let mut boundary = body.clone(); boundary.set("name", &"🔧".repeat(50))?;
        store.transaction(|store| task_templates::create(store, &boundary, || "boundary".into()))?;
        boundary.set("name", &"🔧".repeat(51))?;
        assert_eq!(task_templates::create(store, &boundary, || "overflow".into()).unwrap_err().to_string(), "message.invalid_or_overly_long_text");
        boundary.set("name", &" ")?;
        assert_eq!(task_templates::create(store, &boundary, || "empty".into()).unwrap_err().to_string(), "message.template_name_required");
        assert_eq!(store.document::<Document>(Documents::Cards, "card")?.unwrap().raw("checklist").unwrap().get(), card.raw("checklist").unwrap().get());
        Ok(())
    }).await.unwrap();
    database.close().await.unwrap();
}
