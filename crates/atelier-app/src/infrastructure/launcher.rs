use super::{local_http::LocalHttp, session};
use crate::Result;
use reqwest::{
    Method,
    header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap},
};
use serde_json::Value;
use std::{path::Path, process::Stdio, time::Duration};
use tokio::{process::Command, time::Instant};

pub(super) async fn health(http: &LocalHttp, data: &Path, port: u16) -> Result<Option<Value>> {
    Ok(identity(http, data, port).await?.map(|(value, _)| value))
}

async fn identity(http: &LocalHttp, data: &Path, port: u16) -> Result<Option<(Value, String)>> {
    let mut token = None;
    let observed = http
        .authenticated_health(&format!("http://127.0.0.1:{port}"), || {
            token = session::read_optional(data)?;
            Ok(token.clone())
        })
        .await?;
    if let Some(value) = &observed {
        let directory = value["dataDirectory"]
            .as_str()
            .ok_or("Cette instance ne fournit pas son identité de données Rust.")?;
        if value["app"] != "atelier-kanban"
            || value["protocol"] != "atelier-local/1"
            || std::fs::canonicalize(directory)? != std::fs::canonicalize(data)?
        {
            return Err("Ce port appartient à une autre instance d’Atelier.".into());
        }
    }
    observed
        .map(|value| Ok((value, token.ok_or("Session vérifiée absente.")?)))
        .transpose()
}

pub async fn start(
    executable: &Path,
    data: &Path,
    assets: &Path,
    port: u16,
    browser: bool,
) -> Result<()> {
    if port == 0 {
        return Err("Port du lanceur invalide.".into());
    }
    let http = LocalHttp::new()?;
    if health(&http, data, port).await?.is_none() {
        let mut command = Command::new(executable);
        command
            .arg("serve")
            .arg("--data-dir")
            .arg(data)
            .arg("--assets")
            .arg(assets)
            .arg("--port")
            .arg(port.to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(windows)]
        command.creation_flags(0x08000000 | 0x00000200);
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.as_std_mut().process_group(0);
        }
        let mut child = command.spawn()?;
        let ready = wait_for_ready(
            &http,
            data,
            port,
            &mut child,
            Instant::now() + Duration::from_secs(20),
        )
        .await;
        if let Err(error) = ready {
            let _ = child.kill().await;
            let _ = child.wait().await;
            return Err(error);
        }
    }
    if browser {
        open(data, port).await?;
    }
    Ok(())
}

async fn wait_for_ready(
    http: &LocalHttp,
    data: &Path,
    port: u16,
    child: &mut tokio::process::Child,
    deadline: Instant,
) -> Result<()> {
    loop {
        if Instant::now() >= deadline {
            return Err("Délai de démarrage d’Atelier dépassé.".into());
        }
        if child.try_wait()?.is_some() {
            return Err("Le serveur Atelier s’est arrêté au démarrage. Lance la commande serve avec ces dossiers pour lire le diagnostic.".into());
        }
        match tokio::time::timeout_at(deadline, health(http, data, port)).await {
            Ok(Ok(Some(_))) => return Ok(()),
            Ok(Ok(None)) => {}
            Ok(Err(error))
                if error
                    .downcast_ref::<reqwest::Error>()
                    .is_some_and(reqwest::Error::is_timeout) => {}
            Ok(Err(error)) => return Err(error),
            Err(_) => return Err("Délai de démarrage d’Atelier dépassé.".into()),
        }
        tokio::time::sleep_until(deadline.min(Instant::now() + Duration::from_millis(100))).await;
    }
}

pub async fn open(data: &Path, port: u16) -> Result<()> {
    open_owned(data, port, None).await
}

pub(super) async fn open_owned(data: &Path, port: u16, owner: Option<u32>) -> Result<()> {
    if port == 0 {
        return Err("L’instance Atelier n’est plus disponible.".into());
    }
    let (observed, token) = identity(&LocalHttp::new()?, data, port)
        .await?
        .ok_or("L’instance Atelier n’est plus disponible.")?;
    require_owner(&observed, owner)?;
    let address = session::browser_address(data, &format!("http://127.0.0.1:{port}"), &token)?;
    open_browser(&address).await
}

pub async fn stop(data: &Path, port: u16) -> Result<()> {
    stop_owned(data, port, None).await
}

