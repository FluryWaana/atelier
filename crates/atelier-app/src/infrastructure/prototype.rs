use super::{
    checked_file::{CheckedFile, require_plain},
    data_lock::DataLock,
};
use crate::Result;
use cap_std::{ambient_authority, fs::Dir};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub struct PrototypeDirectory {
    pub path: PathBuf,
    _lock: DataLock,
    directory: Dir,
}

impl PrototypeDirectory {
    pub fn open(path: &Path) -> Result<Self> {
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(path)?;
        let path = fs::canonicalize(path)?;
        let directory = Dir::open_ambient_dir(&path, ambient_authority())?;
        prepare(&directory)?;
        let lock = DataLock::acquire(&path)?;
        Ok(Self {
            path,
            _lock: lock,
            directory,
        })
    }

    pub fn backups(&self) -> Result<super::backups::BackupDirectory> {
        super::backups::BackupDirectory::open_in(&self.path, &self.directory)
    }

    pub fn vault_service(&self) -> Result<String> {
        use std::io::Write;
        let name = Path::new(".atelier-prototype-vault");
        let id = match self.directory.symlink_metadata(name) {
            Ok(_) => String::from_utf8(
                CheckedFile::open_in(&self.directory, name)?
                    .private()?
                    .read(64)?,
            )?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let id = super::session::new_token()?;
                let mut options = cap_std::fs::OpenOptions::new();
                options.write(true).create_new(true);
                #[cfg(unix)]
                {
                    use cap_std::fs::OpenOptionsExt;
                    options.mode(0o600);
                }
                let mut file = self.directory.open_with(name, &options)?;
                file.write_all(id.as_bytes())?;
                file.sync_all()?;
                id
            }
            Err(error) => return Err(error.into()),
        };
        if id.len() != 64
            || !id
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err("Identifiant de coffre prototype invalide.".into());
        }
        Ok(format!(
            "{}{id}",
            super::token_vault::PROTOTYPE_SERVICE_PREFIX
        ))
    }
}

fn prepare(directory: &Dir) -> Result<()> {
    #[cfg(unix)]
    {
        use cap_std::fs::MetadataExt;
        // geteuid has no memory-safety preconditions.
        if directory.dir_metadata()?.uid() != unsafe { libc::geteuid() } {
            return Err("Le dossier du prototype doit appartenir au compte courant.".into());
        }
    }
    let marker = Path::new(".atelier-rust-prototype");
    match directory.symlink_metadata(marker) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if directory.entries()?.next().is_some() {
                return Err("Le prototype exige un dossier vide ; les données Node sont interdites avant qualification M02.".into());
            }
            let mut options = cap_std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use cap_std::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            directory.open_with(marker, &options)?.sync_all()?;
        }
        Err(error) => return Err(error.into()),
        Ok(_) => {
            CheckedFile::open_in(directory, marker)?.read(0)?;
        }
    }
    if let Some(metadata) = match directory.symlink_metadata("kanban.sqlite") {
        Ok(metadata) => Some(metadata),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    } {
        require_plain(&metadata)?;
        if !metadata.is_file() {
            return Err("Base du prototype non régulière.".into());
        }
    }
    #[cfg(unix)]
    {
        use cap_std::fs::PermissionsExt;
        directory.set_permissions(".", cap_std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prototype_vault_identifiers_are_private_stable_distinct_and_validated() {
        let root = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let opened = PrototypeDirectory::open(root.path()).unwrap();
        let service = opened.vault_service().unwrap();
        assert!(service.starts_with(super::super::token_vault::PROTOTYPE_SERVICE_PREFIX));
        assert_ne!(
            service,
            PrototypeDirectory::open(other.path())
                .unwrap()
                .vault_service()
                .unwrap()
        );
        drop(opened);
        let opened = PrototypeDirectory::open(root.path()).unwrap();
        assert_eq!(opened.vault_service().unwrap(), service);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(root.path().join(".atelier-prototype-vault"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        fs::write(
            root.path().join(".atelier-prototype-vault"),
            "atelier-kanban",
        )
        .unwrap();
        assert!(opened.vault_service().is_err());
    }

    #[test]
    fn prototype_markers_must_be_empty_regular_files_before_storage_opens() {
        for is_directory in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let marker = root.path().join(".atelier-rust-prototype");
            if is_directory {
                fs::create_dir(&marker).unwrap();
            } else {
                fs::write(&marker, "unexpected marker").unwrap();
            }
            assert!(PrototypeDirectory::open(root.path()).is_err());
            assert!(!root.path().join("kanban.sqlite").exists());
            assert!(
                !root
                    .path()
                    .join(super::super::data_lock::LOCK_NAME)
                    .exists()
            );
        }
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join(".atelier-rust-prototype"), "").unwrap();
        fs::create_dir(root.path().join("kanban.sqlite")).unwrap();
        assert!(PrototypeDirectory::open(root.path()).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn preparation_is_private_and_stays_with_the_opened_root() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let root = tempfile::tempdir().unwrap();
        let data = root.path().join("data");
        let moved = root.path().join("moved");
        let outside = root.path().join("outside");
        fs::create_dir(&data).unwrap();
        fs::create_dir(&outside).unwrap();
        fs::set_permissions(&data, fs::Permissions::from_mode(0o755)).unwrap();
        fs::write(outside.join("untouched"), "synthetic").unwrap();
        let selected = Dir::open_ambient_dir(&data, ambient_authority()).unwrap();
        fs::rename(&data, &moved).unwrap();
        symlink(&outside, &data).unwrap();
        prepare(&selected).unwrap();
        assert_eq!(
            fs::metadata(&moved).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(moved.join(".atelier-rust-prototype"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert_eq!(fs::read_dir(&outside).unwrap().count(), 1);
        fs::remove_file(moved.join(".atelier-rust-prototype")).unwrap();
        fs::write(outside.join("marker"), "").unwrap();
        symlink(
            outside.join("marker"),
            moved.join(".atelier-rust-prototype"),
        )
        .unwrap();
        assert!(prepare(&selected).is_err());
        fs::remove_file(moved.join(".atelier-rust-prototype")).unwrap();
        fs::write(moved.join(".atelier-rust-prototype"), "").unwrap();
        symlink(outside.join("untouched"), moved.join("kanban.sqlite")).unwrap();
        assert!(prepare(&selected).is_err());
        assert_eq!(fs::read(outside.join("untouched")).unwrap(), b"synthetic");
    }
}
