use atelier_app::infrastructure::{database::Database, jira, settings, store::Documents};
use atelier_core::{
    contracts::{JiraConfigurationUpdate, JiraMode},
    document::Document,
};

fn input(revision: u64, token: Option<&str>) -> JiraConfigurationUpdate {
    JiraConfigurationUpdate {
        expected_revision: revision,
        mode: JiraMode::Cloud,
        url: "https://synthetic.example.test".into(),
        email: "synthetic@example.test".into(),
        query: "project = SYN".into(),
        token: token.map(str::to_owned),
        clear_token: false,
    }
}

#[tokio::test]
async fn jira_settings_and_vault_commit_together_without_returning_the_secret() {
    let temporary = tempfile::tempdir().unwrap();
    let backup_path = temporary.path().to_owned();
    let database = Database::open(std::path::Path::new(":memory:")).unwrap();
    database
        .call(move |store| {
            let mut protector = |value: &str, decrypt: bool| -> atelier_app::Result<String> {
                if decrypt {
                    Ok(value.strip_prefix("opaque:").ok_or("bad fixture")?.into())
                } else {
                    Ok(format!("opaque:{value}"))
                }
            };
            let saved = jira::configure(store, input(0, Some("synthetic-token")), &mut protector)?;
            assert!(saved.token_set);
            assert_eq!(saved.configuration.revision, 1);
            assert!(!serde_json::to_string(&saved)?.contains("synthetic-token"));
            let (connection, token) = jira::credentials(store, 1, &mut protector)?;
            assert_eq!(token, "synthetic-token");
            assert_eq!(connection.url, "https://synthetic.example.test");
            let before = serde_json::to_string(&store.settings::<Document>()?)?;
            assert!(
                jira::configure(store, input(0, Some("synthetic-stale")), &mut protector).is_err()
            );
            let mut fail_encrypt = |value: &str, decrypt: bool| -> atelier_app::Result<String> {
                if decrypt {
                    Ok(value.strip_prefix("opaque:").unwrap().into())
                } else {
                    Err(settings::VaultFailure.into())
                }
            };
            assert!(
                jira::configure(store, input(1, Some("synthetic-failed")), &mut fail_encrypt)
                    .is_err()
            );
            assert_eq!(
                serde_json::to_string(&store.settings::<Document>()?)?,
                before
            );
            assert_eq!(
                jira::credentials(store, 1, &mut protector)?.1,
                "synthetic-token"
            );
            let backups =
                atelier_app::infrastructure::backups::BackupDirectory::open(&backup_path)?;
            let saved_backup = backups.create(store)?;
            let rotation: Document =
                serde_json::from_str(r#"{"token":"synthetic-legacy-rotation"}"#)?;
            settings::update(store, &rotation, &mut protector)?;
            assert_eq!(
                jira::describe(store, &mut protector)?
                    .configuration
                    .revision,
                2
            );
            assert_eq!(
                jira::credentials(store, 2, &mut protector)?.1,
                "synthetic-legacy-rotation"
            );
            settings::remove_token(store, "jira", &mut protector)?;
            assert_eq!(
                jira::describe(store, &mut protector)?
                    .configuration
                    .revision,
                3
            );
            backups.restore(store, &saved_backup.name)?;
            assert_eq!(
                jira::describe(store, &mut protector)?
                    .configuration
                    .revision,
                4
            );
            assert!(!jira::describe(store, &mut protector)?.token_set);
            let mut moved = input(4, None);
            moved.url = "https://another.example.test".into();
            let saved = jira::configure(store, moved, &mut protector)?;
            assert!(!saved.token_set);
            assert!(jira::credentials(store, 1, &mut protector).is_err());
            assert!(store.documents::<Document>(Documents::Cards)?.is_empty());
            Ok(())
        })
        .await
        .unwrap();
    database.close().await.unwrap();
}