pub(super) async fn stop_owned(data: &Path, port: u16, owner: Option<u32>) -> Result<()> {
    if port == 0 {
        return Err("Port du lanceur invalide.".into());
    }
    let http = LocalHttp::new()?;
    let deadline = Instant::now() + Duration::from_secs(30);
    let Some((observed, token)) = identity(&http, data, port).await? else {
        return wait_for_data_close(data, deadline).await;
    };
    require_owner(&observed, owner)?;
    let mut headers = HeaderMap::new();
    headers.insert(AUTHORIZATION, format!("Bearer {token}").parse()?);
    headers.insert(CONTENT_TYPE, "application/json".parse()?);
    headers.insert("x-local-request", "1".parse()?);
    let response = http
        .request(
            &format!("http://127.0.0.1:{port}/api/shutdown"),
            Method::POST,
            headers,
            Some("{}"),
            Duration::from_secs(30),
        )
        .await?;
    if !response.status.is_success() {
        return Err("Arrêt d’Atelier refusé ; les opérations en cours sont conservées.".into());
    }
    loop {
        if health(&http, data, port).await?.is_none() {
            return wait_for_data_close(data, deadline).await;
        }
        if Instant::now() >= deadline {
            return Err("Le serveur Atelier n’a pas confirmé son arrêt.".into());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn wait_for_data_close(data: &Path, deadline: Instant) -> Result<()> {
    let data = data.to_owned();
    tokio::task::spawn_blocking(move || {
        let directory = match cap_std::fs::Dir::open_ambient_dir(&data, cap_std::ambient_authority()) {
            Ok(directory) => directory,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        match directory.symlink_metadata(super::data_lock::LOCK_NAME) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.into()),
            Ok(metadata) => super::checked_file::require_plain(&metadata)?,
        }
        loop {
            if let Ok(lock) = super::data_lock::DataLock::acquire_in(&data, &directory) {
                drop(lock);
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err("Le dossier Atelier est encore ouvert ou son verrou a changé. Aucun redémarrage confirmé.".into());
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }).await?
}

fn require_owner(observed: &Value, owner: Option<u32>) -> Result<()> {
    if owner.is_some_and(|owner| observed["pid"].as_u64() != Some(u64::from(owner))) {
        return Err(
            "L’instance propriétaire de cette icône a changé. Rouvre Atelier avec son lanceur."
                .into(),
        );
    }
    Ok(())
}

async fn open_browser(address: &str) -> Result<()> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL};
        let operation: Vec<u16> = "open\0".encode_utf16().collect();
        let address: Vec<u16> = address.encode_utf16().chain(Some(0)).collect();
        // ShellExecute copies these terminated buffers during the call; no child command is logged.
        let result = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                operation.as_ptr(),
                address.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                SW_SHOWNORMAL,
            )
        } as isize;
        if result <= 32 {
            return Err(
                "Le navigateur n’a pas pu ouvrir Atelier. Le serveur reste disponible.".into(),
            );
        }
        Ok(())
    }
    #[cfg(unix)]
    {
        let program = if cfg!(target_os = "macos") {
            "/usr/bin/open"
        } else {
            "/usr/bin/xdg-open"
        };
        let mut child = Command::new(program)
            .arg(address)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()?;
        match tokio::time::timeout(Duration::from_secs(15), child.wait()).await {
            Ok(Ok(status)) if status.success() => Ok(()),
            _ => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                Err("Le navigateur n’a pas pu ouvrir Atelier. Le serveur reste disponible.".into())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Json, Router,
        extract::{Query, State},
        http::HeaderMap,
        response::IntoResponse,
        routing::get,
    };
    use std::{
        collections::HashMap,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
    };

    #[test]
    #[ignore = "Child fixture invoked only by the bounded launcher readiness test"]
    fn synthetic_startup_child() {
        assert_eq!(
            std::fs::read_to_string(".launcher-startup-fixture").unwrap(),
            "atelier-launcher-synthetic-only"
        );
        std::thread::sleep(Duration::from_secs(30));
    }
    #[derive(Clone, Copy)]
    enum Readiness {
        AfterTimeouts,
        Timeout,
        WrongIdentity,
    }
    #[derive(Clone)]
    struct ReadyState {
        mode: Readiness,
        data: std::path::PathBuf,
        port: u16,
        pid: u32,
        token: String,
        proofs: Arc<AtomicUsize>,
        credentials: Arc<AtomicUsize>,
        release: Arc<tokio::sync::Notify>,
    }
    async fn startup_health(
        State(state): State<ReadyState>,
        Query(query): Query<HashMap<String, String>>,
        headers: HeaderMap,
    ) -> axum::response::Response {
        if let Some(challenge) = query.get("challenge") {
            assert!(!headers.contains_key("authorization"));
            let count = state.proofs.fetch_add(1, Ordering::SeqCst) + 1;
            if matches!(state.mode, Readiness::Timeout)
                || (matches!(state.mode, Readiness::AfterTimeouts) && count <= 3)
            {
                state.release.notified().await;
                return axum::http::StatusCode::REQUEST_TIMEOUT.into_response();
            }
            if matches!(state.mode, Readiness::WrongIdentity) {
                return Json(serde_json::json!({"proof":"not-an-atelier-proof"})).into_response();
            }
            return Json(serde_json::json!({"proof":session::proof(&state.token,challenge,state.port).unwrap()})).into_response();
        }
        assert_eq!(headers["authorization"], format!("Bearer {}", state.token));
        state.credentials.fetch_add(1, Ordering::SeqCst);
        Json(serde_json::json!({"app":"atelier-kanban","protocol":"atelier-local/1","pid":state.pid,"dataDirectory":state.data})).into_response()
    }
    #[tokio::test]
    async fn child_readiness_retries_only_health_timeouts_and_bounds_the_entire_wait() {
        for mode in [
            Readiness::AfterTimeouts,
            Readiness::Timeout,
            Readiness::WrongIdentity,
        ] {
            let directory = tempfile::tempdir().unwrap();
            std::fs::write(
                directory.path().join(".launcher-startup-fixture"),
                "atelier-launcher-synthetic-only",
            )
            .unwrap();
            let token = session::new_token().unwrap();
            session::write(directory.path(), &token).unwrap();
            let mut command = Command::new(std::env::current_exe().unwrap());
            command
                .env_clear()
                .current_dir(directory.path())
                .args([
                    "--ignored",
                    "--exact",
                    "infrastructure::launcher::tests::synthetic_startup_child",
                ])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .kill_on_drop(true);
            for name in ["SystemRoot", "WINDIR"] {
                if let Some(value) = std::env::var_os(name) {
                    command.env(name, value);
                }
            }
            #[cfg(windows)]
            command.creation_flags(0x08000000);
            let mut child = command.spawn().unwrap();
            let pid = child.id().unwrap();
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let port = listener.local_addr().unwrap().port();
            let state = ReadyState {
                mode,
                data: directory.path().to_owned(),
                port,
                pid,
                token,
                proofs: Arc::default(),
                credentials: Arc::default(),
                release: Arc::default(),
            };
            let router = Router::new()
                .route("/api/health", get(startup_health))
                .with_state(state.clone());
            let server = tokio::spawn(async { axum::serve(listener, router).await.unwrap() });
            let limit = if matches!(mode, Readiness::Timeout) {
                Duration::from_millis(250)
            } else {
                Duration::from_secs(20)
            };
            let deadline = Instant::now() + limit;
            let result = wait_for_ready(
                &LocalHttp::new().unwrap(),
                directory.path(),
                port,
                &mut child,
                deadline,
            )
            .await;
            let still_owned_and_running =
                child.id() == Some(pid) && child.try_wait().unwrap().is_none();
            let _ = child.kill().await;
            let _ = child.wait().await;
            state.release.notify_waiters();
            server.abort();
            assert!(
                still_owned_and_running,
                "The readiness probe must not replace or kill the child"
            );
            match mode {
                Readiness::AfterTimeouts => {
                    result.unwrap();
                    assert_eq!(state.proofs.load(Ordering::SeqCst), 4);
                    assert_eq!(state.credentials.load(Ordering::SeqCst), 1);
                }
                Readiness::Timeout => {
                    assert!(
                        result
                            .unwrap_err()
                            .to_string()
                            .contains("Délai de démarrage")
                    );
                    assert!(state.proofs.load(Ordering::SeqCst) <= 1);
                    assert_eq!(state.credentials.load(Ordering::SeqCst), 0);
                }
                Readiness::WrongIdentity => {
                    assert!(
                        result
                            .unwrap_err()
                            .to_string()
                            .contains("Identité du serveur local non vérifiée")
                    );
                    assert_eq!(state.proofs.load(Ordering::SeqCst), 1);
                    assert_eq!(state.credentials.load(Ordering::SeqCst), 0);
                }
            }
        }
    }
    #[tokio::test]
    async fn shutdown_waits_for_data_lock_release_without_touching_business_data() {
        let root = tempfile::tempdir().unwrap();
        let lock = super::super::data_lock::DataLock::acquire(root.path()).unwrap();
        let data = root.path().to_owned();
        let waiting = tokio::spawn(async move {
            wait_for_data_close(&data, Instant::now() + Duration::from_secs(2)).await
        });
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(!waiting.is_finished());
        drop(lock);
        waiting.await.unwrap().unwrap();
        assert!(!root.path().join("kanban.sqlite").exists());
        let absent = root.path().join("never-created");
        wait_for_data_close(&absent, Instant::now()).await.unwrap();
        assert!(!absent.exists());
    }
    #[test]
    fn a_stale_tray_cannot_control_another_server_on_the_same_port() {
        let observed = serde_json::json!({"pid":42});
        assert!(require_owner(&observed, Some(42)).is_ok());
        assert!(require_owner(&observed, Some(7)).is_err());
        assert!(require_owner(&serde_json::json!({}), Some(42)).is_err());
        assert!(require_owner(&observed, None).is_ok());
    }
}
