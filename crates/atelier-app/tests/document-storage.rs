use atelier_app::infrastructure::{backups, database::Database, store::Documents};
use atelier_core::document::Document;

#[tokio::test]
async fn sqlite_and_restore_preserve_raw_keys_and_strings_while_changing_known_fields() {
    let directory = tempfile::tempdir().unwrap();
    let database = Database::open(&directory.path().join("test.sqlite")).unwrap();
    let backup = directory.path().join("backup.sqlite");
    let copy = backup.clone();
    database.call(move |store| {
        let document: Document = serde_json::from_str(r#"{"id":"card","title":"\u00e9","status":null,"automation":{"state":"ready","\u00e9":"\u00e8"},"future":["\u00e9",{"\u00e8":null}]}"#)?;
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
            assert!(raw.contains(r#""\u00e9":"\u00e8""#));
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
