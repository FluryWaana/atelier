use crate::Result;
use std::path::Path;
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
        let directory = cap_std::fs::Dir::open_ambient_dir(root, cap_std::ambient_authority())?;
        let marker = || -> Result<()> {
            match directory.symlink_metadata(".package-maintenance") {Ok(_)=>Err("Une installation ou désinstallation est en cours. Termine sa récupération avant de lancer Atelier.".into()),Err(error) if error.kind()==std::io::ErrorKind::NotFound=>Ok(()),Err(error)=>Err(error.into())}
        };
        marker()?;
        let file = match directory.symlink_metadata(".package-lock") {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Guard { _lock: None });
            }
            Err(error) => return Err(error.into()),
            Ok(metadata) => {
                super::checked_file::require_plain(&metadata)?;
                let mut options = cap_std::fs::OpenOptions::new();
                options.read(true).follow(FollowSymlinks::No).nonblock(true);
                let file = directory.open_with(".package-lock", &options)?.into_std();
                if !file.metadata()?.is_file() {
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
