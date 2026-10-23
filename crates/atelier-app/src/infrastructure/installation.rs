use super::checked_file::{CheckedFile, Identity};
use crate::Result;
use serde_json::Value;
use std::path::{Path, PathBuf};

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
    let path = root.join("installation.json");
    match std::fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(InstallationMode::Portable);
        }
        Err(error) => return Err(error.into()),
        Ok(_) => {}
    }
    let identity = Identity::inspect(&path)?;
    let value: Value = serde_json::from_slice(&CheckedFile::open(&path, &identity)?.read(65_536)?)?;
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
