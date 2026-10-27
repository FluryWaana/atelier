use crate::Result;
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{
    io::AsyncReadExt,
    process::{Child, Command},
};

const READY: &[u8] = b"atelier-tray/1 ready\n";

pub struct Owner(Child);

impl Owner {
    pub async fn start(data: &Path, port: u16) -> Result<Option<Self>> {
        #[cfg(target_os = "linux")]
        if (std::env::var_os("DISPLAY").is_none() && std::env::var_os("WAYLAND_DISPLAY").is_none())
            || std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_none()
        {
            return Ok(None);
        }
        if !data.is_absolute() || port == 0 {
            return Err("Identité du tray invalide.".into());
        }
        let mut command = Command::new(std::env::current_exe()?);
        command
            .arg("tray-helper")
            .arg(data)
            .arg(port.to_string())
            .arg(std::process::id().to_string())
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        for key in [
            "SystemRoot",
            "WINDIR",
            "TEMP",
            "TMP",
            "USERPROFILE",
            "LOCALAPPDATA",
            "APPDATA",
            "HOME",
            "DISPLAY",
            "WAYLAND_DISPLAY",
            "DBUS_SESSION_BUS_ADDRESS",
            "XDG_RUNTIME_DIR",
            "XDG_CONFIG_HOME",
            "XDG_DATA_HOME",
            "XDG_CACHE_HOME",
            "XDG_CURRENT_DESKTOP",
            "XDG_SESSION_DESKTOP",
            "DESKTOP_SESSION",
            "XAUTHORITY",
            "LANG",
            "LC_ALL",
        ] {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
        #[cfg(windows)]
        command.creation_flags(0x08000000);
        let mut child = command.spawn()?;
        let mut output = child.stdout.take().ok_or("Sortie du tray absente.")?;
        let mut ready = vec![0; READY.len()];
        let received =
            tokio::time::timeout(Duration::from_secs(5), output.read_exact(&mut ready)).await;
        if !matches!(received, Ok(Ok(_))) || ready != READY {
            let _ = child.kill().await;
            let _ = child.wait().await;
            return Err(
                "Icône système indisponible ; Atelier reste accessible dans le navigateur.".into(),
            );
        }
        Ok(Some(Self(child)))
    }

    pub async fn close(mut self) -> Result<()> {
        drop(self.0.stdin.take());
        match tokio::time::timeout(Duration::from_secs(5), self.0.wait()).await {
            Ok(Ok(status)) if status.success() => Ok(()),
            _ => {
                let _ = self.0.kill().await;
                let _ = self.0.wait().await;
                Err("L’icône système n’a pas confirmé son retrait.".into())
            }
        }
    }
}

fn arguments(args: impl IntoIterator<Item = OsString>) -> Result<(PathBuf, u16, u32)> {
    let mut args = args.into_iter();
    let data = args
        .next()
        .map(PathBuf::from)
        .ok_or("Dossier du tray absent.")?;
    let port = args
        .next()
        .and_then(|value| value.to_str().and_then(|value| value.parse::<u16>().ok()))
        .filter(|port| *port != 0)
        .ok_or("Port du tray invalide.")?;
    let owner = args
        .next()
        .and_then(|value| value.to_str().and_then(|value| value.parse::<u32>().ok()))
        .filter(|pid| *pid != 0)
        .ok_or("Propriétaire du tray invalide.")?;
    if !data.is_absolute()
        || data
            .to_str()
            .is_none_or(|value| value.len() > 32_000 || value.chars().any(char::is_control))
        || args.next().is_some()
    {
        return Err("Arguments du tray invalides.".into());
    }
    Ok((data, port, owner))
}

pub fn command(args: impl IntoIterator<Item = OsString>) -> Result<()> {
    let (data, port, owner) = arguments(args)?;
    #[cfg(target_os = "macos")]
    return objc2::rc::autoreleasepool(|_| run(data, port, owner));
    #[cfg(not(target_os = "macos"))]
    run(data, port, owner)
}

fn run(data: PathBuf, port: u16, owner: u32) -> Result<()> {
    use std::{
        io::{Read, Write},
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        thread,
    };
    use tray_icon::{
        Icon, TrayIconBuilder,
        menu::{Menu, MenuEvent, MenuItem},
    };

    #[cfg(target_os = "macos")]
    {
        use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
        let marker =
            objc2::MainThreadMarker::new().ok_or("Le tray macOS exige le thread principal.")?;
        let app = NSApplication::sharedApplication(marker);
        app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
        app.finishLaunching();
        pump();
    }
    let open = MenuItem::new("Ouvrir Atelier / Open Atelier", true, None);
    let stop = MenuItem::new("Quitter Atelier / Quit Atelier", true, None);
    let status = MenuItem::new("Prêt / Ready", false, None);
    let menu = Menu::with_items(&[&open, &stop, &status])?;
    let tray = TrayIconBuilder::new()
        .with_id(format!("atelier-rust-{}", std::process::id()))
        .with_tooltip("Atelier Kanban")
        .with_icon(Icon::from_rgba(pixels(), 16, 16)?)
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(true)
        .build()?;
    #[cfg(any(windows, target_os = "macos"))]
    {
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while tray.rect().is_none() {
            if std::time::Instant::now() >= deadline {
                return Err("L’icône système ne possède pas de position visible.".into());
            }
            pump();
        }
    }
    let disconnected = Arc::new(AtomicBool::new(false));
    let observer = disconnected.clone();
    thread::spawn(move || {
        let _ = std::io::stdin().read(&mut [0u8; 1]);
        observer.store(true, Ordering::Release);
    });
    std::io::stdout().write_all(READY)?;
    std::io::stdout().flush()?;
    let mut active = None::<thread::JoinHandle<std::result::Result<(), String>>>;
    while !disconnected.load(Ordering::Acquire) {
        pump();
        if active.as_ref().is_some_and(|active| active.is_finished()) {
            let result = active.take().ok_or("Action du tray absente.")?.join();
            let message = match result {
                Ok(Ok(())) => "Prêt / Ready",
                _ => "Action refusée ; consulte Atelier / Action refused; check Atelier",
            };
            status.set_text(message);
            open.set_enabled(true);
            stop.set_enabled(true);
            let _ = tray.set_tooltip(Some(format!("Atelier Kanban — {message}")));
        }
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            if active.is_some() {
                continue;
            }
            let quitting = event.id == *stop.id();
            if !quitting && event.id != *open.id() {
                continue;
            }
            open.set_enabled(false);
            stop.set_enabled(false);
            status.set_text("En cours / Working");
            let data = data.clone();
            active = Some(thread::spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|error| error.to_string())?;
                runtime
                    .block_on(async {
                        if quitting {
                            super::launcher::stop_owned(&data, port, Some(owner)).await
                        } else {
                            super::launcher::open_owned(&data, port, Some(owner)).await
                        }
                    })
                    .map_err(|error| error.to_string())
            }));
        }
    }
    // The GUI process owns only its icon; disconnect never stops or restarts a server.
    #[cfg(windows)]
    let window = tray.window_handle();
    drop(tray);
    #[cfg(windows)]
    // The tray library owns this hidden window and destroys it with its notification.
    if unsafe { windows_sys::Win32::UI::WindowsAndMessaging::IsWindow(window) } != 0 {
        return Err("Fenêtre de l’icône encore présente après retrait.".into());
    }
    Ok(())
}

