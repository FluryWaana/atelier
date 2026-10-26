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
    let observed = http
        .authenticated_health(&format!("http://127.0.0.1:{port}"), || {
            session::read_optional(data)
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
    Ok(observed)
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
        let ready:Result<()>=async {
            let deadline=Instant::now()+Duration::from_secs(20);
            loop {
                if health(&http,data,port).await?.is_some(){return Ok(());}
                if child.try_wait()?.is_some(){return Err("Le serveur Atelier s’est arrêté au démarrage. Lance la commande serve avec ces dossiers pour lire le diagnostic.".into());}
                if Instant::now()>=deadline{return Err("Délai de démarrage d’Atelier dépassé.".into());}
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }.await;
        if let Err(error) = ready {
            let _ = child.kill().await;
            let _ = child.wait().await;
            return Err(error);
        }
    }
    if browser {
        let token = session::read(data)?;
        let address = session::browser_address(data, &format!("http://127.0.0.1:{port}"), &token)?;
        open_browser(&address).await?;
    }
    Ok(())
}

pub async fn stop(data: &Path, port: u16) -> Result<()> {
    if port == 0 {
        return Err("Port du lanceur invalide.".into());
    }
    let http = LocalHttp::new()?;
    if health(&http, data, port).await?.is_none() {
        return Ok(());
    }
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        format!("Bearer {}", session::read(data)?).parse()?,
    );
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
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if health(&http, data, port).await?.is_none() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err("Le serveur Atelier n’a pas confirmé son arrêt.".into());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
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
