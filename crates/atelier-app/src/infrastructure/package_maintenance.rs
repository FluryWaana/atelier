use crate::Result;
use std::path::Path;

pub fn recover_command(args: impl IntoIterator<Item = std::ffi::OsString>) -> Result<()> {
    let mut args = args.into_iter();
    if args.next().as_deref() != Some(std::ffi::OsStr::new("--root")) {
        return Err("Indique --root DOSSIER_INSTALLATION_ABSOLU --confirm.".into());
    }
    let root = args
        .next()
        .map(std::path::PathBuf::from)
        .ok_or("Installation absente.")?;
    if !root.is_absolute()
        || root
            .to_str()
            .is_none_or(|root| root.len() > 32_000 || root.chars().any(char::is_control))
        || args.next().as_deref() != Some(std::ffi::OsStr::new("--confirm"))
        || args.next().is_some()
    {
        return Err("Confirme explicitement le dossier absolu de maintenance à récupérer.".into());
    }
    #[cfg(target_os = "linux")]
    {
        let receipt = recover(&root, || {
            super::installed::verify_all(&root, super::package::native_target()?)
        })?;
        println!(
            "Maintenance récupérée ; ancien marqueur conservé : {}",
            receipt.display()
        );
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    Err("La récupération de marqueur de paquet concerne uniquement les paquets Linux.".into())
}

#[cfg(target_os = "linux")]
fn recover(root: &Path, verify: impl FnOnce() -> Result<()>) -> Result<std::path::PathBuf> {
    use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt, OpenOptionsSyncExt};
    use cap_std::fs::MetadataExt;
    use std::{io::Write, os::fd::AsRawFd};
    const MARKER: &str = ".package-maintenance";
    const CONTENT: &[u8] = b"atelier-package-maintenance/1\n";
    let directory = super::repository_access::absolute(root)?;
    let original = directory.dir_metadata()?;
    let mut options = cap_std::fs::OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No).nonblock(true);
    let lock = directory.open_with(".package-lock", &options)?.into_std();
    super::checked_file::CheckedFile::from_file(lock.try_clone()?)?.single_link()?;
    if lock.metadata()?.len() != 0 {
        return Err("Verrou de paquet modifié ; contenu conservé.".into());
    }
    // Match the shared lock held by every Linux packaged server, without stopping any process.
    if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err("Arrête toutes les instances et opérations du gestionnaire de paquets avant la récupération.".into());
    }
    let marker =
        super::checked_file::CheckedFile::open_in(&directory, Path::new(MARKER))?.single_link()?;
    if marker.open_same(&root.join(MARKER))?.read(64)? != CONTENT {
        return Err("Marqueur de maintenance étranger ; contenu conservé pour examen.".into());
    }
    verify()?;
    let current = super::repository_access::absolute(root)?.dir_metadata()?;
    if original.dev() != current.dev() || original.ino() != current.ino() {
        return Err("Le dossier d’installation a changé ; aucune récupération appliquée.".into());
    }
    if marker.open_same(&root.join(MARKER))?.read(64)? != CONTENT {
        return Err("Le marqueur a été modifié ; récupération refusée, contenu conservé.".into());
    }
    let receipt = format!(".package-maintenance-recovered-{}", uuid::Uuid::new_v4());
    directory.rename(MARKER, &directory, &receipt)?;
    if !matches!(marker.open_same(&root.join(&receipt)).and_then(|file| file.read(64)), Ok(bytes) if bytes == CONTENT)
    {
        // Preserve the raced file and leave the installation blocked, without replacing another marker.
        let mut blocked = cap_std::fs::OpenOptions::new();
        blocked
            .write(true)
            .create_new(true)
            .follow(FollowSymlinks::No);
        match directory.open_with(MARKER, &blocked) {
            Ok(mut file) => {
                file.write_all(CONTENT)?;
                file.sync_all()?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => return Err("Le marqueur a changé et n’a pas pu être rétabli. Ne relance pas Atelier ; conserve le reçu et fais vérifier l’installation.".into()),
        }
        return Err(
            "Le marqueur a changé pendant la récupération ; installation conservée et bloquée."
                .into(),
        );
    }
    directory.try_clone()?.into_std_file().sync_all()?;
    Ok(root.join(receipt))
}

