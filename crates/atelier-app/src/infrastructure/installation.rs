use super::checked_file::{CheckedFile, require_plain};
use crate::Result;
use cap_fs_ext::DirExt;
use cap_std::{ambient_authority, fs::Dir};
use serde_json::Value;
use std::path::{Component, Path, PathBuf};

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

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
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
        Some("portable") if value["format"] == "atelier-rust-installation/1" => {
            Ok(InstallationMode::Portable)
        }
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
            .filter(|path| path.is_absolute())
            .map(Path::to_owned)
            .unwrap_or_else(|| home.join("AppData/Local"))
            .join("AtelierKanban/data")),
        Platform::Linux => Ok(xdg_data_home
            .filter(|path| path.is_absolute())
            .map(Path::to_owned)
            .unwrap_or_else(|| home.join(".local/share"))
            .join("atelier-kanban")),
        Platform::Macos => Ok(home.join("Library/Application Support/AtelierKanban/data")),
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum DataLocation {
    Ready(PathBuf),
    ConfirmFresh { previous: PathBuf, fresh: PathBuf },
}

enum DirectoryState {
    Empty,
    Rust,
    Foreign,
}

fn directory_state(path: &Path) -> Result<DirectoryState> {
    if !path.is_absolute()
        || path.components().count() > 100
        || path
            .components()
            .any(|part| matches!(part, Component::CurDir | Component::ParentDir))
    {
        return Err("Chemin de données absolu sans traversée requis.".into());
    }
    let anchor = path
        .ancestors()
        .last()
        .ok_or("Racine de données absente.")?;
    let mut directory = Dir::open_ambient_dir(anchor, ambient_authority())?;
    for component in path.components() {
        if let Component::Normal(name) = component {
            match directory.symlink_metadata(name) {
                Ok(metadata) => require_plain(&metadata)?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    return Ok(DirectoryState::Empty);
                }
                Err(error) => return Err(error.into()),
            }
            directory = directory.open_dir_nofollow(name)?;
        }
    }
    #[cfg(unix)]
    {
        use cap_std::fs::MetadataExt;
        // geteuid has no arguments or memory safety preconditions.
        if directory.dir_metadata()?.uid() != unsafe { libc::geteuid() } {
            return Err("Le dossier de données doit appartenir au compte courant.".into());
        }
    }
    let marker = Path::new(".atelier-rust-prototype");
    match directory.symlink_metadata(marker) {
        Ok(metadata) => {
            require_plain(&metadata)?;
            CheckedFile::open_in(&directory, marker)?.read(0)?;
            Ok(DirectoryState::Rust)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(if directory.entries()?.next().transpose()?.is_some() {
                DirectoryState::Foreign
            } else {
                DirectoryState::Empty
            })
        }
        Err(error) => Err(error.into()),
    }
}

