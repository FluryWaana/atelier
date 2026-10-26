use crate::Result;
use std::{path::Path, time::Duration};
use tokio::process::Command;

fn selected(value: &str) -> Result<Option<String>> {
    if value.is_empty() {
        return Ok(None);
    }
    if value.len() > 32_000
        || value.chars().any(char::is_control)
        || !Path::new(value).is_absolute()
    {
        return Err("Le sélecteur a retourné un chemin invalide.".into());
    }
    Ok(Some(value.into()))
}
async fn helper(name: &str, paths: &[&Path]) -> Result<std::process::Output> {
    let mut command = Command::new(std::env::current_exe()?);
    command.arg(name).args(paths);
    environment(&mut command);
    super::process_tree::capture(&mut command, &[], Duration::from_secs(300), 80 * 1024).await
}
fn environment(command: &mut Command) {
    command.env_clear();
    for key in [
        "SystemRoot",
        "WINDIR",
        "TEMP",
        "TMP",
        "USERPROFILE",
        "APPDATA",
        "LOCALAPPDATA",
        "DISPLAY",
        "WAYLAND_DISPLAY",
        "DBUS_SESSION_BUS_ADDRESS",
        "XDG_RUNTIME_DIR",
        "HOME",
        "LANG",
        "LC_ALL",
        "XAUTHORITY",
    ] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
}
pub async fn choose() -> Result<Option<String>> {
    let output = helper("choose-folder", &[]).await?;
    if !output.status.success() {
        return Err("Le sélecteur est indisponible. Saisis le chemin du dossier ; sous Linux, vérifie Zenity.".into());
    }
    let path: Option<String> =
        serde_json::from_slice(&output.stdout).map_err(|_| "Réponse du sélecteur invalide.")?;
    path.map(|path| selected(&path))
        .transpose()
        .map(Option::flatten)
}

fn transfer_prompt(source: &Path, destination: &Path) -> Result<String> {
    let source = source.to_str().ok_or("Chemin source Unicode requis.")?;
    let destination = destination
        .to_str()
        .ok_or("Chemin de destination Unicode requis.")?;
    if selected(source)?.is_none() || selected(destination)?.is_none() {
        return Err("Les deux chemins du transfert doivent être absolus et non vides.".into());
    }
    Ok(format!(
        "Copier les données Rust / Copy Rust data\n\nSource : {source}\nDestination : {destination}\n\nLa source sera conservée. La destination doit être vide. Les dépôts locaux devront être réautorisés.\nThe source is preserved. The destination must be empty. Local repositories must be authorized again."
    ))
}

pub async fn confirm_transfer(source: &Path, destination: &Path) -> Result<bool> {
    transfer_prompt(source, destination)?;
    let output = helper("confirm-import", &[source, destination]).await?;
    if !output.status.success() {
        return Err("Confirmation graphique indisponible ; aucune copie effectuée.".into());
    }
    serde_json::from_slice(&output.stdout).map_err(|_| "Réponse de confirmation invalide.".into())
}

