use std::{process::Stdio, time::Duration};
use tokio::{io::AsyncReadExt, process::Command};

#[tokio::test]
#[ignore = "requires the final campaign and a disposable native desktop"]
async fn the_product_tray_removes_itself_on_owner_disconnect_without_opening_data() {
    assert_eq!(std::env::var("CI").as_deref(), Ok("true"));
    assert_eq!(
        std::env::var("ATELIER_RUST_FINAL_CAMPAIGN").as_deref(),
        Ok("1")
    );
    let synthetic = tempfile::tempdir().unwrap();
    let data = synthetic.path().join("never-opened-data");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let mut command = Command::new(env!("CARGO_BIN_EXE_atelier"));
    command
        .arg("tray-helper")
        .arg(&data)
        .arg(port.to_string())
        .arg(std::process::id().to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    let mut child = command.spawn().unwrap();
    let mut output = child.stdout.take().unwrap();
    let mut ready = [0; b"atelier-tray/1 ready\n".len()];
    tokio::time::timeout(Duration::from_secs(10), output.read_exact(&mut ready))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(&ready, b"atelier-tray/1 ready\n");
    assert!(!data.exists());
    drop(child.stdin.take());
    let status = tokio::time::timeout(Duration::from_secs(10), child.wait())
        .await
        .unwrap()
        .unwrap();
    assert!(status.success());
    assert!(!data.exists());
    assert!(
        std::fs::read_dir(synthetic.path())
            .unwrap()
            .next()
            .is_none()
    );
}

#[tokio::test]
async fn the_product_tray_rejects_missing_or_foreign_arguments_before_native_ui() {
    let synthetic = tempfile::tempdir().unwrap();
    for args in [
        vec![],
        vec!["relative", "4317"],
        vec!["--data-dir", "4317"],
        vec!["relative", "4317", "extra"],
    ] {
        let result = Command::new(env!("CARGO_BIN_EXE_atelier"))
            .arg("tray-helper")
            .args(args)
            .current_dir(synthetic.path())
            .output()
            .await
            .unwrap();
        assert!(!result.status.success());
        assert!(result.stdout.is_empty());
    }
    assert!(
        std::fs::read_dir(synthetic.path())
            .unwrap()
            .next()
            .is_none()
    );
}
