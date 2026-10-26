use atelier_app::infrastructure::{launcher, local_http::LocalHttp, session};
use serde_json::json;
use std::{path::Path, time::Duration};

#[test]
fn the_folder_helper_rejects_arguments_before_opening_a_dialog_or_data_directory() {
    let synthetic = tempfile::tempdir().unwrap();
    let forbidden = synthetic.path().join("must-not-be-created");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_atelier"))
        .args(["choose-folder", "--data-dir"])
        .arg(&forbidden)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(!forbidden.exists());
}

#[test]
fn transfer_dialogs_reject_unconfirmed_or_invalid_paths_before_opening_native_ui() {
    let synthetic = tempfile::tempdir().unwrap();
    for args in [
        vec!["confirm-import"],
        vec!["confirm-import", "relative", "relative"],
        vec!["import-data", "--choose-source"],
        vec![
            "import-data",
            "--choose-source",
            "--destination",
            "relative",
            "--confirm",
        ],
    ] {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_atelier"))
            .args(args)
            .current_dir(synthetic.path())
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
    }
    assert!(
        std::fs::read_dir(synthetic.path())
            .unwrap()
            .next()
            .is_none()
    );
}

struct Instance {
    data: tempfile::TempDir,
    port: u16,
}
impl Drop for Instance {
    fn drop(&mut self) {
        let data = self.data.path().to_owned();
        let port = self.port;
        let _ = std::thread::spawn(move || {
            if let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                let _ = runtime.block_on(launcher::stop(&data, port));
            }
        })
        .join();
    }
}
fn port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}
async fn health(instance: &Instance) -> serde_json::Value {
    LocalHttp::new()
        .unwrap()
        .authenticated_health(&format!("http://127.0.0.1:{}", instance.port), || {
            session::read_optional(instance.data.path())
        })
        .await
        .unwrap()
        .unwrap()
}

#[tokio::test]
async fn native_launcher_reuses_only_its_authenticated_instance_and_preserves_data_on_restart() {
    let instance = Instance {
        data: tempfile::tempdir().unwrap(),
        port: port(),
    };
    let assets = tempfile::tempdir().unwrap();
    std::fs::write(
        assets.path().join("index.html"),
        "Synthetic Rust launcher fixture",
    )
    .unwrap();
    let executable = Path::new(env!("CARGO_BIN_EXE_atelier"));
    launcher::start(
        executable,
        instance.data.path(),
        assets.path(),
        instance.port,
        false,
    )
    .await
    .unwrap();
    let first = health(&instance).await;
    assert_eq!(first["protocol"], "atelier-local/1");
    launcher::start(
        executable,
        instance.data.path(),
        assets.path(),
        instance.port,
        false,
    )
    .await
    .unwrap();
    assert_eq!(health(&instance).await["pid"], first["pid"]);
    let other = tempfile::tempdir().unwrap();
    assert!(
        launcher::start(
            executable,
            other.path(),
            assets.path(),
            instance.port,
            false
        )
        .await
        .is_err()
    );
    assert!(launcher::stop(other.path(), instance.port).await.is_err());
    assert_eq!(std::fs::read_dir(other.path()).unwrap().count(), 0);
    assert_eq!(health(&instance).await["pid"], first["pid"]);
    let token = session::read(instance.data.path()).unwrap();
    let http = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    let address = format!("http://127.0.0.1:{}", instance.port);
    let card: serde_json::Value = http
        .post(format!("{address}/api/brainstorms"))
        .bearer_auth(&token)
        .header("x-local-request", "1")
        .json(&json!({"title":"Synthetic launcher persistence"}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    launcher::stop(instance.data.path(), instance.port)
        .await
        .unwrap();
    launcher::start(
        executable,
        instance.data.path(),
        assets.path(),
        instance.port,
        false,
    )
    .await
    .unwrap();
    let next = session::read(instance.data.path()).unwrap();
    assert_ne!(token, next);
    let state: serde_json::Value = http
        .get(format!("{address}/api/state"))
        .bearer_auth(next)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(state["cards"].as_array().unwrap().iter().any(
        |saved| saved["id"] == card["id"] && saved["title"] == "Synthetic launcher persistence"
    ));
    launcher::stop(instance.data.path(), instance.port)
        .await
        .unwrap();
    launcher::stop(instance.data.path(), instance.port)
        .await
        .unwrap();
}

#[tokio::test]
async fn native_service_refuses_legacy_assets_without_initializing_a_data_directory() {
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("synthetic-unused");
    let assets = tempfile::tempdir().unwrap();
    std::fs::write(assets.path().join("index.html"), "Synthetic").unwrap();
    std::fs::write(assets.path().join("app.js"), "Never load legacy code").unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    assert!(
        atelier_app::http::serve_native(listener, data.clone(), assets.path().to_owned())
            .await
            .is_err()
    );
    assert!(!data.exists());
}

#[tokio::test]
async fn occupied_unverified_ports_cannot_initialize_data_or_receive_a_launcher_command() {
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("synthetic-unused");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            axum::Router::new().route(
                "/api/health",
                axum::routing::get(|| async { axum::Json(json!({"proof":"untrusted-synthetic"})) }),
            ),
        )
        .await
        .unwrap();
    });
    let error = launcher::start(
        Path::new("must-never-execute"),
        &data,
        root.path(),
        port,
        false,
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(error.contains("Identité du serveur local non vérifiée"));
    assert!(!data.exists());
    assert!(launcher::stop(&data, port).await.is_err());
    server.abort();
    let _ = server.await;
}
