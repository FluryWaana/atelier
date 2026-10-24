#![cfg(windows)]

#[path = "support/windows.rs"]
mod windows;

use atelier_app::infrastructure::{
    command_runs::CommandRuns, database::Database, files::Repository,
    prototype::PrototypeDirectory, store::Documents,
};
use atelier_core::document::{Document, JsonString};
use std::{path::Path, sync::Arc, time::Duration};
use windows_sys::Win32::Foundation::WAIT_OBJECT_0;

#[tokio::test]
async fn explicit_commands_persist_failure_and_stop_descendants_before_releasing_activity() {
    let executable = Path::new(env!("CARGO_BIN_EXE_native-fixture"));
    for mode in ["failure", "hang"] {
        let fixture = tempfile::tempdir().unwrap();
        let root_path = std::fs::canonicalize(fixture.path()).unwrap();
        let repository = root_path.join("repositories/synthetic");
        std::fs::create_dir_all(repository.join(".git")).unwrap();
        std::fs::write(
            repository.join(".process-fixture"),
            "atelier-m04-synthetic-only",
        )
        .unwrap();
        let prototype = PrototypeDirectory::open(&root_path).unwrap();
        let root = prototype.opened_directory().unwrap();
        let selected = Repository::synthetic(
            &root,
            std::slice::from_ref(&root_path),
            &JsonString::from(repository.to_str().unwrap()),
        )
        .unwrap();
        let database = Arc::new(Database::open(&root_path.join("synthetic.sqlite")).unwrap());
        database
            .call(|store| {
                let card: Document = serde_json::from_str(r#"{"id":"card","title":"Synthetic"}"#)?;
                store.put(Documents::Cards, &card)
            })
            .await
            .unwrap();
        let manager = CommandRuns::new(tokio::sync::broadcast::channel(16).0);
        let mut record: Document =
            serde_json::from_str(r#"{"id":"run","cardId":"card","status":"starting"}"#).unwrap();
        record.set("repo", &repository.to_str().unwrap()).unwrap();
        let mut command = tokio::process::Command::new(executable);
        command
            .args(["process-output", mode])
            .arg(&repository)
            .current_dir(&repository);
        manager
            .start(database.clone(), record, command, String::new(), selected)
            .await
            .unwrap();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
        let descendant = loop {
            if let Ok(value) = std::fs::read_to_string(repository.join("pid"))
                && let Ok(pid) = value.parse()
            {
                break windows::open_process(pid).unwrap();
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "Synthetic command did not start"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        };
        assert_eq!(manager.active_cards(), vec!["card"]);
        assert!(windows::creation_time(&descendant).unwrap() > 0);
        if mode == "hang" {
            manager.stop("run").await.unwrap();
        } else {
            std::fs::write(repository.join("go"), "go").unwrap();
        }
        let run = loop {
            let run = database
                .call(|store| Ok(store.document::<Document>(Documents::Runs, "run")?.unwrap()))
                .await
                .unwrap();
            if !atelier_app::infrastructure::runs::active(&run).unwrap() {
                break run;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "Synthetic command did not finish"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        };
        assert!(
            run.string_equals(
                "status",
                if mode == "hang" {
                    "cancelled"
                } else {
                    "failed"
                }
            )
            .unwrap()
        );
        assert_eq!(windows::wait(&descendant, 10_000), WAIT_OBJECT_0);
        if mode == "failure" {
            assert_eq!(run.get::<i32>("exitCode").unwrap(), Some(7));
            let logs = database.call(|store| store.logs("run", 0)).await.unwrap();
            assert!(
                logs.iter()
                    .any(|log| log.text.contains("synthetic failure"))
            );
        }
        manager.close().await.unwrap();
        assert!(manager.active_cards().is_empty());
        database.close().await.unwrap();
    }
}
