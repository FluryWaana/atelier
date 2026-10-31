use atelier_app::infrastructure::{backups, database::Database, store::Documents};
use atelier_core::document::Document;

#[tokio::test]
async fn invalid_known_fields_in_a_backup_never_replace_the_current_data() {
    let directory = tempfile::tempdir().unwrap();
    let database = Database::open(&directory.path().join("test.sqlite")).unwrap();
    let backup = directory.path().join("invalid.sqlite");
    let rescue = directory.path().join("rescue.sqlite");
    database.call(move |store| {
        store.put(Documents::Cards, &serde_json::json!({"id":"card","title":"Invalid backup","status":null,"automation":{"state":"ready"}}))?;
        store.backup(&backup)?;
        let current = serde_json::json!({"id":"card","title":"Current local work","status":"todo"});
        store.put(Documents::Cards, &current)?;
        assert!(backups::restore(store, &backup, &rescue).is_err());
        assert_eq!(store.document::<serde_json::Value>(Documents::Cards, "card")?, Some(current));
        Ok(())
    }).await.unwrap();
    database.close().await.unwrap();
}

#[tokio::test]
async fn sqlite_and_restore_preserve_raw_keys_and_strings_while_changing_known_fields() {
    let directory = tempfile::tempdir().unwrap();
    let database = Database::open(&directory.path().join("test.sqlite")).unwrap();
    let backup = directory.path().join("backup.sqlite");
    let copy = backup.clone();
    database.call(move |store| {
        let document: Document = serde_json::from_str(r#"{"id":"card","title":"\u00e9","status":"review","automation":{"state":"ready","\u00e9":"\u00e8"},"future":["\u00e9",{"\u00e8":null}]}"#)?;
        store.put(Documents::Cards, &document)?;
        store.save_settings(&document)?;
        store.backup(&copy)?;
        let mut changed = document;
        changed.set("title", &"changed")?;
        store.put(Documents::Cards, &changed)
    }).await.unwrap();
    let rescue = directory.path().join("rescue.sqlite");
    database
        .call(move |store| backups::restore(store, &backup, &rescue))
        .await
        .unwrap();
    database
        .call(|store| {
            let card = store
                .document::<Document>(Documents::Cards, "card")?
                .unwrap();
            assert_eq!(card.get::<String>("title")?, Some("é".to_owned()));
            assert!(card.string_equals("status", "standby")?);
            let raw = serde_json::to_string(&card)?;
            assert!(raw.contains(r#""é":"\u00e8""#));
            assert!(raw.contains(r#""future":["\u00e9",{"\u00e8":null}]"#));
            let settings = store.settings::<Document>()?;
            assert_eq!(settings.get::<bool>("credentialsReset")?, Some(true));
            assert_eq!(settings.get::<String>("title")?, Some("é".to_owned()));
            Ok(())
        })
        .await
        .unwrap();
    database.close().await.unwrap();
}
