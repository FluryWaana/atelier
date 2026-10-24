use crate::Result;
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::Path,
};

pub fn plain_metadata(path: &Path) -> Result<std::fs::Metadata> {
    let metadata = std::fs::symlink_metadata(path)?;
    #[cfg(windows)]
    let linked = {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes()
            & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT
            != 0
    };
    #[cfg(unix)]
    let linked = metadata.file_type().is_symlink();
    if linked {
        return Err("Les liens sont interdits dans les données importées.".into());
    }
    Ok(metadata)
}

pub(super) fn require_plain(metadata: &cap_std::fs::Metadata) -> Result<()> {
    #[cfg(windows)]
    let linked = {
        use cap_std::fs::MetadataExt;
        metadata.file_attributes()
            & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT
            != 0
    };
    #[cfg(unix)]
    let linked = metadata.file_type().is_symlink();
    if linked {
        return Err("Les liens sont interdits dans les données importées.".into());
    }
    Ok(())
}

#[derive(Debug, PartialEq)]
pub struct Identity {
    device: u64,
    inode: u64,
    links: u64,
}

pub struct CheckedFile {
    file: File,
    identity: Identity,
}

impl Identity {
    pub fn inspect(path: &Path) -> Result<Self> {
        Ok(CheckedFile::open_unchecked(path)?.identity)
    }

    #[cfg(windows)]
    fn from_windows_handle(handle: windows_sys::Win32::Foundation::HANDLE) -> Result<Self> {
        use windows_sys::Win32::Storage::FileSystem::{
            BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT,
            GetFileInformationByHandle,
        };
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        // The caller retains the file/connection; Windows writes only to this live output buffer.
        if unsafe { GetFileInformationByHandle(handle, &mut info) } == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        if info.dwFileAttributes & (FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_DIRECTORY) != 0 {
            return Err("Fichier régulier sans lien requis.".into());
        }
        Ok(Self {
            device: u64::from(info.dwVolumeSerialNumber),
            inode: (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
            links: u64::from(info.nNumberOfLinks),
        })
    }
}

#[cfg(any(unix, test))]
fn sqlite_descriptor(connection: &rusqlite::Connection) -> Result<u64> {
    use rusqlite::ffi;
    struct Diagnostic(*mut ffi::sqlite3_str);
    impl Drop for Diagnostic {
        fn drop(&mut self) {
            // Both allocations belong to SQLite; finishing consumes the builder, free accepts NULL.
            unsafe { ffi::sqlite3_free(ffi::sqlite3_str_finish(self.0).cast()) };
        }
    }
    // The connection outlives its diagnostic builder and the borrowed native descriptor.
    let builder = Diagnostic(unsafe { ffi::sqlite3_str_new(connection.handle()) });
    if builder.0.is_null() {
        return Err("Diagnostic SQLite indisponible.".into());
    }
    // FILESTAT inspects the open file without preparing SQL or reading the database schema.
    let (code, output) = unsafe {
        (
            ffi::sqlite3_file_control(
                connection.handle(),
                c"main".as_ptr(),
                ffi::SQLITE_FCNTL_FILESTAT,
                builder.0.cast(),
            ),
            ffi::sqlite3_str_value(builder.0),
        )
    };
    if code != ffi::SQLITE_OK || output.is_null() {
        return Err("Identité SQLite indisponible : SQLITE_ENABLE_FILESTAT requis.".into());
    }
    // sqlite3_str_value is NUL-terminated and remains valid until the builder is finished.
    let diagnostic: serde_json::Value =
        serde_json::from_slice(unsafe { std::ffi::CStr::from_ptr(output) }.to_bytes())?;
    if !matches!(diagnostic["vfs"].as_str(), Some("unix" | "win32")) {
        return Err("VFS SQLite non qualifié pour le contrôle d’identité.".into());
    }
    diagnostic["h"]
        .as_u64()
        .ok_or_else(|| "Descripteur SQLite absent.".into())
}

impl CheckedFile {
    pub(crate) fn open_in(directory: &cap_std::fs::Dir, name: &Path) -> Result<Self> {
        use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt, OpenOptionsSyncExt};
        let mut options = cap_std::fs::OpenOptions::new();
        options.read(true).follow(FollowSymlinks::No).nonblock(true);
        Self::from_file(directory.open_with(name, &options)?.into_std())
    }

