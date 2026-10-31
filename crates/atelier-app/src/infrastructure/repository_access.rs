use super::{
    checked_file::{CheckedFile, require_plain},
    publish,
    task_creation::path_string,
};
use crate::Result;
use cap_fs_ext::DirExt;
use cap_std::{ambient_authority, fs::Dir};
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};
pub(super) const FILE: &str = ".atelier-repositories.json";
#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Registry {
    pub revision: u64,
    pub repositories: Vec<Registration>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Registration {
    pub path: String,
    identity: String,
}
pub fn read(root: &Dir) -> Result<Registry> {
    match root.symlink_metadata(FILE) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Registry::default()),
        Err(error) => Err(error.into()),
        Ok(_) => {
            let registry: Registry = serde_json::from_slice(
                &CheckedFile::open_in(root, Path::new(FILE))?
                    .private()?
                    .read(256 * 1024)?,
            )?;
            if registry.repositories.len() > 100 {
                return Err("message.too_many_authorized_repositories".into());
            }
            let mut seen = std::collections::BTreeSet::new();
            for entry in &registry.repositories {
                if !seen.insert(entry.path.clone())
                    || entry.path.len() > 8000
                    || entry.identity.is_empty()
                {
                    return Err("message.invalid_repository_registry".into());
                }
            }
            Ok(registry)
        }
    }
}
pub(super) fn absolute(path: &Path) -> Result<Dir> {
    let mut components = path.components();
    let mut anchor = PathBuf::new();
    #[cfg(windows)]
    match components.next() {
        Some(Component::Prefix(prefix)) if matches!(prefix.kind(), std::path::Prefix::Disk(_)) => {
            anchor.push(prefix.as_os_str())
        }
        _ => return Err("message.an_absolute_local_drive_path_is_required".into()),
    }
    match components.next() {
        Some(Component::RootDir) => anchor.push(std::path::MAIN_SEPARATOR.to_string()),
        _ => return Err("message.an_absolute_repository_path_is_required".into()),
    }
    let mut directory = Dir::open_ambient_dir(anchor, ambient_authority())?;
    let mut depth = 0;
    for component in components {
        let Component::Normal(name) = component else {
            return Err("message.parent_traversal_is_forbidden_in_repository_paths".into());
        };
        depth += 1;
        if depth > 100 {
            return Err("message.the_repository_path_is_too_deep".into());
        }
        require_plain(&directory.symlink_metadata(name)?)?;
        directory = directory.open_dir_nofollow(name)?;
    }
    if depth == 0 {
        return Err("message.choose_a_repository_subdirectory".into());
    }
    Ok(directory)
}
pub(super) fn identity(directory: &Dir) -> Result<String> {
    #[cfg(unix)]
    {
        use cap_std::fs::MetadataExt;
        let metadata = directory.dir_metadata()?;
        Ok(format!("{}:{}", metadata.dev(), metadata.ino()))
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_ID_INFO, FileIdInfo, GetFileInformationByHandleEx,
        };
        let mut info = FILE_ID_INFO::default();
        // The directory owns this live handle while Windows fills the fixed-size identity.
        if unsafe {
            GetFileInformationByHandleEx(
                directory.as_raw_handle(),
                FileIdInfo,
                (&mut info as *mut FILE_ID_INFO).cast(),
                std::mem::size_of_val(&info) as u32,
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(format!(
            "{}:{:02x?}",
            info.VolumeSerialNumber, info.FileId.Identifier
        ))
    }
}
pub fn open(root: &Dir, path: &Path) -> Result<Dir> {
    let selected = path_string(path);
    let registry = read(root)?;
    let entry = registry
        .repositories
        .iter()
        .find(|entry| {
            atelier_core::domain::projects::same_repository(&entry.path, &selected, cfg!(windows))
        })
        .ok_or("message.explicitly_authorize_this_repository_in_settings_before_using_it")?;
    let directory = absolute(Path::new(&entry.path))?;
    if identity(&directory)? != entry.identity {
        return Err(
            "message.the_authorized_directory_was_replaced_check_it_then_authorize_it".into(),
        );
    }
    Ok(directory)
}
pub fn register(root: &Dir, path: &str, expected: u64, confirmed: bool) -> Result<Registry> {
    register_checked(root, path, expected, confirmed, None)
}

pub(super) fn register_checked(
    root: &Dir,
    path: &str,
    expected: u64,
    confirmed: bool,
    expected_identity: Option<&str>,
) -> Result<Registry> {
    if !confirmed || path.encode_utf16().count() > 2000 || path.chars().any(char::is_control) {
        return Err("message.confirm_access_to_this_repository_directory".into());
    }
    let mut registry = read(root)?;
    if registry.revision != expected {
        return Err("message.repository_authorizations_changed_reload_the_list".into());
    }
    let normalized = path_string(Path::new(path));
    let directory = absolute(Path::new(&normalized))?;
    let identity = identity(&directory)?;
    if expected_identity.is_some_and(|expected| expected != identity) {
        return Err("message.clone_destination_changed".into());
    }
    let metadata = directory
        .symlink_metadata(".git")
        .map_err(|_| "message.this_directory_does_not_contain_a_main_git_repository")?;
    require_plain(&metadata)?;
    if !metadata.is_dir() {
        return Err(
            "message.authorize_the_main_repository_rather_than_an_external_worktree".into(),
        );
    }
    let git = directory.open_dir_nofollow(".git")?;
    let _ = git.dir_metadata()?;
    let selected = path_string(Path::new(path));
    let entry = Registration {
        path: selected,
        identity,
    };
    if let Some(previous) = registry.repositories.iter_mut().find(|previous| {
        atelier_core::domain::projects::same_repository(&previous.path, &entry.path, cfg!(windows))
    }) {
        *previous = entry;
    } else {
        if registry.repositories.len() >= 100 {
            return Err("message.too_many_authorized_repositories".into());
        }
        registry.repositories.push(entry);
    }
    registry.revision = registry
        .revision
        .checked_add(1)
        .ok_or("message.repository_revision_exhausted")?;
    let bytes = serde_json::to_vec(&registry)?;
    if bytes.len() > 256 * 1024 {
        return Err("message.the_repository_registry_is_too_large".into());
    }
    publish::write_private(root, Path::new(FILE), &bytes, false)?;
    Ok(registry)
}

pub fn revoke(
    root: &Dir,
    path: &str,
    expected: u64,
    confirmed: bool,
    in_use: bool,
) -> Result<Registry> {
    let selected = path_string(Path::new(path));
    if !confirmed {
        return Err("message.confirm_removal_of_this_local_authorization".into());
    }
    if in_use {
        return Err(
            "message.this_repository_is_still_referenced_by_settings_projects_or_task".into(),
        );
    }
    let mut registry = read(root)?;
    if registry.revision != expected {
        return Err("message.repository_authorizations_changed_reload_the_list".into());
    }
    let position = registry
        .repositories
        .iter()
        .position(|entry| {
            atelier_core::domain::projects::same_repository(&entry.path, &selected, cfg!(windows))
        })
        .ok_or("message.repository_authorization_not_found")?;
    registry.repositories.remove(position);
    registry.revision = registry
        .revision
        .checked_add(1)
        .ok_or("message.repository_revision_exhausted")?;
    publish::write_private(
        root,
        Path::new(FILE),
        &serde_json::to_vec(&registry)?,
        false,
    )?;
    Ok(registry)
}

pub fn references(value: &serde_json::Value, path: &str) -> bool {
    match value {
        serde_json::Value::String(value) => {
            atelier_core::domain::projects::same_repository(value, path, cfg!(windows))
        }
        serde_json::Value::Array(values) => values.iter().any(|value| references(value, path)),
        serde_json::Value::Object(values) => values.values().any(|value| references(value, path)),
        _ => false,
    }
}
