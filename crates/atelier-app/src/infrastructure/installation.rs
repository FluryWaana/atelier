use super::checked_file::CheckedFile;
use crate::Result;
use cap_std::{ambient_authority, fs::Dir};
use serde_json::Value;
use std::path::{Path, PathBuf};

#[cfg(windows)]
#[path = "windows_registration.rs"]
mod windows_registration;

pub fn update_installed_version(root: &Path, version: &str) -> Result<()> {
    #[cfg(windows)]
    if mode(root)? == InstallationMode::User {
        windows_registration::update(root, version)?;
    }
    #[cfg(not(windows))]
    let _ = (root, version);
    Ok(())
}

#[derive(Debug, PartialEq)]
pub enum InstallationMode {
    Portable,
    User,
    System,
}

pub enum Platform {
    Windows,
    Linux,
    Macos,
}

pub fn mode(root: &Path) -> Result<InstallationMode> {
    let directory = match Dir::open_ambient_dir(root, ambient_authority()) {
        Ok(directory) => directory,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(InstallationMode::Portable);
        }
        Err(error) => return Err(error.into()),
    };
    match directory.symlink_metadata("installation.json") {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(InstallationMode::Portable);
        }
        Err(error) => return Err(error.into()),
        Ok(_) => {}
    }
    let value: Value = serde_json::from_slice(
        &CheckedFile::open_in(&directory, Path::new("installation.json"))?.read(65_536)?,
    )?;
    match value.get("mode").and_then(Value::as_str) {
        Some("user") => Ok(InstallationMode::User),
        Some("system") => Ok(InstallationMode::System),
        _ => Err("Mode d’installation invalide.".into()),
    }
}

pub fn data_directory(
    root: &Path,
    platform: Platform,
    home: &Path,
    local_app_data: Option<&Path>,
    xdg_data_home: Option<&Path>,
) -> Result<PathBuf> {
    if mode(root)? == InstallationMode::Portable {
        return Ok(root.join("data"));
    }
    match platform {
        Platform::Windows => Ok(local_app_data
            .filter(|path| !path.as_os_str().is_empty())
            .map(Path::to_owned)
            .unwrap_or_else(|| home.join("AppData/Local"))
            .join("AtelierKanban/data")),
        Platform::Linux => Ok(xdg_data_home
            .filter(|path| path.is_absolute())
            .map(Path::to_owned)
            .unwrap_or_else(|| home.join(".local/share"))
            .join("atelier-kanban")),
        Platform::Macos => Err("Installation native non prise en charge sur ce système.".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn portable_user_and_system_paths_preserve_platform_rules() {
        let root = tempfile::tempdir().unwrap();
        let root = root.path();
        let home = root.join("home");
        assert_eq!(mode(root).unwrap(), InstallationMode::Portable);
        assert_eq!(
            data_directory(root, Platform::Macos, &home, None, None).unwrap(),
            root.join("data")
        );
        fs::write(root.join("installation.json"), r#"{"mode":"user"}"#).unwrap();
        assert_eq!(
            data_directory(root, Platform::Windows, &home, None, None).unwrap(),
            home.join("AppData/Local/AtelierKanban/data")
        );
        assert_eq!(
            data_directory(root, Platform::Windows, &home, Some(&home), None).unwrap(),
            home.join("AtelierKanban/data")
        );
        assert_eq!(
            data_directory(
                root,
                Platform::Linux,
                &home,
                None,
                Some(Path::new("relative"))
            )
            .unwrap(),
            home.join(".local/share/atelier-kanban")
        );
        assert_eq!(
            data_directory(root, Platform::Linux, &home, None, Some(&home)).unwrap(),
            home.join("atelier-kanban")
        );
        assert!(data_directory(root, Platform::Macos, &home, None, None).is_err());
        fs::write(root.join("installation.json"), r#"{"mode":"system"}"#).unwrap();
        assert_eq!(mode(root).unwrap(), InstallationMode::System);
        for invalid in [r#"{"mode":"portable"}"#, "invalid", "null", "{}"] {
            fs::write(root.join("installation.json"), invalid).unwrap();
            assert!(mode(root).is_err());
        }
    }
}