    fn open_unchecked(path: &Path) -> Result<Self> {
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            use windows_sys::Win32::Storage::FileSystem::{
                FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
            };
            options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
        }
        Self::from_file(options.open(path)?)
    }

    pub(super) fn from_file(file: File) -> Result<Self> {
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            if !file.metadata()?.is_file() {
                return Err("Fichier régulier sans lien requis.".into());
            }
            let identity = Identity::from_windows_handle(file.as_raw_handle())?;
            Ok(Self { file, identity })
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let metadata = file.metadata()?;
            if !metadata.is_file() {
                return Err("Fichier régulier sans lien requis.".into());
            }
            Ok(Self {
                file,
                identity: Identity {
                    device: metadata.dev(),
                    inode: metadata.ino(),
                    links: metadata.nlink(),
                },
            })
        }
    }

    pub fn open(path: &Path, expected: &Identity) -> Result<Self> {
        let opened = Self::open_unchecked(path)?;
        if &opened.identity != expected {
            return Err("Le fichier a changé pendant sa lecture.".into());
        }
        Ok(opened)
    }

    pub(super) fn open_same(&self, path: &Path) -> Result<Self> {
        Self::open(path, &self.identity)
    }

    #[cfg(windows)]
    pub(super) fn verify_sqlite(&self, connection: &rusqlite::Connection) -> Result<()> {
        use rusqlite::ffi;
        use windows_sys::Win32::Foundation::HANDLE;
        let mut handle: HANDLE = std::ptr::null_mut();
        // The connection and output HANDLE stay alive; this control borrows, never closes, its file.
        let code = unsafe {
            ffi::sqlite3_file_control(
                connection.handle(),
                c"main".as_ptr(),
                ffi::SQLITE_FCNTL_WIN32_GET_HANDLE,
                (&mut handle as *mut HANDLE).cast(),
            )
        };
        if code != ffi::SQLITE_OK || handle.is_null() {
            return Err("Identité du fichier SQLite indisponible.".into());
        }
        if Identity::from_windows_handle(handle)? != self.identity {
            return Err(
                "La base SQLite ouverte ne correspond plus à la source sélectionnée.".into(),
            );
        }
        Ok(())
    }

    #[cfg(unix)]
    #[allow(clippy::unnecessary_cast)] // libc field widths differ; match std MetadataExt's u64 representation.
    pub(super) fn verify_sqlite(&self, connection: &rusqlite::Connection) -> Result<()> {
        let descriptor = i32::try_from(sqlite_descriptor(connection)?)?;
        let mut metadata = std::mem::MaybeUninit::<libc::stat>::uninit();
        // SQLite retains the descriptor; fstat initializes the structure on success without closing it.
        let metadata = unsafe {
            if libc::fstat(descriptor, metadata.as_mut_ptr()) != 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            metadata.assume_init()
        };
        let identity = Identity {
            device: metadata.st_dev as u64,
            inode: metadata.st_ino as u64,
            links: metadata.st_nlink as u64,
        };
        if metadata.st_mode & libc::S_IFMT != libc::S_IFREG || identity != self.identity {
            return Err(
                "La base SQLite ouverte ne correspond plus à la source sélectionnée.".into(),
            );
        }
        Ok(())
    }

    pub fn private(self) -> Result<Self> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let metadata = self.file.metadata()?;
            // geteuid has no parameters or memory safety preconditions.
            if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o077 != 0 {
                return Err("Permissions de la session locale invalides.".into());
            }
        }
        Ok(self)
    }

    pub fn read(self, limit: usize) -> Result<Vec<u8>> {
        if self.file.metadata()?.len() > u64::try_from(limit)? {
            return Err("Fichier trop volumineux.".into());
        }
        let mut bytes = Vec::new();
        self.file
            .take(u64::try_from(
                limit.checked_add(1).ok_or("Limite de lecture invalide.")?,
            )?)
            .read_to_end(&mut bytes)?;
        if bytes.len() > limit {
            return Err("Fichier trop volumineux.".into());
        }
        Ok(bytes)
    }

    pub fn copy_to(self, output: &mut impl Write) -> Result<()> {
        let size = self.file.metadata()?.len();
        let copied = std::io::copy(
            &mut self
                .file
                .take(size.checked_add(1).ok_or("Fichier trop volumineux.")?),
            output,
        )?;
        if copied != size {
            return Err("Le fichier a changé pendant sa lecture.".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn sqlite_handle_identity_accepts_only_the_selected_file_without_closing_its_handle() {
        use rusqlite::{Connection, OpenFlags};
        let root = tempfile::tempdir().unwrap();
        let first = root.path().join("first.sqlite");
        let other = root.path().join("other.sqlite");
        for path in [&first, &other] {
            Connection::open(path)
                .unwrap()
                .execute_batch("CREATE TABLE sample(value); INSERT INTO sample VALUES (42);")
                .unwrap();
        }
        let selected = CheckedFile::open_unchecked(&first).unwrap();
        let correct =
            Connection::open_with_flags(&first, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        let wrong = Connection::open_with_flags(&other, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        let descriptor = sqlite_descriptor(&correct).unwrap();
        #[cfg(windows)]
        assert_eq!(
            Identity::from_windows_handle(
                usize::try_from(descriptor).unwrap() as windows_sys::Win32::Foundation::HANDLE
            )
            .unwrap(),
            selected.identity
        );
        #[cfg(unix)]
        assert!(i32::try_from(descriptor).is_ok());
        selected.verify_sqlite(&correct).unwrap();
        assert!(selected.verify_sqlite(&wrong).is_err());
        assert!(
            selected
                .verify_sqlite(&Connection::open_in_memory().unwrap())
                .is_err()
        );
        for connection in [&correct, &wrong] {
            assert_eq!(
                connection
                    .query_row::<i64, _, _>("SELECT value FROM sample", [], |row| row.get(0))
                    .unwrap(),
                42
            );
        }
        correct.close().unwrap();
        wrong.close().unwrap();
        let _prechecked = selected.open_same(&first).unwrap();
        fs::rename(&first, root.path().join("original.sqlite")).unwrap();
        fs::copy(&other, &first).unwrap();
        let replaced =
            Connection::open_with_flags(&first, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        assert!(selected.verify_sqlite(&replaced).is_err());
        assert_eq!(
            replaced
                .query_row::<i64, _, _>("SELECT value FROM sample", [], |row| row.get(0))
                .unwrap(),
            42
        );
        replaced.close().unwrap();
    }

    #[test]
    fn anchored_reads_reject_links_directories_and_paths_outside_the_root() {
        let root = tempfile::tempdir().unwrap();
        let private = root.path().join("private");
        fs::create_dir(&private).unwrap();
        fs::write(private.join("file"), "synthetic").unwrap();
        fs::write(root.path().join("outside"), "must not read").unwrap();
        let directory =
            cap_std::fs::Dir::open_ambient_dir(&private, cap_std::ambient_authority()).unwrap();
        assert_eq!(
            CheckedFile::open_in(&directory, Path::new("file"))
                .unwrap()
                .read(16)
                .unwrap(),
            b"synthetic"
        );
        for path in [
            Path::new("."),
            Path::new("../outside"),
            &root.path().join("outside"),
        ] {
            assert!(CheckedFile::open_in(&directory, path).is_err());
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("file", private.join("link")).unwrap();
            assert!(CheckedFile::open_in(&directory, Path::new("link")).is_err());
        }
    }

    #[test]
    fn rejects_replacement_new_hardlinks_and_oversized_files() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("file");
        fs::write(&path, "original").unwrap();
        let original = Identity::inspect(&path).unwrap();
        assert_eq!(
            CheckedFile::open(&path, &original)
                .unwrap()
                .read(32)
                .unwrap(),
            b"original"
        );
        fs::rename(&path, root.path().join("old")).unwrap();
        fs::write(&path, "replacement").unwrap();
        assert!(CheckedFile::open(&path, &original).is_err());
        let replaced = Identity::inspect(&path).unwrap();
        fs::hard_link(&path, root.path().join("alias")).unwrap();
        assert!(CheckedFile::open(&path, &replaced).is_err());
        let linked = Identity::inspect(&path).unwrap();
        let opened = CheckedFile::open(&path, &linked).unwrap();
        fs::write(&path, [b'x'; 33]).unwrap();
        assert!(opened.read(32).is_err());
        assert!(Identity::inspect(root.path()).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn refuses_symlinks_and_permissions_changed_after_inspection() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("session");
        fs::write(&path, "private").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        let identity = Identity::inspect(&path).unwrap();
        assert!(
            CheckedFile::open(&path, &identity)
                .unwrap()
                .private()
                .is_ok()
        );
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(
            CheckedFile::open(&path, &identity)
                .unwrap()
                .private()
                .is_err()
        );
        let link = root.path().join("link");
        symlink(&path, &link).unwrap();
        assert!(Identity::inspect(&link).is_err());
        assert!(CheckedFile::open(&link, &identity).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn refuses_a_windows_junction_reparse_point() {
        use std::{os::windows::process::CommandExt, process::Command};
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("target");
        let junction = root.path().join("junction");
        fs::create_dir(&target).unwrap();
        let output = Command::new(std::env::var_os("ComSpec").unwrap_or_else(|| "cmd.exe".into()))
            .args(["/D", "/C"])
            .raw_arg("mklink /J \"%ATELIER_TEST_JUNCTION%\" \"%ATELIER_TEST_TARGET%\"")
            .env("ATELIER_TEST_JUNCTION", &junction)
            .env("ATELIER_TEST_TARGET", &target)
            .creation_flags(0x08000000)
            .output()
            .unwrap();
        assert!(output.status.success(), "Junction fixture creation failed");
        assert!(Identity::inspect(&junction).is_err());
    }
}
