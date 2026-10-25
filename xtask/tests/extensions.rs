#![cfg(windows)]
use atelier_app::infrastructure::extensions::Executable;
use atelier_core::application::extensions::{Manifest, Method, PROTOCOL, Request, Status};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::PathBuf, time::Duration};
#[path = "support/windows.rs"]
mod windows;

fn request(fault: &str) -> Request {
    Request {
        protocol: PROTOCOL.into(),
        request_id: format!("request-{fault}"),
        module: "synthetic-extension".into(),
        method: Method::Check,
        operation_id: None,
        action: None,
        settings: BTreeMap::from([("fault".into(), Value::String(fault.into()))]),
        secrets: BTreeMap::from([("token".into(), "synthetic-extension-private-token".into())]),
        input: Value::Null,
    }
}
async fn installed() -> (atelier_core::application::extensions::Binary, Manifest) {
    Executable::open(PathBuf::from(env!("CARGO_BIN_EXE_native-fixture")), None)
        .await
        .unwrap()
        .describe("synthetic-extension")
        .await
        .unwrap()
}

#[tokio::test]
async fn pinned_processes_keep_credentials_in_stdin_and_reject_untrusted_frames() {
    let (binary, manifest) = installed().await;
    let fixture = Executable::open(PathBuf::from(&binary.path), None)
        .await
        .unwrap();
    assert!(
        fixture
            .prepare(&manifest, request("success"), Duration::from_secs(15))
            .is_err()
    );
    assert!(
        Executable::open(PathBuf::from(&binary.path), Some("0".repeat(64)))
            .await
            .is_err()
    );
    for fault in [
        "success",
        "secret",
        "secret-key",
        "stderr",
        "identity",
        "extra",
        "partial",
        "flood",
    ] {
        let executable = Executable::open(PathBuf::from(&binary.path), Some(binary.sha256.clone()))
            .await
            .unwrap();
        let result = executable
            .prepare(&manifest, request(fault), Duration::from_secs(15))
            .unwrap()
            .dispatch()
            .await;
        if fault == "success" {
            let reply = result.unwrap();
            assert_eq!(reply.status, Status::Ok);
            assert_eq!(reply.output["privateHome"], true);
        } else {
            let error = result.unwrap_err().to_string();
            assert!(
                !error.contains("synthetic-extension-private-token"),
                "{fault}"
            );
        }
    }
    let executable = Executable::open(PathBuf::from(&binary.path), Some(binary.sha256.clone()))
        .await
        .unwrap();
    let mut public_secret = request("success");
    public_secret
        .settings
        .insert("token".into(), json!("must-never-be-public"));
    assert!(
        executable
            .prepare(&manifest, public_secret, Duration::from_secs(15))
            .is_err()
    );
    for path in [
        "relative.exe",
        r"\\synthetic-server\share\module.exe",
        "C:/synthetic-module.mjs",
    ] {
        assert!(Executable::open(PathBuf::from(path), None).await.is_err());
    }
}

#[tokio::test]
async fn timeout_stops_the_extension_and_its_descendant_before_returning() {
    let (binary, manifest) = installed().await;
    let fixture = tempfile::tempdir().unwrap();
    std::fs::write(
        fixture.path().join(".extension-fixture"),
        "atelier-r04-synthetic-only",
    )
    .unwrap();
    let executable = Executable::open(PathBuf::from(&binary.path), Some(binary.sha256))
        .await
        .unwrap();
    let mut request = request("hang");
    request.input = json!({"fixtureRoot":fixture.path()});
    let prepared = executable
        .prepare(&manifest, request, Duration::from_secs(3))
        .unwrap();
    let task = tokio::spawn(async move { prepared.dispatch().await });
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let pid = loop {
        if let Ok(pid) = std::fs::read_to_string(fixture.path().join("extension-child.pid")) {
            break pid.parse::<u32>().unwrap();
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "Synthetic descendant never started"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    let process = windows::open_process(pid).unwrap();
    assert!(windows::creation_time(&process).unwrap() > 0);
    assert!(task.await.unwrap().is_err());
    assert_eq!(
        windows::wait(&process, 1000),
        windows_sys::Win32::Foundation::WAIT_OBJECT_0
    );
}

#[tokio::test]
async fn a_pinned_synthetic_image_cannot_be_replaced_until_its_call_is_released() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("synthetic-extension.exe");
    std::fs::copy(env!("CARGO_BIN_EXE_native-fixture"), &path).unwrap();
    let executable = Executable::open(path.clone(), None).await.unwrap();
    let binary = executable.binary();
    assert!(std::fs::write(&path, b"replacement must fail while pinned").is_err());
    assert!(
        Executable::open(path.clone(), Some(binary.sha256.clone()))
            .await
            .is_ok()
    );
    drop(executable);
    std::fs::write(&path, b"synthetic replacement after release").unwrap();
    assert!(Executable::open(path, Some(binary.sha256)).await.is_err());
}