fn pixels() -> Vec<u8> {
    let mut pixels = vec![0; 16 * 16 * 4];
    for y in 2..14 {
        for x in 2..14 {
            if x == 6 || x == 10 {
                continue;
            }
            let offset = (y * 16 + x) * 4;
            pixels[offset..offset + 4].copy_from_slice(&[40, 110, 220, 255]);
        }
    }
    pixels
}

#[cfg(windows)]
fn pump() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, TranslateMessage,
    };
    let mut message = MSG::default();
    // Only messages belonging to this helper's GUI thread are dispatched.
    unsafe {
        while PeekMessageW(&mut message, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    std::thread::sleep(Duration::from_millis(50));
}

#[cfg(target_os = "linux")]
fn pump() {
    std::thread::sleep(Duration::from_millis(50));
}

#[cfg(target_os = "macos")]
fn pump() {
    use objc2_foundation::{NSDate, NSRunLoop};
    objc2::rc::autoreleasepool(|_| {
        NSRunLoop::currentRunLoop().runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(0.05));
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn helper_validates_scope_without_opening_any_data_or_desktop() {
        let directory = tempfile::tempdir().unwrap();
        let valid = vec![
            directory.path().as_os_str().to_owned(),
            "4317".into(),
            "42".into(),
        ];
        assert!(arguments(valid.clone()).is_ok());
        for args in [
            vec![],
            vec!["relative".into(), "4317".into()],
            vec![valid[0].clone(), "0".into()],
            vec![valid[0].clone(), "4317".into(), "secret".into()],
        ] {
            assert!(arguments(args).is_err());
        }
        assert_eq!(pixels().len(), 16 * 16 * 4);
        assert!(
            std::fs::read_dir(directory.path())
                .unwrap()
                .next()
                .is_none()
        );
    }
}
