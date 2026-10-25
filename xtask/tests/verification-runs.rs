#![cfg(windows)]
use atelier_app::infrastructure::{
    command_runs::{CommandRuns, RunDirectory, RunInput},
    database::Database,
    files::Repository,
    process_tree,
    store::Documents,
};
use atelier_core::document::{Document, JsonString};
use cap_std::{ambient_authority, fs::Dir};
use serde_json::json;
use std::{path::Path, sync::Arc, time::Duration};
async fn git(path: &Path, args: &[&str]) {
    let mut command = tokio::process::Command::new("git");
    command
        .current_dir(path)
        .env("GIT_CONFIG_GLOBAL", "NUL")
        .env("GIT_CONFIG_SYSTEM", "NUL")
        .args([
            "-c",
            "core.hooksPath=NUL",
            "-c",
            "user.name=Synthetic",
            "-c",
            "user.email=synthetic@example.invalid",
        ])
        .args(args);
    assert!(
        process_tree::capture(&mut command, &[], Duration::from_secs(15), 1024 * 1024)
            .await
            .unwrap()
            .status
            .success()
    );
}
#[tokio::test]
async fn compiled_verification_records_success_failure_and_changed_sources_without_checking_criteria()
 {
    for (mode, expected) in [
        ("success", "passed"),
        ("failure", "failed"),
        ("modify", "stale"),
    ] {
        let fixture = tempfile::tempdir().unwrap();
        let base = fixture.path().canonicalize().unwrap();
        let repo = base.join("repositories/verification");
        std::fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-b", "main"]).await;
        std::fs::write(repo.join(".run-fixture"), "atelier-r03-synthetic-only").unwrap();
        git(&repo, &["add", "."]).await;
        git(&repo, &["commit", "-qm", "Synthetic baseline"]).await;
        let root = Dir::open_ambient_dir(&base, ambient_authority()).unwrap();
        let repository = Repository::synthetic(
            &root,
            std::slice::from_ref(&base),
            &JsonString::from(repo.to_str().unwrap()),
        )
        .unwrap();
        let fingerprint = repository.fingerprint().await.unwrap();
        let database = Arc::new(Database::open(&base.join("synthetic.sqlite")).unwrap());
        let command = format!("synthetic verify {mode}");
        let worktree = json!({"source":repo,"path":repo,"branch":"main","base":"main","baseCommit":"a".repeat(40),"state":"ready"});
        let card:atelier_core::contracts::Card=serde_json::from_str(&json!({"id":"task","title":"Synthetic","repo":repo,"worktree":worktree,"checklist":[{"text":"Human criterion","done":false}]}).to_string()).unwrap();
        let requirements = atelier_core::application::preparation::requirements(&card).unwrap();
        let selected = card.clone();
        let configured = command.clone();
        database.call(move|store|{store.put(Documents::Cards,&selected)?;store.put(Documents::Projects,&json!({"id":"project","repo":selected.fields.get::<String>("repo")?,"testCommand":configured}))}).await.unwrap();
        let record:Document=serde_json::from_str(&json!({"id":"verify","cardId":"task","repo":repo,"purpose":"verification","status":"starting","verificationCommand":command,"verificationWorktree":worktree,"verificationState":"running","sourceFingerprint":fingerprint,"requirements":requirements}).to_string()).unwrap();
        let mut process = tokio::process::Command::new(env!("CARGO_BIN_EXE_native-fixture"));
        process.current_dir(&repo).args(["verify", mode]);
        let manager = CommandRuns::new(tokio::sync::broadcast::channel(16).0);
        manager
            .start(
                database.clone(),
                record,
                process,
                RunInput::Verification,
                RunDirectory::Worktree(repository),
            )
            .await
            .unwrap();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
        while !manager.active_cards().is_empty() {
            assert!(
                tokio::time::Instant::now() < deadline,
                "Verification did not finish"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        database
            .call(move |store| {
                let run = store
                    .document::<Document>(Documents::Runs, "verify")?
                    .unwrap();
                assert!(run.string_equals("verificationState", expected)?);
                let retained = store
                    .document::<atelier_core::contracts::Card>(Documents::Cards, "task")?
                    .unwrap();
                assert_eq!(
                    serde_json::to_value(&retained)?,
                    serde_json::to_value(&card)?
                );
                Ok(())
            })
            .await
            .unwrap();
        manager.close().await.unwrap();
        database.close().await.unwrap();
    }
}
