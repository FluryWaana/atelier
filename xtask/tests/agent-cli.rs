#![cfg(windows)]
use atelier_app::infrastructure::{agent_cli::Catalog, process_tree};
use atelier_core::{
    application::{
        agent_cli::{Availability, UpdateInfo},
        extensions::Binary,
    },
    document::Document,
};
use axum::{
    Json, Router,
    extract::{Request, State},
};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
#[path = "support/windows.rs"]
mod windows;

#[derive(Clone)]
struct Published(Arc<AtomicUsize>);
async fn latest(State(state): State<Published>, request: Request) -> Json<serde_json::Value> {
    state.0.fetch_add(1, Ordering::SeqCst);
    assert_eq!(request.method(), axum::http::Method::GET);
    assert_eq!(request.uri().path(), "/@openai%2Fcodex/latest");
    assert!(request.uri().query().is_none());
    assert!(!request.headers().contains_key("authorization"));
    Json(serde_json::json!({"name":"@openai/codex","version":"0.0.8"}))
}

struct Registry {
    catalog: Catalog,
    calls: Arc<AtomicUsize>,
    server: tokio::task::JoinHandle<()>,
}
impl Drop for Registry {
    fn drop(&mut self) {
        self.server.abort();
    }
}
impl Registry {
    async fn new() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let state = Published(calls.clone());
        let server = tokio::spawn(async move {
            axum::serve(listener, Router::new().fallback(latest).with_state(state))
                .await
                .unwrap();
        });
        Self {
            catalog: Catalog::synthetic(&format!("http://{address}/")).unwrap(),
            calls,
            server,
        }
    }
}