pub struct Guard {
    #[cfg(windows)]
    _running: std::os::windows::io::OwnedHandle,
    #[cfg(target_os = "linux")]
    _lock: Option<std::fs::File>,
}
pub fn acquire(root: Option<&Path>) -> Result<Guard> {
    #[cfg(windows)]
    {
        let _ = root;
        windows()
    }
    #[cfg(target_os = "linux")]
    {
        use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt, OpenOptionsSyncExt};
        use std::os::fd::AsRawFd;
        let Some(root) = root else {
            return Ok(Guard { _lock: None });
        };
        let directory = super::repository_access::absolute(root)?;
        let marker = || -> Result<()> {
            match directory.symlink_metadata(".package-maintenance") {Ok(_)=>Err("Une installation ou désinstallation est en cours. Termine sa récupération avant de lancer Atelier.".into()),Err(error) if error.kind()==std::io::ErrorKind::NotFound=>Ok(()),Err(error)=>Err(error.into())}
        };
        marker()?;
        let file = match directory.symlink_metadata(".package-lock") {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if super::installation::mode(root)? == super::installation::InstallationMode::System
                {
                    return Err("Verrou du paquet système absent ; termine l’installation avant de démarrer Atelier.".into());
                }
                return Ok(Guard { _lock: None });
            }
            Err(error) => return Err(error.into()),
            Ok(metadata) => {
                super::checked_file::require_plain(&metadata)?;
                let mut options = cap_std::fs::OpenOptions::new();
                options.read(true).follow(FollowSymlinks::No).nonblock(true);
                let file = directory.open_with(".package-lock", &options)?.into_std();
                super::checked_file::CheckedFile::from_file(file.try_clone()?)?.single_link()?;
                if !file.metadata()?.is_file() || file.metadata()?.len() != 0 {
                    return Err("Fichier de verrou du paquet invalide.".into());
                }
                file
            }
        };
        // The open file owns this advisory shared lock until the server has fully stopped.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_SH | libc::LOCK_NB) } != 0 {
            return Err("Le paquet Atelier est en cours de maintenance.".into());
        }
        marker()?;
        Ok(Guard { _lock: Some(file) })
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = root;
        Ok(Guard {})
    }
}
#[cfg(windows)]
fn windows() -> Result<Guard> {
    windows_named(
        "Global\\AtelierKanbanRustRunning",
        "Global\\AtelierKanbanRustMaintenance",
    )
}
#[cfg(windows)]
fn windows_named(running_name: &str, maintenance_name: &str) -> Result<Guard> {
    use std::{
        os::windows::io::{FromRawHandle, OwnedHandle},
        ptr,
    };
    use windows_sys::Win32::{
        Foundation::{ERROR_FILE_NOT_FOUND, GetLastError, LocalFree},
        Security::{
            Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW,
            SECURITY_ATTRIBUTES,
        },
        System::Threading::{CreateMutexW, OpenMutexW, SYNCHRONIZATION_SYNCHRONIZE},
    };
    let running: Vec<u16> = running_name.encode_utf16().chain(Some(0)).collect();
    let maintenance: Vec<u16> = maintenance_name.encode_utf16().chain(Some(0)).collect();
    let idle = || -> Result<()> {
        // OpenMutex only observes existence; it neither acquires nor releases installer ownership.
        let handle = unsafe { OpenMutexW(SYNCHRONIZATION_SYNCHRONIZE, 0, maintenance.as_ptr()) };
        if !handle.is_null() {
            drop(unsafe { OwnedHandle::from_raw_handle(handle) });
            return Err("Le paquet Atelier est en cours de maintenance.".into());
        }
        if unsafe { GetLastError() } != ERROR_FILE_NOT_FOUND {
            return Err("État de maintenance Windows inaccessible.".into());
        }
        Ok(())
    };
    idle()?;
    let mut handle = unsafe { OpenMutexW(SYNCHRONIZATION_SYNCHRONIZE, 0, running.as_ptr()) };
    if handle.is_null() {
        let mut descriptor = ptr::null_mut();
        // Other sessions need only SYNCHRONIZE to observe/retain the running marker, never to modify it.
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                windows_sys::core::w!("D:(A;;GA;;;SY)(A;;GA;;;BA)(A;;GA;;;OW)(A;;0x00100000;;;WD)"),
                1,
                &mut descriptor,
                ptr::null_mut(),
            )
        } == 0
        {
            return Err("Protection du verrou Windows impossible.".into());
        }
        let attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor,
            bInheritHandle: 0,
        };
        handle = unsafe { CreateMutexW(&attributes, 0, running.as_ptr()) };
        unsafe { LocalFree(descriptor) };
        if handle.is_null() {
            handle = unsafe { OpenMutexW(SYNCHRONIZATION_SYNCHRONIZE, 0, running.as_ptr()) };
        }
    }
    if handle.is_null() {
        return Err("Verrou d’installation Windows indisponible.".into());
    }
    let running = unsafe { OwnedHandle::from_raw_handle(handle) };
    idle()?;
    Ok(Guard { _running: running })
}
#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::os::fd::AsRawFd;
    #[test]
    fn a_system_installation_cannot_start_during_an_incomplete_unpack() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(
            root.path().join("installation.json"),
            r#"{"format":"atelier-rust-installation/1","mode":"system"}"#,
        )
        .unwrap();
        assert!(acquire(Some(root.path())).is_err());
        std::fs::write(root.path().join(".package-lock"), []).unwrap();
        assert!(acquire(Some(root.path())).is_ok());
    }
    #[test]
    fn recovery_preserves_a_receipt_and_refuses_foreign_or_changed_state() {
        for fault in ["none", "signature", "edited"] {
            let root = tempfile::tempdir().unwrap();
            std::fs::write(root.path().join(".package-lock"), []).unwrap();
            let marker = root.path().join(".package-maintenance");
            std::fs::write(&marker, "atelier-package-maintenance/1\n").unwrap();
            std::fs::write(root.path().join("synthetic-data"), "human content").unwrap();
            let result = recover(root.path(), || {
                if fault == "signature" {
                    return Err("Synthetic signature refusal".into());
                }
                if fault == "edited" {
                    std::fs::write(&marker, "human maintenance edit")?;
                }
                Ok(())
            });
            if fault == "none" {
                let receipt = result.unwrap();
                assert!(!marker.exists());
                assert_eq!(
                    std::fs::read(receipt).unwrap(),
                    b"atelier-package-maintenance/1\n"
                );
                assert!(acquire(Some(root.path())).is_ok());
            } else {
                assert!(result.is_err());
                assert!(marker.exists());
                assert!(acquire(Some(root.path())).is_err());
                if fault == "edited" {
                    assert_eq!(
                        std::fs::read_to_string(&marker).unwrap(),
                        "human maintenance edit"
                    );
                }
            }
            assert_eq!(
                std::fs::read_to_string(root.path().join("synthetic-data")).unwrap(),
                "human content"
            );
        }
    }

    #[test]
    fn recovery_cannot_overlap_a_server_or_follow_a_linked_marker() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join(".package-lock"), []).unwrap();
        let server = acquire(Some(root.path())).unwrap();
        let marker = root.path().join(".package-maintenance");
        std::fs::write(&marker, "atelier-package-maintenance/1\n").unwrap();
        assert!(
            recover(root.path(), || panic!(
                "verification must not run while a server owns the lock"
            ))
            .is_err()
        );
        drop(server);
        let linked = root.path().join("human-marker-copy");
        std::fs::hard_link(&marker, &linked).unwrap();
        assert!(recover(root.path(), || panic!("linked marker must not be trusted")).is_err());
        assert_eq!(
            std::fs::read(linked).unwrap(),
            b"atelier-package-maintenance/1\n"
        );
        assert!(marker.exists());
    }
    #[test]
    fn maintenance_and_live_servers_exclude_each_other_without_touching_data() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join(".package-lock"), []).unwrap();
        std::fs::write(root.path().join("synthetic-data"), "preserve").unwrap();
        let server = acquire(Some(root.path())).unwrap();
        let file = std::fs::File::open(root.path().join(".package-lock")).unwrap();
        assert_ne!(
            unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
            0
        );
        drop(server);
        assert_eq!(
            unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
            0
        );
        assert!(acquire(Some(root.path())).is_err());
        std::fs::write(
            root.path().join(".package-maintenance"),
            "synthetic install",
        )
        .unwrap();
        drop(file);
        assert!(acquire(Some(root.path())).is_err());
        assert_eq!(
            std::fs::read_to_string(root.path().join("synthetic-data")).unwrap(),
            "preserve"
        );
    }
}
#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;
    use std::os::windows::io::{FromRawHandle, OwnedHandle};
    use windows_sys::Win32::System::Threading::{
        CreateMutexW, OpenMutexW, SYNCHRONIZATION_SYNCHRONIZE,
    };
    #[test]
    fn unique_synthetic_mutexes_expose_running_and_block_start_during_maintenance() {
        let id = uuid::Uuid::new_v4();
        let running = format!("Local\\AtelierSyntheticRunning-{id}");
        let maintenance = format!("Local\\AtelierSyntheticMaintenance-{id}");
        let guard = windows_named(&running, &maintenance).unwrap();
        let name: Vec<u16> = running.encode_utf16().chain(Some(0)).collect();
        let observed = unsafe { OpenMutexW(SYNCHRONIZATION_SYNCHRONIZE, 0, name.as_ptr()) };
        assert!(!observed.is_null());
        drop(unsafe { OwnedHandle::from_raw_handle(observed) });
        let name: Vec<u16> = maintenance.encode_utf16().chain(Some(0)).collect();
        let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
        assert!(!handle.is_null());
        let installer = unsafe { OwnedHandle::from_raw_handle(handle) };
        assert!(windows_named(&running, &maintenance).is_err());
        drop(guard);
        assert!(windows_named(&running, &maintenance).is_err());
        drop(installer);
        assert!(windows_named(&running, &maintenance).is_ok());
    }
}