pub async fn confirmation_command(args: impl Iterator<Item = std::ffi::OsString>) -> Result<()> {
    let mut args = args;
    let source = args
        .next()
        .map(std::path::PathBuf::from)
        .ok_or("Source de confirmation absente.")?;
    let destination = args
        .next()
        .map(std::path::PathBuf::from)
        .ok_or("Destination de confirmation absente.")?;
    if args.next().is_some() {
        return Err("Arguments de confirmation inattendus.".into());
    }
    let prompt = transfer_prompt(&source, &destination)?;
    #[cfg(windows)]
    let confirmed = tokio::task::spawn_blocking(move || {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            IDCANCEL, IDNO, IDYES, MB_DEFBUTTON2, MB_ICONQUESTION, MB_SETFOREGROUND,
            MB_YESNOCANCEL, MessageBoxW,
        };
        let prompt: Vec<u16> = prompt.encode_utf16().chain(Some(0)).collect();
        // The helper owns this synchronous dialog; default No keeps keyboard cancellation safe.
        match unsafe {
            MessageBoxW(
                std::ptr::null_mut(),
                prompt.as_ptr(),
                windows_sys::core::w!("Atelier Kanban — Import"),
                MB_YESNOCANCEL | MB_ICONQUESTION | MB_DEFBUTTON2 | MB_SETFOREGROUND,
            )
        } {
            IDYES => Ok(true),
            IDNO | IDCANCEL => Ok(false),
            _ => Err(crate::Error::from("Confirmation graphique indisponible.")),
        }
    })
    .await??;
    #[cfg(unix)]
    let confirmed = {
        #[cfg(target_os = "macos")]
        let mut command = {
            let mut command = Command::new("/usr/bin/osascript");
            command.args(["-e", "on run argv\ntry\ndisplay dialog (item 1 of argv) buttons {\"Annuler / Cancel\", \"Copier / Copy\"} default button 1 cancel button 1 with title \"Atelier Kanban\" with icon caution\nreturn true\non error number -128\nreturn false\nend try\nend run", "--", &prompt]);
            command
        };
        #[cfg(not(target_os = "macos"))]
        let mut command = {
            let mut command = Command::new("/usr/bin/zenity");
            command.args([
                "--question",
                "--default-cancel",
                "--no-wrap",
                "--title=Atelier Kanban",
                "--ok-label=Copier / Copy",
                "--cancel-label=Annuler / Cancel",
                "--text",
                &prompt,
            ]);
            command
        };
        environment(&mut command);
        let output =
            super::process_tree::capture(&mut command, &[], Duration::from_secs(290), 1024).await?;
        #[cfg(target_os = "macos")]
        {
            if !output.status.success() {
                return Err("Confirmation graphique indisponible.".into());
            }
            match output.stdout.as_slice() {
                b"true\n" => true,
                b"false\n" => false,
                _ => return Err("Réponse graphique inattendue.".into()),
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            match output.status.code() {
                Some(0) => true,
                Some(1) => false,
                _ => return Err("Confirmation graphique indisponible.".into()),
            }
        }
    };
    use std::io::Write;
    std::io::stdout().write_all(&serde_json::to_vec(&confirmed)?)?;
    Ok(())
}
pub async fn command() -> Result<()> {
    #[cfg(windows)]
    let value = tokio::task::spawn_blocking(native).await??;
    #[cfg(unix)]
    let value = native().await?;
    use std::io::Write;
    std::io::stdout().write_all(&serde_json::to_vec(&value)?)?;
    Ok(())
}
#[cfg(windows)]
fn native() -> Result<Option<String>> {
    use windows_sys::Win32::{
        System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoTaskMemFree, CoUninitialize},
        UI::Shell::{
            BFFM_INITIALIZED, BIF_EDITBOX, BIF_NEWDIALOGSTYLE, BIF_NONEWFOLDERBUTTON,
            BIF_RETURNONLYFSDIRS, BROWSEINFOW, SHBrowseForFolderW, SHGetPathFromIDListEx,
        },
    };
    unsafe extern "system" fn focus(
        window: windows_sys::Win32::Foundation::HWND,
        message: u32,
        _: isize,
        _: isize,
    ) -> i32 {
        if message == BFFM_INITIALIZED {
            // The shell invokes this synchronously with its live dialog window.
            unsafe {
                windows_sys::Win32::UI::WindowsAndMessaging::BringWindowToTop(window);
                windows_sys::Win32::UI::WindowsAndMessaging::SetForegroundWindow(window);
            }
        }
        0
    }
    // This dedicated helper thread owns its STA until the modal dialog and PIDL are released.
    if unsafe { CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32) } < 0 {
        return Err("Initialisation du sélecteur impossible.".into());
    }
    struct Apartment;
    impl Drop for Apartment {
        fn drop(&mut self) {
            unsafe { CoUninitialize() }
        }
    }
    let _apartment = Apartment;
    let title: Vec<u16> = "Choisir un dossier / Choose a folder\0"
        .encode_utf16()
        .collect();
    let mut display = vec![0u16; 32768];
    let info = BROWSEINFOW {
        pszDisplayName: display.as_mut_ptr(),
        lpszTitle: title.as_ptr(),
        ulFlags: BIF_RETURNONLYFSDIRS | BIF_NEWDIALOGSTYLE | BIF_EDITBOX | BIF_NONEWFOLDERBUTTON,
        lpfn: Some(focus),
        ..Default::default()
    };
    // All buffers outlive the synchronous shell call; the returned PIDL uses the COM allocator.
    let pidl = unsafe { SHBrowseForFolderW(&info) };
    if pidl.is_null() {
        return Ok(None);
    }
    let mut path = vec![0u16; 32768];
    let success = unsafe { SHGetPathFromIDListEx(pidl, path.as_mut_ptr(), path.len() as u32, 0) };
    unsafe { CoTaskMemFree(pidl.cast()) };
    if success == 0 {
        return Err("Le sélecteur a retourné un chemin invalide.".into());
    }
    let length = path
        .iter()
        .position(|unit| *unit == 0)
        .ok_or("Chemin de dossier trop long.")?;
    selected(&String::from_utf16(&path[..length]).map_err(|_| "Chemin du dossier non Unicode.")?)
}
#[cfg(unix)]
async fn native() -> Result<Option<String>> {
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = Command::new("/usr/bin/osascript");
        command.args([
            "-e",
            "try",
            "-e",
            "POSIX path of (choose folder with prompt \"Choisir un dossier / Choose a folder\")",
            "-e",
            "on error number -128",
            "-e",
            "return \"\"",
            "-e",
            "end try",
        ]);
        command
    };
    #[cfg(not(target_os = "macos"))]
    let mut command = {
        let mut command = Command::new("/usr/bin/zenity");
        command.args([
            "--file-selection",
            "--directory",
            "--title=Choisir un dossier / Choose a folder",
        ]);
        command
    };
    environment(&mut command);
    let output =
        super::process_tree::capture(&mut command, &[], Duration::from_secs(290), 64 * 1024)
            .await?;
    if !output.status.success() {
        if cfg!(target_os = "linux") && output.status.code() == Some(1) && output.stdout.is_empty()
        {
            return Ok(None);
        }
        return Err("Sélecteur de dossiers indisponible.".into());
    }
    let path = String::from_utf8(output.stdout)?;
    selected(path.strip_suffix('\n').unwrap_or(&path))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn transfer_confirmation_names_both_exact_paths_and_rejects_empty_or_control_paths() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("synthetic source");
        let destination = root.path().join("synthetic destination");
        let prompt = transfer_prompt(&source, &destination).unwrap();
        assert!(prompt.contains(source.to_str().unwrap()));
        assert!(prompt.contains(destination.to_str().unwrap()));
        assert!(transfer_prompt(Path::new(""), &destination).is_err());
        assert!(transfer_prompt(Path::new("relative"), &destination).is_err());
        assert!(transfer_prompt(&source, &root.path().join("control\npath")).is_err());
        assert!(std::fs::read_dir(root.path()).unwrap().next().is_none());
    }
    #[test]
    fn cancellation_and_invalid_native_output_never_authorize_a_path() {
        assert!(selected("").unwrap().is_none());
        for value in [
            "relative",
            "../outside",
            "/synthetic\nfolder",
            "/synthetic\0folder",
        ] {
            assert!(selected(value).is_err());
        }
        let root = if cfg!(windows) {
            "C:\\synthetic folder"
        } else {
            "/synthetic folder"
        };
        assert_eq!(selected(root).unwrap().as_deref(), Some(root));
    }
}
