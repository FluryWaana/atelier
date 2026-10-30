#![cfg(unix)]
use atelier_app::infrastructure::{agent_diagnostics::diagnose, process_tree::ContainedChild};
use atelier_core::{contracts::AgentAuthentication, document::Document};
use std::{process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, BufReader},
    process::Command,
};

#[tokio::test]
async fn persistent_commands_keep_the_activity_guard_until_the_owned_group_is_stopped() {
    use atelier_app::infrastructure::{
        command_runs::{CommandRuns, RunDirectory, RunInput},
        database::Database,
        files::Repository,
        prototype::PrototypeDirectory,
        store::Documents,
    };

    use std::sync::Arc;
    let root = tempfile::tempdir().unwrap();
    let data = PrototypeDirectory::open(root.path()).unwrap();
    let repository = data.path.join("repositories/synthetic");
    std::fs::create_dir_all(repository.join(".git")).unwrap();
    std::fs::write(
        repository.join(".process-fixture"),
        "atelier-m04-synthetic-only",
    )
    .unwrap();
    let opened = Repository::synthetic(
        &data.opened_directory().unwrap(),
        std::slice::from_ref(&data.path),
        (repository.to_str().unwrap()),
    )
    .unwrap();
    let database = Arc::new(Database::open(std::path::Path::new(":memory:")).unwrap());
    database
        .call(|store| {
            store.put(
                Documents::Cards,
                &serde_json::json!({"id":"card","title":"Synthetic"}),
            )
        })
        .await
        .unwrap();
    let mut record: Document =
        serde_json::from_str(r#"{"id":"run","cardId":"card","status":"starting"}"#).unwrap();
    record.set("repo", &repository.to_str().unwrap()).unwrap();
    let manager = CommandRuns::new(tokio::sync::broadcast::channel(16).0);
    let mut command = Command::new(env!("CARGO_BIN_EXE_native-fixture"));
    command
        .args(["process-output", "hang"])
        .arg(&repository)
        .env_clear()
        .current_dir(&repository);
    manager
        .start(
            database.clone(),
            record,
            command,
            RunInput::Command(String::new()),
            RunDirectory::Worktree(opened),
        )
        .await
        .unwrap();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while !repository.join("pid").exists() {
        assert!(tokio::time::Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(manager.active_cards(), vec!["card"]);
    manager.stop("run").await.unwrap();
    assert!(manager.active_cards().is_empty());
    let run = database
        .call(|store| Ok(store.document::<Document>(Documents::Runs, "run")?.unwrap()))
        .await
        .unwrap();
    assert!(run.string_equals("status", "cancelled").unwrap());
    assert_eq!(
        std::fs::read_to_string(repository.join(".process-fixture")).unwrap(),
        "atelier-m04-synthetic-only"
    );
    manager.close().await.unwrap();
    database.close().await.unwrap();
}

#[tokio::test]
async fn compiled_synthetic_diagnostics_resolve_native_programs_without_shell_or_secrets() {
    let root = tempfile::tempdir().unwrap();
    let binary = root.path().join("synthetic-agent");
    std::fs::copy(env!("CARGO_BIN_EXE_native-fixture"), &binary).unwrap();
    let profile:Document=serde_json::from_str(&serde_json::json!({"id":"synthetic","type":"command","auth":"cli","command":binary,"statusArgs":["agent-status","present"]}).to_string()).unwrap();
    let diagnostic = diagnose(profile, None, false).await.unwrap();
    assert_eq!(diagnostic.version.as_deref(), Some("0.0.7"));
    assert_eq!(diagnostic.authentication, AgentAuthentication::Present);
    assert!(
        !serde_json::to_string(&diagnostic)
            .unwrap()
            .contains("private-output-must-not-be-returned")
    );
}

#[tokio::test]
async fn a_setsid_descendant_demonstrates_the_group_limit_and_is_closed_through_its_owned_pipe() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join(".agent-run-fixture"),
        "atelier-r03-synthetic-only",
    )
    .unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_native-fixture"));
    command
        .arg("posix-detached-parent")
        .current_dir(root.path())
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut parent = ContainedChild::spawn(&mut command).await.unwrap();
    let mut input = parent.child.stdin.take();
    let mut output = BufReader::new(parent.child.stdout.take().unwrap());
    let mut errors = parent.child.stderr.take().unwrap();
    let mut line = String::new();
    tokio::time::timeout(Duration::from_secs(5), output.read_line(&mut line))
        .await
        .unwrap()
        .unwrap();
    let pid = line
        .trim()
        .strip_prefix("detached-ready ")
        .unwrap()
        .parse::<libc::pid_t>()
        .unwrap();
    assert_eq!(unsafe { libc::getpgid(pid) }, pid);
    assert!(
        tokio::time::timeout(Duration::from_secs(5), parent.wait())
            .await
            .unwrap()
            .unwrap()
            .success()
    );
    // Group cleanup succeeded, but this separately sessioned descendant is still alive.
    assert_eq!(unsafe { libc::getpgid(pid) }, pid);
    drop(input.take());
    let mut discarded = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), errors.read_to_end(&mut discarded))
        .await
        .unwrap()
        .unwrap();
    assert!(discarded.is_empty());
}