struct Fixture {
    _root: tempfile::TempDir,
    binary: PathBuf,
    environment: BTreeMap<String, OsString>,
    profile: Document,
    info: UpdateInfo,
}
impl Fixture {
    fn new(mode: &str) -> Self {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("synthetic-home");
        let bin = home.join(".local/bin");
        std::fs::create_dir_all(&bin).unwrap();
        let binary = bin.join("codex.exe");
        std::fs::copy(env!("CARGO_BIN_EXE_native-fixture"), &binary).unwrap();
        std::fs::write(
            bin.join(".cli-update-fixture"),
            "atelier-r03-cli-update/synthetic-only",
        )
        .unwrap();
        std::fs::write(bin.join("installed-version"), "0.0.7").unwrap();
        std::fs::write(bin.join("update-mode"), mode).unwrap();
        let mut environment = BTreeMap::from([
            ("USERPROFILE".into(), home.as_os_str().to_owned()),
            ("HOME".into(), home.as_os_str().to_owned()),
            (
                "OPENAI_API_KEY".into(),
                OsString::from("synthetic-key-must-be-dropped"),
            ),
            (
                "NODE_OPTIONS".into(),
                OsString::from("synthetic-injection-must-be-dropped"),
            ),
        ]);
        if let Some(system) = std::env::var_os("SystemRoot") {
            environment.insert("SYSTEMROOT".into(), system);
        }
        let image = Binary {
            path: binary.to_str().unwrap().to_owned(),
            sha256: atelier_app::infrastructure::session::hexadecimal(&Sha256::digest(
                std::fs::read(&binary).unwrap(),
            )),
        };
        let info = UpdateInfo {
            package: "@openai/codex".into(),
            latest_version: Some("0.0.8".into()),
            installed_version: Some("0.0.7".into()),
            managed_root: Some(home.join(".local").to_str().unwrap().replace('\\', "/")),
            observed_at: time::OffsetDateTime::now_utc()
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap(),
            availability: Availability::Available,
            image: Some(image),
        };
        let profile =
            serde_json::from_str(r#"{"id":"synthetic-update","type":"codex","auth":"cli"}"#)
                .unwrap();
        Self {
            _root: root,
            binary,
            environment,
            profile,
            info,
        }
    }
    fn directory(&self) -> &Path {
        self.binary.parent().unwrap()
    }
    fn clean_environment(&self) -> BTreeMap<String, OsString> {
        self.environment
            .iter()
            .filter(|(key, _)| !matches!(key.as_str(), "OPENAI_API_KEY" | "NODE_OPTIONS"))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect()
    }
    async fn version(&self) -> String {
        let mut command = tokio::process::Command::new(&self.binary);
        command
            .arg("--version")
            .current_dir(self.directory())
            .env_clear()
            .envs(self.clean_environment());
        let output = process_tree::capture(&mut command, &[], Duration::from_secs(5), 65_536)
            .await
            .unwrap();
        assert!(output.status.success());
        String::from_utf8(output.stdout).unwrap()
    }
}

#[tokio::test]
async fn the_confirmed_native_command_runs_once_with_filtered_environment_and_a_reobserved_version()
{
    let registry = Registry::new().await;
    let fixture = Fixture::new("success");
    let (mut command, pin, lock) = registry
        .catalog
        .prepare_selected(
            &fixture.profile,
            &fixture.info,
            fixture.binary.clone(),
            fixture.environment.clone(),
        )
        .await
        .unwrap();
    assert_eq!(
        command.as_std().get_args().collect::<Vec<_>>(),
        vec![std::ffi::OsStr::new("update")]
    );
    assert!(
        registry
            .catalog
            .prepare_selected(
                &fixture.profile,
                &fixture.info,
                fixture.binary.clone(),
                fixture.environment.clone()
            )
            .await
            .is_err()
    );
    let output = process_tree::capture_pinned(
        &mut command,
        pin,
        Duration::from_secs(10),
        1024 * 1024,
        std::future::pending(),
    )
    .await
    .unwrap();
    assert!(output.status.success());
    assert_eq!(fixture.version().await.trim(), "synthetic-agent 0.0.8");
    assert_eq!(
        std::fs::read(fixture.directory().join("update-attempted")).unwrap(),
        b"one synthetic native update"
    );
    assert_eq!(registry.calls.load(Ordering::SeqCst), 1);
    drop(lock);
    assert!(
        registry
            .catalog
            .prepare_selected(
                &fixture.profile,
                &fixture.info,
                fixture.binary.clone(),
                fixture.environment.clone()
            )
            .await
            .is_err()
    );
}

#[tokio::test]
async fn stale_versions_images_and_expired_confirmations_never_send_a_native_update() {
    for fault in ["expired", "image", "published"] {
        let registry = Registry::new().await;
        let fixture = Fixture::new("success");
        let mut info = fixture.info.clone();
        match fault {
            "expired" => info.observed_at = "2000-01-01T00:00:00Z".into(),
            "image" => info.image.as_mut().unwrap().sha256 = "0".repeat(64),
            _ => info.latest_version = Some("0.0.9".into()),
        }
        assert!(
            registry
                .catalog
                .prepare_selected(
                    &fixture.profile,
                    &info,
                    fixture.binary.clone(),
                    fixture.environment.clone()
                )
                .await
                .is_err()
        );
        assert!(!fixture.directory().join("update-attempted").exists());
        assert_eq!(fixture.version().await.trim(), "synthetic-agent 0.0.7");
        assert_eq!(
            registry.calls.load(Ordering::SeqCst),
            if fault == "published" { 1 } else { 0 }
        );
    }
}

#[tokio::test]
async fn a_different_image_is_killed_suspended_before_any_native_update_effect() {
    let registry = Registry::new().await;
    let fixture = Fixture::new("success");
    let (_, pin, _lock) = registry
        .catalog
        .prepare_selected(
            &fixture.profile,
            &fixture.info,
            fixture.binary.clone(),
            fixture.environment.clone(),
        )
        .await
        .unwrap();
    let other = fixture.directory().join("different-image.exe");
    std::fs::copy(env!("CARGO_BIN_EXE_native-fixture"), &other).unwrap();
    let mut command = tokio::process::Command::new(other);
    command
        .arg("update")
        .current_dir(fixture.directory())
        .env_clear()
        .envs(fixture.clean_environment());
    assert!(
        process_tree::capture_pinned(
            &mut command,
            pin,
            Duration::from_secs(5),
            1024 * 1024,
            std::future::pending()
        )
        .await
        .is_err()
    );
    assert!(!fixture.directory().join("update-attempted").exists());
}

#[tokio::test]
async fn interruption_stops_native_update_descendants_without_replaying_the_command() {
    let registry = Registry::new().await;
    let fixture = Fixture::new("hang");
    let (mut command, pin, lock) = registry
        .catalog
        .prepare_selected(
            &fixture.profile,
            &fixture.info,
            fixture.binary.clone(),
            fixture.environment.clone(),
        )
        .await
        .unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let task = tokio::spawn(async move {
        let _lock = lock;
        process_tree::capture_pinned(
            &mut command,
            pin,
            Duration::from_secs(15),
            1024 * 1024,
            async {
                let _ = stopped.await;
                atelier_app::Error::from("Synthetic update interruption")
            },
        )
        .await
    });
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let pid = loop {
        if let Ok(value) = std::fs::read_to_string(fixture.directory().join("update-child.pid"))
            && let Ok(pid) = value.parse()
        {
            break pid;
        }
        assert!(tokio::time::Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(10)).await;
    };
    let process = windows::open_process(pid).unwrap();
    assert!(windows::creation_time(&process).unwrap() > 0);
    stop.send(()).unwrap();
    assert!(task.await.unwrap().is_err());
    assert_eq!(
        windows::wait(&process, 1000),
        windows_sys::Win32::Foundation::WAIT_OBJECT_0
    );
    assert_eq!(fixture.version().await.trim(), "synthetic-agent 0.0.7");
    assert_eq!(
        std::fs::read(fixture.directory().join("update-attempted")).unwrap(),
        b"one synthetic native update"
    );
}