pub fn default_data_location(
    root: &Path,
    platform: Platform,
    home: &Path,
    local_app_data: Option<&Path>,
    xdg_data_home: Option<&Path>,
) -> Result<DataLocation> {
    if mode(root)? == InstallationMode::Portable {
        return Ok(DataLocation::Ready(root.join("data")));
    }
    let fresh_name = match &platform {
        Platform::Linux => "atelier-kanban-workspace",
        Platform::Windows | Platform::Macos => "workspace",
    };
    let previous = data_directory(root, platform, home, local_app_data, xdg_data_home)?;
    let fresh = previous
        .parent()
        .ok_or("Parent du dossier de données absent.")?
        .join(fresh_name);
    match directory_state(&fresh)? {
        DirectoryState::Rust => return Ok(DataLocation::Ready(fresh)),
        DirectoryState::Foreign => {
            return Err("Le nouvel espace contient déjà des données non reconnues. Aucun fichier n’est remplacé.".into());
        }
        DirectoryState::Empty => {}
    }
    Ok(match directory_state(&previous)? {
        DirectoryState::Rust => DataLocation::Ready(previous),
        DirectoryState::Empty => DataLocation::Ready(fresh),
        DirectoryState::Foreign => DataLocation::ConfirmFresh { previous, fresh },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn default_resolution_preserves_legacy_data_and_never_creates_a_workspace() {
        let temporary = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(temporary.path()).unwrap();
        fs::write(root.join("installation.json"), r#"{"mode":"user"}"#).unwrap();
        for (platform, previous, fresh) in [
            (
                Platform::Windows,
                "AppData/Local/AtelierKanban/data",
                "AppData/Local/AtelierKanban/workspace",
            ),
            (
                Platform::Macos,
                "Library/Application Support/AtelierKanban/data",
                "Library/Application Support/AtelierKanban/workspace",
            ),
            (
                Platform::Linux,
                ".local/share/atelier-kanban",
                ".local/share/atelier-kanban-workspace",
            ),
        ] {
            let home = root.join(previous.replace('/', "-"));
            let previous = home.join(previous);
            let fresh = home.join(fresh);
            let resolve = || {
                let platform = match &platform {
                    Platform::Windows => Platform::Windows,
                    Platform::Macos => Platform::Macos,
                    Platform::Linux => Platform::Linux,
                };
                default_data_location(&root, platform, &home, None, None)
            };
            assert_eq!(resolve().unwrap(), DataLocation::Ready(fresh.clone()));
            assert!(!home.exists());
            fs::create_dir_all(&previous).unwrap();
            assert_eq!(resolve().unwrap(), DataLocation::Ready(fresh.clone()));
            fs::write(previous.join("human-note"), b"preserve previous data").unwrap();
            assert_eq!(
                resolve().unwrap(),
                DataLocation::ConfirmFresh {
                    previous: previous.clone(),
                    fresh: fresh.clone()
                }
            );
            assert_eq!(
                fs::read(previous.join("human-note")).unwrap(),
                b"preserve previous data"
            );
            assert_eq!(fs::read_dir(&previous).unwrap().count(), 1);
            assert!(!fresh.exists());
            fs::write(previous.join(".atelier-rust-prototype"), []).unwrap();
            assert_eq!(resolve().unwrap(), DataLocation::Ready(previous.clone()));
            fs::create_dir(&fresh).unwrap();
            assert_eq!(resolve().unwrap(), DataLocation::Ready(previous.clone()));
            fs::write(fresh.join("human-note"), b"preserve fresh data").unwrap();
            assert!(resolve().is_err());
            assert_eq!(
                fs::read(fresh.join("human-note")).unwrap(),
                b"preserve fresh data"
            );
            fs::write(fresh.join(".atelier-rust-prototype"), []).unwrap();
            assert_eq!(resolve().unwrap(), DataLocation::Ready(fresh));
        }
    }

    #[test]
    fn default_resolution_honors_explicit_platform_roots_and_portable_mode() {
        let temporary = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(temporary.path()).unwrap();
        let home = root.join("home");
        assert_eq!(
            default_data_location(&root, Platform::Windows, &home, None, None).unwrap(),
            DataLocation::Ready(root.join("data"))
        );
        assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
        fs::write(root.join("installation.json"), r#"{"mode":"user"}"#).unwrap();
        let configured = root.join("configured");
        assert_eq!(
            default_data_location(&root, Platform::Windows, &home, Some(&configured), None)
                .unwrap(),
            DataLocation::Ready(configured.join("AtelierKanban/workspace"))
        );
        assert_eq!(
            default_data_location(&root, Platform::Linux, &home, None, Some(&configured)).unwrap(),
            DataLocation::Ready(configured.join("atelier-kanban-workspace"))
        );
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
    }

    #[test]
    fn malformed_markers_and_non_directories_fail_without_falling_back() {
        let temporary = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(temporary.path()).unwrap();
        fs::write(root.join("installation.json"), r#"{"mode":"user"}"#).unwrap();
        for name in ["atelier-kanban", "atelier-kanban-workspace"] {
            let base = root.join(name);
            fs::create_dir(&base).unwrap();
            let selected = base.join(name);
            let resolve =
                || default_data_location(&root, Platform::Linux, &root, None, Some(&base));
            fs::write(&selected, b"human file, not a directory").unwrap();
            assert!(resolve().is_err());
            assert_eq!(fs::read(&selected).unwrap(), b"human file, not a directory");
            fs::remove_file(&selected).unwrap();
            fs::create_dir(&selected).unwrap();
            let marker = selected.join(".atelier-rust-prototype");
            fs::write(&marker, b"invalid marker").unwrap();
            assert!(resolve().is_err());
            assert_eq!(fs::read(&marker).unwrap(), b"invalid marker");
            fs::remove_file(&marker).unwrap();
            fs::create_dir(&marker).unwrap();
            assert!(resolve().is_err());
            assert!(marker.is_dir());
            assert_eq!(fs::read_dir(&base).unwrap().count(), 1);
        }
    }

    #[cfg(unix)]
    #[test]
    fn linked_directories_parents_and_markers_are_never_followed() {
        use std::os::unix::fs::symlink;
        let temporary = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(temporary.path()).unwrap();
        fs::write(root.join("installation.json"), r#"{"mode":"user"}"#).unwrap();
        let outside = root.join("outside");
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("human-note"), b"preserve outside").unwrap();
        fs::write(outside.join(".atelier-rust-prototype"), []).unwrap();
        for name in ["atelier-kanban", "atelier-kanban-workspace"] {
            let base = root.join(name);
            fs::create_dir(&base).unwrap();
            let selected = base.join(name);
            let resolve =
                || default_data_location(&root, Platform::Linux, &root, None, Some(&base));
            symlink(&outside, &selected).unwrap();
            assert!(resolve().is_err());
            fs::remove_file(&selected).unwrap();
            fs::create_dir(&selected).unwrap();
            symlink(
                outside.join(".atelier-rust-prototype"),
                selected.join(".atelier-rust-prototype"),
            )
            .unwrap();
            assert!(resolve().is_err());
        }
        let linked_parent = root.join("linked-parent");
        symlink(&outside, &linked_parent).unwrap();
        assert!(
            default_data_location(&root, Platform::Linux, &root, None, Some(&linked_parent))
                .is_err()
        );
        assert_eq!(fs::read_dir(&outside).unwrap().count(), 2);
        assert_eq!(
            fs::read(outside.join("human-note")).unwrap(),
            b"preserve outside"
        );
    }

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
        assert_eq!(
            data_directory(root, Platform::Macos, &home, None, None).unwrap(),
            home.join("Library/Application Support/AtelierKanban/data")
        );
        fs::write(root.join("installation.json"), r#"{"mode":"system"}"#).unwrap();
        assert_eq!(mode(root).unwrap(), InstallationMode::System);
        for invalid in [r#"{"mode":"portable"}"#, "invalid", "null", "{}"] {
            fs::write(root.join("installation.json"), invalid).unwrap();
            assert!(mode(root).is_err());
        }
    }
}
