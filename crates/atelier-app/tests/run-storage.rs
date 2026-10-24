use atelier_app::infrastructure::{database::Database, runs, store::Documents};
use atelier_core::document::Document;

fn document(json: &str) -> Document {
    serde_json::from_str(json).unwrap()
}

#[tokio::test]
async fn reservations_logs_and_interrupted_runs_survive_restart_without_relaunch() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("synthetic.sqlite");
    let database = Database::open(&path).unwrap();
    database.call(|store| {
        store.put(Documents::Cards, &document(r#"{"id":"card","title":"Synthetic"}"#))?;
        store.put(Documents::Cards, &document(r#"{"id":"other","title":"Synthetic"}"#))?;
        let run = document(r#"{"id":"run","cardId":"card","repo":"synthetic/worktree","status":"starting","pid":123,"interactions":[{"id":"stale"}]}"#);
        runs::reserve(store, &run)?;
        assert!(runs::reserve(store, &run).is_err());
        let mut collision = run.clone(); collision.set("id", &"collision")?;
        assert!(runs::reserve(store, &collision).is_err());
        collision.set("cardId", &"other")?;
        assert!(runs::reserve(store, &collision).is_err());
        runs::transition(store, "run", "running", "start", None)?;
        assert!(runs::transition(store, "run", "running", "again", None).is_err());
        let text = "🔧".repeat(20_000);
        runs::append(store, "run", "stdout", &text)?;
        let reconstructed: String = store.logs("run", 0)?.into_iter().map(|log| log.text).collect();
        assert_eq!(reconstructed, text);
        assert!(runs::append(store, "run", "unknown", "rejected").is_err());
        runs::append(store, "run", "stderr", &"x".repeat(runs::LOG_LIMIT))?;
        runs::append(store, "run", "stderr", "ignored")?;
        assert_eq!(store.logs("run", 0)?.iter().filter(|log| log.stream == "system").count(), 1);
        assert_eq!(store.document::<Document>(Documents::Runs, "run")?.unwrap().get::<usize>("logBytes")?, Some(text.len()));
        Ok(())
    }).await.unwrap();
    database.close().await.unwrap();
    drop(database);
    let database = Database::open(&path).unwrap();
    database
        .call(|store| {
            assert_eq!(runs::recover(store, "restart")?, 1);
            assert_eq!(runs::recover(store, "second restart")?, 0);
            let run = store.document::<Document>(Documents::Runs, "run")?.unwrap();
            assert!(run.string_equals("status", "interrupted")?);
            assert!(run.string_equals("endedAt", "restart")?);
            assert!(run.raw("pid").is_none());
            assert!(run.raw("interactions").is_none());
            assert!(runs::transition(store, "run", "completed", "late", Some(0)).is_err());
            assert!(runs::append(store, "run", "stdout", "late").is_err());
            assert!(
                store
                    .document::<Document>(Documents::Cards, "card")?
                    .unwrap()
                    .raw("status")
                    .is_none()
            );
            Ok(())
        })
        .await
        .unwrap();
    database.close().await.unwrap();
}

#[tokio::test]
async fn stopping_cannot_be_overwritten_by_a_late_success_and_recovery_rolls_back_on_error() {
    let database = Database::open(std::path::Path::new(":memory:")).unwrap();
    database
        .call(|store| {
            store.put(
                Documents::Cards,
                &document(r#"{"id":"card","title":"Synthetic"}"#),
            )?;
            runs::reserve(
                store,
                &document(r#"{"id":"run","cardId":"card","repo":"synthetic","status":"starting"}"#),
            )?;
            runs::transition(store, "run", "stopping", "stop", None)?;
            assert!(runs::transition(store, "run", "completed", "late", Some(0)).is_err());
            runs::transition(store, "run", "cancelled", "stopped", Some(1))?;
            assert_eq!(runs::recover(store, "restart")?, 0);
            store.put(Documents::Runs, &document(r#"{"id":"broken","status":42}"#))?;
            store.put(
                Documents::Runs,
                &document(r#"{"id":"pending","status":"running"}"#),
            )?;
            assert!(runs::recover(store, "restart").is_err());
            assert!(
                store
                    .document::<Document>(Documents::Runs, "pending")?
                    .unwrap()
                    .string_equals("status", "running")?
            );
            assert!(store.logs("pending", 0)?.is_empty());
            Ok(())
        })
        .await
        .unwrap();
    database.close().await.unwrap();
}
