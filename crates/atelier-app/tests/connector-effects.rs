use atelier_app::infrastructure::{connector_effects as effects, database::Database};
use atelier_core::{
    application::extensions::{self as protocol, Method, RemotePreview, Reply, Status},
    contracts::ForgeWriteState as Phase,
    document::Document,
};
use serde_json::json;

fn doc(value: serde_json::Value) -> Document {
    serde_json::from_str(&value.to_string()).unwrap()
}
fn preview() -> RemotePreview {
    RemotePreview {
        expected_revision: 1,
        action: "write".into(),
    }
}
fn reply(id: &str, nonce: &str, method: Method, outcome: Option<&str>) -> Reply {
    Reply { protocol: protocol::PROTOCOL.into(), request_id: nonce.into(), module: "synthetic-effect".into(), method, status: Status::Ok, message: "Synthetic observation".into(), output: outcome.map(|outcome|json!({"receipt":{"operationId":id,"outcome":outcome,"reference":"synthetic-object-1","observedAt":time::OffsetDateTime::now_utc().format(&time::format_description::well_known::Rfc3339).unwrap()}})).unwrap_or(json!({})) }
}
fn settings() -> Document {
    doc(json!({"connectorRevision":1,"processConnectors":[{
        "manifest":{"id":"synthetic-effect","name":"Synthetic effect","description":"Synthetic only","sourceLocale":"en-US","methods":["check","action","reconcile"],"fields":[],"tabs":[{"id":"actions","label":"Actions","description":"Synthetic only","actions":[{"id":"write","label":"Write","effect":"remote"}]}]},
        "binary":{"path":"/synthetic/never-executed","sha256":"a".repeat(64)},"revision":1,"values":{},"report":null
    }]}))
}

#[tokio::test]
async fn lost_reply_recovery_never_permits_resend_and_requires_a_fresh_scoped_observation() {
    let temporary = tempfile::tempdir().unwrap();
    let backup_directory =
        atelier_app::infrastructure::backups::BackupDirectory::open(temporary.path()).unwrap();
    let db = Database::open(std::path::Path::new(":memory:")).unwrap();
    db.call(move |store| {
        store.save_settings(&settings())?;
        let before = backup_directory.create(store)?;
        let prepared = effects::preview(store, "synthetic-effect", &preview())?;
        assert!(backup_directory.create(store).is_err());
        assert!(backup_directory.restore(store, &before.name).is_err());
        assert!(effects::preview(store, "synthetic-effect", &preview()).is_err());
        for path in [
            "/api/cards/operations",
            "/api/cards/invoke",
            "/api/backups/restore",
            "/api/updates/apply",
        ] {
            assert!(effects::blocks(store, path)?);
        }
        assert!(!effects::blocks(
            store,
            "/api/extensions/synthetic-effect/operations/effect/reconcile"
        )?);
        assert!(effects::require_removal(store, "synthetic-effect").is_err());
        let sent = effects::sending(
            store,
            "synthetic-effect",
            &prepared.id,
            prepared.version,
            1,
            "request-send",
        )?;
        effects::recover(store)?;
        let uncertain = effects::current(store, "synthetic-effect", &sent.id)?;
        assert_eq!(uncertain.state, Phase::Uncertain);
        let (_, request) = effects::prepare(
            store,
            "synthetic-effect",
            &sent.id,
            uncertain.version,
            1,
            true,
            &mut |value, _| Ok(value.into()),
        )?;
        assert_eq!(request.method, Method::Reconcile);
        assert_eq!(request.operation_id.as_deref(), Some(sent.id.as_str()));
        assert!(request.action.is_none());
        assert_eq!(request.input["action"], "write");
        let mut profile =
            atelier_app::infrastructure::connectors::profile(store, "synthetic-effect", 1)?;
        profile.binary.sha256 = "b".repeat(64);
        assert!(effects::require_configuration(store, &profile).is_err());
        profile.binary.sha256 = "a".repeat(64);
        profile.revision = 2;
        effects::require_configuration(store, &profile)?;
        assert!(
            effects::sending(
                store,
                "synthetic-effect",
                &sent.id,
                uncertain.version,
                1,
                "request-repeat"
            )
            .is_err()
        );
        assert!(effects::cancel(store, "synthetic-effect", &sent.id, uncertain.version).is_err());
        let observing = effects::observing(
            store,
            "synthetic-effect",
            &sent.id,
            uncertain.version,
            1,
            "request-observe",
        )?;
        assert!(
            effects::record(
                store,
                "synthetic-effect",
                &sent.id,
                observing.version,
                1,
                reply(&sent.id, "request-send", Method::Reconcile, Some("applied")),
                true
            )
            .is_err()
        );
        let unknown = effects::record(
            store,
            "synthetic-effect",
            &sent.id,
            observing.version,
            1,
            reply(&sent.id, "request-observe", Method::Reconcile, None),
            true,
        )?;
        assert_eq!(unknown.state, Phase::Uncertain);
        let observing = effects::observing(
            store,
            "synthetic-effect",
            &sent.id,
            unknown.version,
            1,
            "request-final",
        )?;
        assert!(
            effects::record(
                store,
                "synthetic-effect",
                &sent.id,
                observing.version,
                1,
                reply(
                    "effect-other",
                    "request-final",
                    Method::Reconcile,
                    Some("applied")
                ),
                true
            )
            .is_err()
        );
        let observed = effects::record(
            store,
            "synthetic-effect",
            &sent.id,
            observing.version,
            1,
            reply(
                &sent.id,
                "request-final",
                Method::Reconcile,
                Some("applied"),
            ),
            true,
        )?;
        assert_eq!(observed.state, Phase::Reconciled);
        assert!(backup_directory.restore(store, &before.name).is_err());
        assert!(!effects::blocks(store, "/api/cards/operations")?);
        effects::recover(store)?;
        assert_eq!(
            effects::current(store, "synthetic-effect", &sent.id)?.version,
            observed.version
        );
        Ok(())
    })
    .await
    .unwrap();
    db.close().await.unwrap();
}

