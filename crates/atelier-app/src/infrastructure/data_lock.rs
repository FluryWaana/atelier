use crate::Result;
use cap_std::{ambient_authority, fs::Dir};
use rusqlite::Connection;
use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

pub const LOCK_NAME: &str = ".atelier-data-lock.sqlite";

pub struct DataLock {
    pub directory: PathBuf,
    _connection: Connection,
    _directory: Dir,
}

impl DataLock {
    pub fn acquire(directory: &Path) -> Result<Self> {
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(directory)?;
        let directory = fs::canonicalize(directory)?;
        let handle = Dir::open_ambient_dir(&directory, ambient_authority())?;
        Self::acquire_in(&directory, &handle)
    }

    pub(super) fn acquire_in(directory: &Path, handle: &Dir) -> Result<Self> {
        let connection = super::store::open_checked(&directory.join(LOCK_NAME), handle)?;
        connection.busy_timeout(Duration::ZERO)?;
        connection
            .execute_batch("BEGIN EXCLUSIVE")
            .map_err(|_| "Ce dossier est déjà ouvert ou son verrou de données est invalide.")?;
        Ok(Self {
            directory: directory.to_owned(),
            _connection: connection,
            _directory: handle.try_clone()?,
        })
    }
}

pub fn is_lock_file(name: &str) -> bool {
    ["", "-journal", "-wal", "-shm"]
        .iter()
        .any(|suffix| name == format!("{LOCK_NAME}{suffix}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aliases_conflict_and_release_allows_reopening_without_deleting_the_lock_file() {
        let root = tempfile::tempdir().unwrap();
        let guard = DataLock::acquire(root.path()).unwrap();
        assert!(DataLock::acquire(&root.path().join(".")).is_err());
        assert!(DataLock::acquire(root.path()).is_err());
        assert!(!root.path().join("kanban.sqlite").exists());
        drop(guard);
        let again = DataLock::acquire(root.path()).unwrap();
        assert!(root.path().join(LOCK_NAME).is_file());
        drop(again);
        fs::write(root.path().join(LOCK_NAME), "corrupt").unwrap();
        assert!(DataLock::acquire(root.path()).is_err());
    }
}
