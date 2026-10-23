use super::checked_file::plain_metadata;
use crate::Result;
use rusqlite::{Connection, OpenFlags};
use std::{
    fs::{self, OpenOptions},
    path::{Path, PathBuf},
    time::Duration,
};

pub const LOCK_NAME: &str = ".atelier-data-lock.sqlite";

pub struct DataLock {
    pub directory: PathBuf,
    _connection: Connection,
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
        let path = directory.join(LOCK_NAME);
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&path) {
            Ok(file) => drop(file),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
        if !plain_metadata(&path)?.is_file() {
            return Err("Verrou de données invalide.".into());
        }
        // Never open/close this inode outside SQLite while held: POSIX closes can release process locks.
        let connection = Connection::open_with_flags(
            &path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )?;
        connection.busy_timeout(Duration::ZERO)?;
        connection
            .execute_batch("BEGIN EXCLUSIVE")
            .map_err(|_| "Ce dossier est déjà ouvert ou son verrou de données est invalide.")?;
        Ok(Self {
            directory,
            _connection: connection,
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
