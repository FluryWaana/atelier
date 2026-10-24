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
}

impl CheckedFile {
    pub(super) fn open_in(directory: &cap_std::fs::Dir, name: &Path) -> Result<Self> {
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
            use windows_sys::Win32::Storage::FileSystem::{
                BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_REPARSE_POINT,
                GetFileInformationByHandle,
            };
            let mut info = BY_HANDLE_FILE_INFORMATION::default();
            // The owned file handle and output buffer remain valid for this synchronous call.
            if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            if info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
                || !file.metadata()?.is_file()
            {
                return Err("Fichier régulier sans lien requis.".into());
            }
            Ok(Self {
                file,
                identity: Identity {
                    device: u64::from(info.dwVolumeSerialNumber),
                    inode: (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
                    links: u64::from(info.nNumberOfLinks),
                },
            })
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
