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
const FILE: &str = ".atelier-repositories.json";
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
                return Err("Trop de dépôts autorisés.".into());
            }
            let mut seen = std::collections::BTreeSet::new();
            for entry in &registry.repositories {
                if !seen.insert(entry.path.clone())
                    || entry.path.len() > 8000
                    || entry.identity.is_empty()
                {
                    return Err("Registre des dépôts invalide.".into());
                }
            }
            Ok(registry)
        }
    }
}
fn absolute(path: &Path) -> Result<Dir> {
    let mut components = path.components();
    let mut anchor = PathBuf::new();
    #[cfg(windows)]
    match components.next() {
        Some(Component::Prefix(prefix)) if matches!(prefix.kind(), std::path::Prefix::Disk(_)) => {
            anchor.push(prefix.as_os_str())
        }
        _ => return Err("Un chemin absolu de disque local est requis.".into()),
    }
    match components.next() {
        Some(Component::RootDir) => anchor.push(std::path::MAIN_SEPARATOR.to_string()),
        _ => return Err("Un chemin absolu de dépôt est requis.".into()),
    }
    let mut directory = Dir::open_ambient_dir(anchor, ambient_authority())?;
    let mut depth = 0;
    for component in components {
        let Component::Normal(name) = component else {
            return Err("Les traversées sont interdites dans le chemin du dépôt.".into());
        };
        depth += 1;
        if depth > 100 {
            return Err("Chemin du dépôt trop profond.".into());
        }
        require_plain(&directory.symlink_metadata(name)?)?;
        directory = directory.open_dir_nofollow(name)?;
    }
    if depth == 0 {
        return Err("Choisis un sous-dossier de dépôt.".into());
    }
    Ok(directory)
}
fn identity(directory: &Dir) -> Result<String> {
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
    let selected = path_string(path).utf8()?;
    let registry = read(root)?;
    let entry = registry
        .repositories
        .iter()
        .find(|entry| {
            atelier_core::domain::projects::same_repository(&entry.path, &selected, cfg!(windows))
        })
        .ok_or("Autorise explicitement ce dépôt dans les paramètres avant de l’utiliser.")?;
    let directory = absolute(Path::new(&entry.path))?;
    if identity(&directory)? != entry.identity {
        return Err(
            "Le dossier autorisé a été remplacé. Vérifie-le puis autorise-le à nouveau.".into(),
        );
    }
    Ok(directory)
}
pub fn register(root: &Dir, path: &str, expected: u64, confirmed: bool) -> Result<Registry> {
    if !confirmed || path.encode_utf16().count() > 2000 || path.chars().any(char::is_control) {
        return Err("Confirme l’accès à ce dossier de dépôt.".into());
    }
    let mut registry = read(root)?;
    if registry.revision != expected {
        return Err("Les autorisations de dépôts ont changé. Recharge la liste.".into());
    }
    let normalized = path_string(Path::new(path)).utf8()?;
    let directory = absolute(Path::new(&normalized))?;
    let metadata = directory
        .symlink_metadata(".git")
        .map_err(|_| "Ce dossier ne contient pas de dépôt Git principal.")?;
    require_plain(&metadata)?;
    if !metadata.is_dir() {
        return Err("Autorise le dépôt principal, pas un worktree externe.".into());
    }
    let git = directory.open_dir_nofollow(".git")?;
    let _ = git.dir_metadata()?;
    let selected = path_string(Path::new(path)).utf8()?;
    let entry = Registration {
        path: selected,
        identity: identity(&directory)?,
    };
    if let Some(previous) = registry.repositories.iter_mut().find(|previous| {
        atelier_core::domain::projects::same_repository(&previous.path, &entry.path, cfg!(windows))
    }) {
        *previous = entry;
    } else {
        if registry.repositories.len() >= 100 {
            return Err("Trop de dépôts autorisés.".into());
        }
        registry.repositories.push(entry);
    }
    registry.revision = registry
        .revision
        .checked_add(1)
        .ok_or("Révision des dépôts épuisée.")?;
    let bytes = serde_json::to_vec(&registry)?;
    if bytes.len() > 256 * 1024 {
        return Err("Registre des dépôts trop volumineux.".into());
    }
    publish::write_private(root, Path::new(FILE), &bytes, false)?;
    Ok(registry)
}
