#![cfg(unix)]
use atelier_app::infrastructure::process_tree;
use std::time::Duration;
use tokio::process::Command;
#[tokio::test]
async fn bounded_tools_preserve_exit_status_and_close_same_group_descendants() {
    let mut command = Command::new("/bin/sh");
    command.args([
        "-c",
        "sleep 30 & printf synthetic-output; printf synthetic-error >&2; exit 7",
    ]);
    let output = process_tree::capture(&mut command, &[], Duration::from_secs(5), 1024)
        .await
        .unwrap();
    assert_eq!(output.status.code(), Some(7));
    assert_eq!(output.stdout, b"synthetic-output");
    assert_eq!(output.stderr, b"synthetic-error");
}
#[tokio::test]
async fn bounded_tools_stop_on_timeout_overflow_and_guard_cancellation() {
    let mut command = Command::new("/bin/sh");
    command.args(["-c", "sleep 30 & wait"]);
    assert!(
        process_tree::capture(&mut command, &[], Duration::from_millis(150), 1024)
            .await
            .err()
            .unwrap()
            .to_string()
            .contains("Durée maximale")
    );
    let mut command = Command::new("/bin/sh");
    command.args(["-c", "while :; do printf synthetic-output; done"]);
    let overflow = process_tree::capture(&mut command, &[], Duration::from_secs(5), 100)
        .await
        .unwrap_err();
    assert!(
        overflow.to_string().contains("Limite de sortie"),
        "{overflow}"
    );
    let mut command = Command::new("/bin/sh");
    command.args(["-c", "sleep 30 & wait"]);
    let guard = async {
        tokio::time::sleep(Duration::from_millis(150)).await;
        "synthetic cancellation".into()
    };
    assert!(
        process_tree::capture_guarded(&mut command, &[], Duration::from_secs(5), 1024, guard)
            .await
            .err()
            .unwrap()
            .to_string()
            .contains("synthetic cancellation")
    );
}