#[tokio::test]
async fn unsubmitted_previews_are_cancelled_on_restart_and_only_explicit_not_attempted_closes_a_send()
 {
    let db = Database::open(std::path::Path::new(":memory:")).unwrap();
    db.call(|store| {
        store.save_settings(&settings())?;
        let prepared = effects::preview(store, "synthetic-effect", &preview())?;
        effects::recover(store)?;
        assert_eq!(
            effects::current(store, "synthetic-effect", &prepared.id)?.state,
            Phase::Failed
        );
        for outcome in ["unknown", "notAttempted"] {
            let prepared = effects::preview(store, "synthetic-effect", &preview())?;
            let sent = effects::sending(
                store,
                "synthetic-effect",
                &prepared.id,
                prepared.version,
                1,
                "request-send",
            )?;
            let result = effects::record(
                store,
                "synthetic-effect",
                &sent.id,
                sent.version,
                1,
                reply(&sent.id, "request-send", Method::Action, Some(outcome)),
                false,
            )?;
            if outcome == "unknown" {
                assert_eq!(result.state, Phase::Uncertain);
                let observing = effects::observing(
                    store,
                    "synthetic-effect",
                    &sent.id,
                    result.version,
                    1,
                    "request-read",
                )?;
                effects::record(
                    store,
                    "synthetic-effect",
                    &sent.id,
                    observing.version,
                    1,
                    reply(
                        &sent.id,
                        "request-read",
                        Method::Reconcile,
                        Some("notAttempted"),
                    ),
                    true,
                )?;
            } else {
                assert_eq!(result.state, Phase::Failed);
            }
        }
        assert_eq!(effects::read(&store.settings::<Document>()?)?.len(), 3);
        Ok(())
    })
    .await
    .unwrap();
    db.close().await.unwrap();
}
