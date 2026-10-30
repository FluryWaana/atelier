use crate::Result;
use cap_std::fs::Dir;
use std::path::{Component, Path};

#[cfg(target_os = "linux")]
pub(super) fn directory_file(directory: &Dir) -> Result<std::fs::File> {
    use std::os::fd::{AsRawFd, FromRawFd};
    // cap-std may hold an O_PATH descriptor. Reopen the same anchored directory, not its ambient name.
    let descriptor = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            c".".as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if descriptor < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    // A successful openat returned a fresh descriptor; this File becomes its unique owner.
    Ok(unsafe { std::fs::File::from_raw_fd(descriptor) })
}

#[cfg(target_os = "linux")]
pub(super) fn sync_directory(directory: &Dir) -> Result<()> {
    directory_file(directory)?.sync_all()?;
    Ok(())
}

#[cfg(target_os = "linux")]
pub(super) fn rename_new_between(
    source: &Dir,
    name: &std::ffi::OsStr,
    destination: &Dir,
    next: &std::ffi::OsStr,
) -> Result<()> {
    use std::{
        ffi::CString,
        os::{fd::AsRawFd, unix::ffi::OsStrExt},
    };
    for name in [name, next] {
        if !matches!(
            Path::new(name).components().collect::<Vec<_>>().as_slice(),
            [Component::Normal(_)]
        ) {
            return Err("Nom de publication système invalide.".into());
        }
    }
    let name = CString::new(name.as_bytes())?;
    let next = CString::new(next.as_bytes())?;
    // The verified source and destination handles remain live, and existing names are never replaced.
    if unsafe {
        libc::renameat2(
            source.as_raw_fd(),
            name.as_ptr(),
            destination.as_raw_fd(),
            next.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    } != 0
    {
        return Err(std::io::Error::last_os_error().into());
    }
    sync_directory(source)?;
    sync_directory(destination)?;
    Ok(())
}

pub(super) fn write_private(
    directory: &Dir,
    name: &Path,
    bytes: &[u8],
    executable: bool,
) -> Result<()> {
    write_private_from(directory, directory, name, bytes, executable)
}

pub(super) fn write_private_from(
    staging: &Dir,
    directory: &Dir,
    name: &Path,
    bytes: &[u8],
    executable: bool,
) -> Result<()> {
    write_private_from_with_sync(staging, directory, name, bytes, executable, |directory| {
        #[cfg(target_os = "linux")]
        sync_directory(directory)?;
        #[cfg(not(target_os = "linux"))]
        let _ = directory;
        Ok(())
    })
}

fn write_private_from_with_sync(
    staging: &Dir,
    directory: &Dir,
    name: &Path,
    bytes: &[u8],
    _executable: bool,
    synchronize: impl Fn(&Dir) -> Result<()>,
) -> Result<()> {
    if !matches!(
        name.components().collect::<Vec<_>>().as_slice(),
        [Component::Normal(_)]
    ) {
        return Err("Nom de publication invalide.".into());
    }
    let temporary = Path::new(".atelier-write").with_extension(uuid::Uuid::new_v4().to_string());
    let mut options = cap_std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use cap_std::fs::OpenOptionsExt;
        options.mode(if _executable { 0o700 } else { 0o600 });
    }
    let result = (|| -> Result<()> {
        use std::io::Write;
        let mut file = staging.open_with(&temporary, &options)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        staging.rename(&temporary, directory, name)?;
        // Persist the published name before acknowledging it, then the staging removal.
        synchronize(directory)?;
        synchronize(staging)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = staging.remove_file(&temporary);
    }
    result
}

pub(super) fn rename_new(directory: &Dir, source: &Path, destination: &Path) -> Result<()> {
    for name in [source, destination] {
        if !matches!(
            name.components().collect::<Vec<_>>().as_slice(),
            [Component::Normal(_)]
        ) {
            return Err("Nom de publication invalide.".into());
        }
    }
    #[cfg(unix)]
    {
        use std::{
            ffi::CString,
            os::{fd::AsRawFd, unix::ffi::OsStrExt},
        };
        let source = CString::new(source.as_os_str().as_bytes())?;
        let destination = CString::new(destination.as_os_str().as_bytes())?;
        let descriptor = directory.as_raw_fd();
        // Both names and the directory descriptor remain valid throughout this atomic call.
        #[cfg(target_os = "linux")]
        let result = unsafe {
            libc::renameat2(
                descriptor,
                source.as_ptr(),
                descriptor,
                destination.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        };
        #[cfg(target_os = "macos")]
        let result = unsafe {
            libc::renameatx_np(
                descriptor,
                source.as_ptr(),
                descriptor,
                destination.as_ptr(),
                libc::RENAME_EXCL,
            )
        };
        if result != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
    }
    #[cfg(windows)]
    {
        use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt, OpenOptionsMaybeDirExt};
        use cap_std::fs::OpenOptionsExt;
        use std::os::windows::{ffi::OsStrExt, io::AsRawHandle};
        use windows_sys::Win32::Storage::FileSystem::{
            DELETE, FILE_READ_ATTRIBUTES, FILE_RENAME_INFO, FileRenameInfo,
            GetFinalPathNameByHandleW, SetFileInformationByHandle,
        };
        let name: Vec<u16> = destination.as_os_str().encode_wide().collect();
        if name.contains(&0) || name.contains(&u16::from(b':')) {
            return Err("Nom de publication invalide.".into());
        }
        // cap-std holds directories without FILE_SHARE_DELETE, keeping this resolved path stable.
        let size = unsafe {
            GetFinalPathNameByHandleW(directory.as_raw_handle(), std::ptr::null_mut(), 0, 0)
        };
        if size == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let mut target = vec![0; size as usize + 1];
        let count = unsafe {
            GetFinalPathNameByHandleW(
                directory.as_raw_handle(),
                target.as_mut_ptr(),
                u32::try_from(target.len())?,
                0,
            )
        };
        if count == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        if count as usize >= target.len() {
            return Err("Le dossier de publication a changé.".into());
        }
        target.truncate(count as usize);
        target.push(u16::from(b'\\'));
        target.extend(name);
        let mut options = cap_std::fs::OpenOptions::new();
        options
            .read(true)
            .follow(FollowSymlinks::No)
            .maybe_dir(true)
            .access_mode(DELETE | FILE_READ_ATTRIBUTES);
        let file = directory
            .open_with(source, &options)
            .map_err(|error| format!("Ouverture de publication : {error}"))?;
        let size = std::mem::size_of::<FILE_RENAME_INFO>() + target.len() * 2;
        let mut buffer = vec![0usize; size.div_ceil(std::mem::size_of::<usize>())];
        // The aligned buffer includes the variable UTF-16 suffix; both handles outlive the call.
        unsafe {
            let info = buffer.as_mut_ptr().cast::<FILE_RENAME_INFO>();
            (*info).Anonymous.ReplaceIfExists = false;
            (*info).RootDirectory = std::ptr::null_mut();
            (*info).FileNameLength = u32::try_from(target.len() * 2)?;
            std::ptr::copy_nonoverlapping(
                target.as_ptr(),
                std::ptr::addr_of_mut!((*info).FileName).cast::<u16>(),
                target.len(),
            );
            if SetFileInformationByHandle(
                file.as_raw_handle(),
                FileRenameInfo,
                buffer.as_ptr().cast(),
                u32::try_from(size)?,
            ) == 0
            {
                return Err(format!(
                    "Renommage de publication : {}",
                    std::io::Error::last_os_error()
                )
                .into());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cap_std::ambient_authority;
    use std::fs;

    #[test]
    fn synchronization_errors_are_reported_after_publication_without_removing_the_result() {
        use std::cell::Cell;
        let root = tempfile::tempdir().unwrap();
        let directory = Dir::open_ambient_dir(root.path(), ambient_authority()).unwrap();
        directory.create_dir("stage").unwrap();
        let staging = directory.open_dir("stage").unwrap();
        for failure in [1, 2, 3] {
            let calls = Cell::new(0);
            let result = write_private_from_with_sync(
                &staging,
                &directory,
                Path::new("journal"),
                b"saved phase",
                false,
                |anchor| {
                    calls.set(calls.get() + 1);
                    if calls.get() == 1 {
                        assert_eq!(anchor.read("journal")?, b"saved phase");
                    } else {
                        assert_eq!(anchor.entries()?.count(), 0);
                    }
                    if calls.get() == failure {
                        return Err("synthetic directory sync failure".into());
                    }
                    Ok(())
                },
            );
            assert_eq!(result.is_err(), failure <= 2);
            assert_eq!(calls.get(), failure.min(2));
            assert_eq!(directory.read("journal").unwrap(), b"saved phase");
            assert_eq!(staging.entries().unwrap().count(), 0);
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn directory_sync_and_permissions_reopen_path_descriptors_without_following_replaced_names() {
        use std::os::{
            fd::{AsRawFd, FromRawFd},
            unix::fs::{MetadataExt, PermissionsExt},
        };
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("original");
        let moved = root.path().join("moved");
        let outside = root.path().join("outside");
        fs::create_dir(&path).unwrap();
        fs::create_dir(&outside).unwrap();
        let anchor = Dir::open_ambient_dir(&path, ambient_authority()).unwrap();
        // This fresh synthetic O_PATH descriptor is owned exactly once by the File below.
        let descriptor = unsafe {
            libc::openat(
                anchor.as_raw_fd(),
                c".".as_ptr(),
                libc::O_PATH | libc::O_DIRECTORY | libc::O_CLOEXEC,
            )
        };
        assert!(descriptor >= 0);
        let directory = Dir::from_std_file(unsafe { fs::File::from_raw_fd(descriptor) });
        let original = directory.dir_metadata().unwrap();
        let outside_mode = fs::metadata(&outside).unwrap().mode();
        fs::rename(&path, &moved).unwrap();
        std::os::unix::fs::symlink(&outside, &path).unwrap();
        let file = directory_file(&directory).unwrap();
        let reopened = file.metadata().unwrap();
        use cap_std::fs::MetadataExt as _;
        assert_eq!(
            (original.dev(), original.ino()),
            (reopened.dev(), reopened.ino())
        );
        file.set_permissions(fs::Permissions::from_mode(0o750))
            .unwrap();
        sync_directory(&directory).unwrap();
        assert_eq!(fs::metadata(&moved).unwrap().mode() & 0o777, 0o750);
        assert_eq!(fs::metadata(&outside).unwrap().mode(), outside_mode);
    }

    #[test]
    fn failed_private_publication_cleans_staging_and_preserves_existing_data() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("metadata")).unwrap();
        fs::create_dir(root.path().join("target")).unwrap();
        let metadata =
            Dir::open_ambient_dir(root.path().join("metadata"), ambient_authority()).unwrap();
        let target =
            Dir::open_ambient_dir(root.path().join("target"), ambient_authority()).unwrap();
        target.create_dir("occupied").unwrap();
        target.write("occupied/preserved", b"keep").unwrap();
        assert!(
            write_private_from(
                &metadata,
                &target,
                Path::new("occupied"),
                b"private synthetic bytes",
                false
            )
            .is_err()
        );
        assert_eq!(metadata.entries().unwrap().count(), 0);
        assert_eq!(target.entries().unwrap().count(), 1);
        assert_eq!(target.read("occupied/preserved").unwrap(), b"keep");
        write_private_from(&metadata, &target, Path::new("saved"), b"first", false).unwrap();
        write_private_from(&metadata, &target, Path::new("saved"), b"second", false).unwrap();
        assert_eq!(target.read("saved").unwrap(), b"second");
        assert_eq!(metadata.entries().unwrap().count(), 0);
    }

    #[test]
    fn publication_is_anchored_and_never_replaces_an_existing_destination() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("directory");
        fs::create_dir(&path).unwrap();
        let directory = Dir::open_ambient_dir(&path, ambient_authority()).unwrap();
        directory.write("temporary", b"synthetic").unwrap();
        directory.write("existing", b"preserved").unwrap();
        for invalid in ["existing", "../escape", "."] {
            assert!(rename_new(&directory, Path::new("temporary"), Path::new(invalid)).is_err());
        }
        assert_eq!(directory.read("existing").unwrap(), b"preserved");
        assert_eq!(directory.read("temporary").unwrap(), b"synthetic");
        #[cfg(unix)]
        {
            let outside = root.path().join("outside");
            fs::create_dir(&outside).unwrap();
            fs::rename(&path, root.path().join("moved")).unwrap();
            std::os::unix::fs::symlink(&outside, &path).unwrap();
        }
        #[cfg(windows)]
        {
            assert!(fs::rename(&path, root.path().join("moved")).is_err());
            assert!(fs::rename(root.path(), root.path().with_extension("moved")).is_err());
        }
        rename_new(
            &directory,
            Path::new("temporary"),
            Path::new("saved é.sqlite"),
        )
        .unwrap();
        assert_eq!(directory.read("saved é.sqlite").unwrap(), b"synthetic");
        assert!(!directory.exists("temporary"));
        directory.create_dir("stage").unwrap();
        rename_new(&directory, Path::new("stage"), Path::new("published")).unwrap();
        assert!(directory.is_dir("published"));
    }
}
