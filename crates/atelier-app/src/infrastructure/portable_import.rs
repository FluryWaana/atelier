use super::{
    checked_file::{CheckedFile, plain_metadata, require_plain},
    store::Store,
};
use crate::Result;
use cap_fs_ext::DirExt;
use cap_std::{ambient_authority, fs::Dir};
use rusqlite::{Connection, OpenFlags, ffi};
use std::{fs, path::Path};

fn create_directory(path: &Path) -> Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)?;
    Ok(())
}

fn private_directory(parent: &Dir, name: &Path) -> Result<Dir> {
    let builder = cap_std::fs::DirBuilder::new();
    #[cfg(unix)]
    let builder = {
        use cap_std::fs::DirBuilderExt;
        let mut builder = builder;
        builder.mode(0o700);
        builder
    };
    parent.create_dir_with(name, &builder)?;
    Ok(parent.open_dir_nofollow(name)?)
}

fn copy_tree(source: &Dir, destination: &Dir, root: bool) -> Result<()> {
    copy_tree_bounded(source, destination, root, 0, &mut (0usize, 0u64))
}
fn copy_tree_bounded(
    source: &Dir,
    destination: &Dir,
    root: bool,
    depth: usize,
    budget: &mut (usize, u64),
) -> Result<()> {
    use std::io::Write;
    if depth > 32 {
        return Err("Transfert portable trop profond.".into());
    }
    require_plain(&source.dir_metadata()?)?;
    require_plain(&destination.dir_metadata()?)?;
    for entry in source.entries()? {
        let entry = entry?;
        let name = entry.file_name();
        if root && name.to_str().is_some_and(super::data_lock::is_lock_file) {
            continue;
        }
        if root
            && (matches!(name.to_str(), Some("local-session" | "local-browser.html"))
                || name.to_str() == Some(super::repository_access::FILE))
        {
            continue;
        }
        let metadata = source.symlink_metadata(&name)?;
        require_plain(&metadata)?;
        budget.0 = budget
            .0
            .checked_add(1)
            .ok_or("Transfert portable trop volumineux.")?;
        if budget.0 > 100_000 {
            return Err("Transfert portable limité à 100 000 entrées.".into());
        }
        if root
            && matches!(
                name.to_str(),
                Some("kanban.sqlite" | "kanban.sqlite-wal" | "kanban.sqlite-shm")
            )
        {
            continue;
        }
        if metadata.is_dir() {
            let child = source.open_dir_nofollow(&name)?;
            copy_tree_bounded(
                &child,
                &private_directory(destination, Path::new(&name))?,
                false,
                depth + 1,
                budget,
            )?;
        } else if metadata.is_file() {
            let checked = CheckedFile::open_in(source, Path::new(&name))?;
            let bytes = checked.read(128 * 1024 * 1024)?;
            budget.1 = budget
                .1
                .checked_add(bytes.len() as u64)
                .ok_or("Transfert portable trop volumineux.")?;
            if budget.1 > 1024 * 1024 * 1024 {
                return Err("Transfert portable limité à 1 Gio hors base.".into());
            }
            let mut options = cap_std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use cap_std::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut output = destination.open_with(&name, &options)?;
            output.write_all(&bytes)?;
            output.sync_all()?;
        } else {
            return Err("Fichier spécial interdit dans les données importées.".into());
        }
    }
    Ok(())
}

fn resolve_destination(path: &Path) -> Result<std::path::PathBuf> {
    let mut ancestor = path;
    let mut missing = Vec::new();
    let mut resolved = loop {
        match fs::canonicalize(ancestor) {
            Ok(path) => break path,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                missing.push(
                    ancestor
                        .file_name()
                        .ok_or("Dossier de destination invalide.")?,
                );
                ancestor = ancestor
                    .parent()
                    .ok_or("Dossier de destination invalide.")?;
            }
            Err(error) => return Err(error.into()),
        }
    };
    for name in missing.into_iter().rev() {
        resolved.push(name);
    }
    Ok(resolved)
}

struct SourceLock {
    _connection: Connection,
    file: *mut ffi::sqlite3_file,
    unlock: unsafe extern "C" fn(*mut ffi::sqlite3_file, i32) -> i32,
}

impl SourceLock {
    fn acquire(connection: Connection) -> Result<Self> {
        let mut file: *mut ffi::sqlite3_file = std::ptr::null_mut();
        // No SQL is prepared on this connection: only its main-file VFS lock is used.
        let code = unsafe {
            ffi::sqlite3_file_control(
                connection.handle(),
                c"main".as_ptr(),
                ffi::SQLITE_FCNTL_FILE_POINTER,
                (&mut file as *mut *mut ffi::sqlite3_file).cast(),
            )
        };
        if code != ffi::SQLITE_OK || file.is_null() {
            return Err("Verrou SQLite indisponible.".into());
        }
        // The connection owns this file and its VFS methods until SourceLock is dropped.
        let methods = unsafe { (*file).pMethods.as_ref() }
            .ok_or("Méthodes de verrouillage SQLite absentes.")?;
        let lock = methods.xLock.ok_or("Verrou SQLite indisponible.")?;
        let guard = Self {
            unlock: methods
                .xUnlock
                .ok_or("Déverrouillage SQLite indisponible.")?,
            file,
            _connection: connection,
        };
        for level in [ffi::SQLITE_LOCK_SHARED, ffi::SQLITE_LOCK_EXCLUSIVE] {
            // VFS bookkeeping also detects competing connections within this process.
            if unsafe { lock(guard.file, level) } != ffi::SQLITE_OK {
                return Err("Fermez l’application source et rendez sa base accessible en écriture avant l’import.".into());
            }
        }
        Ok(guard)
    }

    fn copy_main(&self, output: &mut impl std::io::Write) -> Result<()> {
        // Read through the locking handle: Windows byte-range locks also block other handles here.
        let methods = unsafe { &*(*self.file).pMethods };
        let size = methods.xFileSize.ok_or("Taille SQLite indisponible.")?;
        let read = methods.xRead.ok_or("Lecture SQLite indisponible.")?;
        let mut length = 0;
        // The VFS initializes length; the guard retains the file throughout every read.
        if unsafe { size(self.file, &mut length) } != ffi::SQLITE_OK
            || !(0..=1024 * 1024 * 1024).contains(&length)
        {
            return Err("Taille de la source SQLite invalide.".into());
        }
        let mut buffer = [0u8; 64 * 1024];
        let mut offset = 0;
        while offset < length {
            let count = usize::try_from((length - offset).min(buffer.len() as i64))?;
            // count fits the live buffer; offset stays inside the locked file's measured length.
            if unsafe { read(self.file, buffer.as_mut_ptr().cast(), count as i32, offset) }
                != ffi::SQLITE_OK
            {
                return Err("Lecture de la source SQLite impossible.".into());
            }
            output.write_all(&buffer[..count])?;
            offset += count as i64;
        }
        Ok(())
    }
}

impl Drop for SourceLock {
    fn drop(&mut self) {
        // Release even a partial PENDING lock before SQLite closes the still-owned file.
        unsafe { (self.unlock)(self.file, ffi::SQLITE_LOCK_NONE) };
    }
}

fn snapshot_source(database: &Path, source: &Dir) -> Result<tempfile::TempDir> {
    #[cfg(windows)]
    let selected = CheckedFile::open_in(source, Path::new("kanban.sqlite"))?;
    #[cfg(windows)]
    let _checked = selected.open_same(database)?;
    #[cfg(unix)]
    let selected =
        super::checked_file::Identity::sqlite_source(source, Path::new("kanban.sqlite"))?;
    // shortcut: POSIX source locks need write access; qualify read-only media before enabling it.
    let connection = Connection::open_with_flags(
        database,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )?;
    selected.verify_sqlite(&connection)?;
    let lock = SourceLock::acquire(connection)?;
    copy_snapshot(source, &lock)
}

fn copy_snapshot(source: &Dir, lock: &SourceLock) -> Result<tempfile::TempDir> {
    let snapshot = super::store::temporary_directory()?;
    let target = Dir::open_ambient_dir(snapshot.path(), ambient_authority())?;
    // shortcut: refuse rollback journals until isolated recovery is qualified.
    match source.symlink_metadata("kanban.sqlite-journal") {
        Ok(_) => return Err("Journal de récupération présent : ouvrez puis fermez la version source avant l’import.".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let wal = match source.symlink_metadata("kanban.sqlite-wal") {
        Ok(_) => Some(CheckedFile::open_in(
            source,
            Path::new("kanban.sqlite-wal"),
        )?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    if source
        .symlink_metadata("kanban.sqlite-wal")
        .is_ok_and(|metadata| metadata.len() > 1024 * 1024 * 1024)
    {
        return Err("Journal portable limité à 1 Gio.".into());
    }
    for (name, input) in [("kanban.sqlite", None), ("kanban.sqlite-wal", wal.as_ref())] {
        if name == "kanban.sqlite" || input.is_some() {
            let mut options = cap_std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use cap_std::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut output = target.open_with(name, &options)?;
            if let Some(input) = input {
                input.copy_to(&mut output)?;
            } else {
                lock.copy_main(&mut output)?;
            }
            output.sync_all()?;
        }
    }
    Ok(snapshot)
}

pub fn import(source: &Path, destination: &Path) -> Result<()> {
    if !plain_metadata(source)?.is_dir() {
        return Err("Choisissez le dossier data de la version portable.".into());
    }
    let source = fs::canonicalize(source)?;
    let destination = std::path::absolute(destination)?;
    if destination == source || destination.starts_with(&source) {
        return Err("Le dossier de destination doit être distinct de la source.".into());
    }
    require_empty(&destination)?;
    let destination = resolve_destination(&destination)?;
    if destination == source || destination.starts_with(&source) {
        return Err("Le dossier de destination doit être distinct de la source.".into());
    }
    let database = source.join("kanban.sqlite");
    if !plain_metadata(&database)?.is_file() {
        return Err("Base portable invalide.".into());
    }
    let parent = destination
        .parent()
        .ok_or("Dossier de destination invalide.")?;
    create_directory(parent)?;
    let parent = fs::canonicalize(parent)?;
    let destination = parent.join(
        destination
            .file_name()
            .ok_or("Dossier de destination invalide.")?,
    );
    if destination == source || destination.starts_with(&source) {
        return Err("Le dossier de destination doit être distinct de la source.".into());
    }
    let parent = Dir::open_ambient_dir(&parent, ambient_authority())?;
    let source_directory = Dir::open_ambient_dir(
        source.parent().ok_or("Dossier source invalide.")?,
        ambient_authority(),
    )?
    .open_dir_nofollow(source.file_name().ok_or("Dossier source invalide.")?)?;
    import_into(
        &database,
        &source_directory,
        &parent,
        Path::new(
            destination
                .file_name()
                .ok_or("Dossier de destination invalide.")?,
        ),
    )
}

fn arguments(
    args: impl Iterator<Item = std::ffi::OsString>,
) -> Result<(Option<std::path::PathBuf>, std::path::PathBuf)> {
    let mut args = args;
    let mut source = None;
    let mut destination = None;
    let mut confirmed = false;
    let mut choose = false;
    while let Some(argument) = args.next() {
        match argument.to_str() {
            Some("--source") if source.is_none() && !choose=>source=Some(std::path::PathBuf::from(args.next().ok_or("Dossier source requis.")?)),
            Some("--choose-source") if source.is_none() && !choose=>choose=true,
            Some("--destination") if destination.is_none()=>destination=Some(std::path::PathBuf::from(args.next().ok_or("Dossier de destination requis.")?)),
            Some("--confirm") if !confirmed=>confirmed=true,
            _=>return Err("Usage : atelier import-data --source DONNEES_RUST --destination DOSSIER_VIDE --confirm".into())
        }
    }
    if !confirmed {
        return Err(
            "Confirme la copie vers un dossier vide ; les données sources seront conservées."
                .into(),
        );
    }
    if source.is_none() && !choose {
        return Err("Dossier source requis ou sélection explicite --choose-source.".into());
    }
    let destination = destination.ok_or("Dossier de destination requis.")?;
    if source.as_ref().is_some_and(|source| !source.is_absolute()) || !destination.is_absolute() {
        return Err("Les dossiers du transfert doivent être absolus.".into());
    }
    for path in source.iter().chain(std::iter::once(&destination)) {
        if path
            .to_str()
            .is_none_or(|path| path.len() > 32_000 || path.chars().any(char::is_control))
        {
            return Err("Les chemins du transfert doivent être des textes Unicode bornés sans caractères de contrôle.".into());
        }
    }
    Ok((source, destination))
}
pub async fn command(args: impl Iterator<Item = std::ffi::OsString>) -> Result<()> {
    let (source, destination) = arguments(args)?;
    let source = match source {
        Some(source) => source,
        None => {
            let Some(source) = super::folders::choose().await? else {
                println!("Transfert annulé ; aucune copie effectuée.");
                return Ok(());
            };
            let source = std::path::PathBuf::from(source);
            if !super::folders::confirm_transfer(&source, &destination).await? {
                println!("Transfert annulé ; aucune copie effectuée.");
                return Ok(());
            }
            source
        }
    };
    import(&source, &destination)?;
    println!(
        "Transfert Rust terminé. Les données sources sont conservées ; réautorise les dépôts locaux sur la destination."
    );
    Ok(())
}
fn import_into(database: &Path, source: &Dir, parent: &Dir, destination: &Path) -> Result<()> {
    require_empty_in(parent, destination)?;
    let _source_lock = super::data_lock::DataLock::acquire_in(
        database.parent().ok_or("Dossier source absent.")?,
        source,
    )?;
    let stage_name = format!(".atelier-import-{}", uuid::Uuid::new_v4());
    let stage = private_directory(parent, Path::new(&stage_name))?;
    let prepared = (|| -> Result<()> {
        let snapshot = snapshot_source(database, source)?;
        let connection = Connection::open_with_flags(
            snapshot.path().join("kanban.sqlite"),
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )?;
        super::store::validate_format(&connection)?;
        copy_tree(source, &stage, true)?;
        let integrity: String = connection.query_row("PRAGMA quick_check", [], |row| row.get(0))?;
        if integrity != "ok" {
            return Err("La base portable est endommagée.".into());
        }
        for table in ["cards", "projects", "settings"] {
            connection.prepare(&format!("SELECT * FROM {table} LIMIT 0"))?;
        }
        // Recover committed WAL only in the private copy, then use SQLite's backup API.
        Store::new(&connection).backup_in(&stage, Path::new("kanban.sqlite"))?;
        connection.close().map_err(|(_, error)| error)?;
        let backups = match stage.open_dir_nofollow("backups") {
            Ok(directory) => directory,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                private_directory(&stage, Path::new("backups"))?
            }
            Err(error) => return Err(error.into()),
        };
        require_plain(&backups.dir_metadata()?)?;
        let mut options = cap_std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use cap_std::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut output = backups.open_with(super::backups::new_name()?, &options)?;
        CheckedFile::open_in(&stage, Path::new("kanban.sqlite"))?.copy_to(&mut output)?;
        output.sync_all()?;
        Ok(())
    })();
    drop(stage);
    let result = prepared.and_then(|()| {
        if require_empty_in(parent, destination)? {
            parent.remove_dir(destination)?;
        }
        super::publish::rename_new(parent, Path::new(&stage_name), destination)
    });
    if result.is_err() {
        let _ = parent.remove_dir_all(&stage_name);
    }
    result
}

fn require_empty_in(parent: &Dir, name: &Path) -> Result<bool> {
    match parent.symlink_metadata(name) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
        Ok(metadata) => {
            require_plain(&metadata)?;
            let directory = parent.open_dir_nofollow(name)?;
            require_plain(&directory.dir_metadata()?)?;
            if directory.entries()?.next().is_some() {
                return Err("Des données existent déjà. L’import ne les écrasera pas.".into());
            }
            Ok(true)
        }
    }
}

fn require_empty(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
        Ok(_) => {
            if !plain_metadata(path)?.is_dir() || fs::read_dir(path)?.next().is_some() {
                return Err("Des données existent déjà. L’import ne les écrasera pas.".into());
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn choosing_a_source_is_explicit_exclusive_and_validated_before_native_ui() {
        let root = tempfile::tempdir().unwrap();
        let args = vec![
            "--choose-source".into(),
            "--destination".into(),
            root.path().join("new").into_os_string(),
            "--confirm".into(),
        ];
        assert!(arguments(args.clone().into_iter()).unwrap().0.is_none());
        let mut missing = args.clone();
        missing.pop();
        assert!(arguments(missing.into_iter()).is_err());
        let mut duplicate = args.clone();
        duplicate.push("--choose-source".into());
        assert!(arguments(duplicate.into_iter()).is_err());
        let mut ambiguous = args;
        ambiguous.extend(["--source".into(), root.path().as_os_str().to_owned()]);
        assert!(arguments(ambiguous.into_iter()).is_err());
        assert!(std::fs::read_dir(root.path()).unwrap().next().is_none());
    }
    #[test]
    fn excessive_depth_leaves_no_published_destination_and_cli_requires_explicit_paths_and_confirmation()
     {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        let destination = root.path().join("destination");
        fs::create_dir(&source).unwrap();
        super::super::store::open(&source.join("kanban.sqlite"))
            .unwrap()
            .close()
            .unwrap();
        let mut nested = source.clone();
        for _ in 0..34 {
            nested.push("d");
            fs::create_dir(&nested).unwrap();
        }
        fs::write(nested.join("synthetic-note"), "preserve").unwrap();
        assert!(import(&source, &destination).is_err());
        assert!(!destination.exists());
        assert_eq!(
            fs::read(nested.join("synthetic-note")).unwrap(),
            b"preserve"
        );
        assert!(!fs::read_dir(root.path()).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".atelier-import-")
        }));
        let args = vec![
            std::ffi::OsString::from("--source"),
            source.into_os_string(),
            std::ffi::OsString::from("--destination"),
            destination.into_os_string(),
        ];
        assert!(arguments(args.clone().into_iter()).is_err());
        let mut approved = args.clone();
        approved.push("--confirm".into());
        assert!(arguments(approved.into_iter()).is_ok());
        let mut unknown = args;
        unknown.extend(["--confirm".into(), "--replace".into()]);
        assert!(arguments(unknown.into_iter()).is_err());
        assert!(
            arguments(
                [
                    "--source",
                    "relative",
                    "--destination",
                    "relative",
                    "--confirm"
                ]
                .into_iter()
                .map(Into::into)
            )
            .is_err()
        );
    }
    use crate::infrastructure::store::{self, Documents};
    use serde_json::json;

    #[test]
    fn incompatible_portable_database_is_refused_without_publishing_or_changing_source() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        let destination = root.path().join("destination");
        fs::create_dir(&source).unwrap();
        let database = source.join("kanban.sqlite");
        let connection = store::open(&database).unwrap();
        connection.pragma_update(None, "application_id", 0).unwrap();
        connection.pragma_update(None, "user_version", 0).unwrap();
        connection.close().unwrap();
        fs::write(source.join("synthetic-note.txt"), "must remain at source").unwrap();
        let before = fs::read(&database).unwrap();
        assert!(import(&source, &destination).is_err());
        assert!(!destination.exists());
        assert_eq!(fs::read(&database).unwrap(), before);
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    }

    #[test]
    fn source_writer_remains_locked_to_other_processes_after_import_refusal() {
        const VARIABLE: &str = "ATELIER_TEST_IMPORT_LOCK_SOURCE";
        if let Some(path) = std::env::var_os(VARIABLE) {
            let connection =
                Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE).unwrap();
            assert!(SourceLock::acquire(connection).is_err());
            return;
        }
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source");
        fs::create_dir(&source).unwrap();
        let path = source.join("kanban.sqlite");
        let writer = store::open(&path).unwrap();
        Store::new(&writer)
            .put(Documents::Cards, &json!({"id":"active"}))
            .unwrap();
        assert!(import(&source, &directory.path().join("destination")).is_err());
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command.args(["--exact", "infrastructure::portable_import::tests::source_writer_remains_locked_to_other_processes_after_import_refusal", "--nocapture"])
            .env(VARIABLE, &path);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        writer.close().unwrap();
        import(&source, &directory.path().join("destination")).unwrap();
    }

    #[test]
    fn exclusive_source_lock_survives_copy_failure_and_releases_readers() {
        struct FullDisk;
        impl std::io::Write for FullDisk {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("synthetic disk full"))
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("source.sqlite");
        let original = store::open(&database).unwrap();
        Store::new(&original)
            .put(Documents::Cards, &json!({"id":"preserved"}))
            .unwrap();
        original.close().unwrap();
        let before = fs::read(&database).unwrap();
        let lock = SourceLock::acquire(
            Connection::open_with_flags(&database, OpenFlags::SQLITE_OPEN_READ_WRITE).unwrap(),
        )
        .unwrap();
        assert!(lock.copy_main(&mut FullDisk).is_err());
        let reader =
            Connection::open_with_flags(&database, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        reader.busy_timeout(std::time::Duration::ZERO).unwrap();
        assert!(reader.prepare("SELECT * FROM cards").is_err());
        drop(lock);
        assert_eq!(
            reader
                .query_row("SELECT count(*) FROM cards", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            1
        );
        reader.close().unwrap();
        assert_eq!(fs::read(&database).unwrap(), before);
    }

    #[test]
    fn rollback_journal_is_refused_without_recovery_or_partial_publication() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source");
        fs::create_dir(&source).unwrap();
        store::open(&source.join("kanban.sqlite"))
            .unwrap()
            .close()
            .unwrap();
        let before = fs::read(source.join("kanban.sqlite")).unwrap();
        fs::write(
            source.join("kanban.sqlite-journal"),
            "synthetic pending recovery",
        )
        .unwrap();
        assert!(import(&source, &directory.path().join("destination")).is_err());
        assert_eq!(fs::read(source.join("kanban.sqlite")).unwrap(), before);
        assert_eq!(
            fs::read(source.join("kanban.sqlite-journal")).unwrap(),
            b"synthetic pending recovery"
        );
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[test]
    fn locked_snapshot_uses_opened_source_after_ancestor_replacement() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source");
        let outside = directory.path().join("outside");
        fs::create_dir(&source).unwrap();
        fs::create_dir(&outside).unwrap();
        let database = source.join("kanban.sqlite");
        let connection = store::open(&database).unwrap();
        Store::new(&connection)
            .put(Documents::Cards, &json!({"id":"selected"}))
            .unwrap();
        connection
            .set_db_config(
                rusqlite::config::DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE,
                true,
            )
            .unwrap();
        connection.close().unwrap();
        fs::write(
            outside.join("kanban.sqlite-wal"),
            "outside must stay untouched",
        )
        .unwrap();
        let opened = Dir::open_ambient_dir(&source, ambient_authority()).unwrap();
        let selected = CheckedFile::open_in(&opened, Path::new("kanban.sqlite")).unwrap();
        let connection =
            Connection::open_with_flags(&database, OpenFlags::SQLITE_OPEN_READ_WRITE).unwrap();
        selected.verify_sqlite(&connection).unwrap();
        let lock = SourceLock::acquire(connection).unwrap();
        #[cfg(unix)]
        {
            fs::rename(&source, directory.path().join("moved")).unwrap();
            std::os::unix::fs::symlink(&outside, &source).unwrap();
        }
        #[cfg(windows)]
        assert!(fs::rename(&source, directory.path().join("moved")).is_err());
        let copied = copy_snapshot(&opened, &lock).unwrap();
        drop(lock);
        let recovered = Connection::open(copied.path().join("kanban.sqlite")).unwrap();
        assert_eq!(
            recovered
                .query_row("SELECT id FROM cards", [], |row| row.get::<_, String>(0))
                .unwrap(),
            "selected"
        );
        recovered.close().unwrap();
        assert_eq!(
            fs::read(outside.join("kanban.sqlite-wal")).unwrap(),
            b"outside must stay untouched"
        );
        assert_eq!(fs::read_dir(outside).unwrap().count(), 1);
    }

    #[test]
    fn import_refuses_a_database_path_from_another_opened_source() {
        let root = tempfile::tempdir().unwrap();
        let root_path = fs::canonicalize(root.path()).unwrap();
        let source = root_path.join("source");
        let other = root_path.join("other");
        for (path, id) in [(&source, "selected"), (&other, "unselected")] {
            fs::create_dir(path).unwrap();
            let connection = store::open(&path.join("kanban.sqlite")).unwrap();
            Store::new(&connection)
                .put(Documents::Cards, &json!({"id":id}))
                .unwrap();
        }
        let selected = Dir::open_ambient_dir(&source, ambient_authority()).unwrap();
        let parent = Dir::open_ambient_dir(&root_path, ambient_authority()).unwrap();
        assert!(
            import_into(
                &other.join("kanban.sqlite"),
                &selected,
                &parent,
                Path::new("destination")
            )
            .is_err()
        );
        #[cfg(unix)]
        {
            fs::rename(&source, root_path.join("moved")).unwrap();
            std::os::unix::fs::symlink(&other, &source).unwrap();
            assert!(
                import_into(
                    &source.join("kanban.sqlite"),
                    &selected,
                    &parent,
                    Path::new("destination")
                )
                .is_err()
            );
            // Even an alias to the selected inode must be refused by SQLite itself.
            std::os::unix::fs::symlink(root_path.join("moved"), root_path.join("alias")).unwrap();
            assert!(
                import_into(
                    &root_path.join("alias/kanban.sqlite"),
                    &selected,
                    &parent,
                    Path::new("destination")
                )
                .is_err()
            );
        }
        assert!(!root_path.join("destination").exists());
        assert!(parent.entries().unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".atelier-import-")
        }));
        let untouched = store::open(&other.join("kanban.sqlite")).unwrap();
        assert_eq!(
            Store::new(&untouched)
                .document::<serde_json::Value>(Documents::Cards, "unselected")
                .unwrap(),
            Some(json!({"id":"unselected"}))
        );
    }

    #[test]
    fn staging_snapshot_and_publication_stay_in_the_opened_destination_parent() {
        let root = tempfile::tempdir().unwrap();
        let source = fs::canonicalize(root.path()).unwrap().join("source");
        let parent = root.path().join("parent");
        let outside = root.path().join("outside");
        for path in [&source, &parent, &outside] {
            fs::create_dir(path).unwrap();
        }
        let database = source.join("kanban.sqlite");
        let connection = store::open(&database).unwrap();
        Store::new(&connection)
            .put(Documents::Cards, &json!({"id":"preserved"}))
            .unwrap();
        connection.close().unwrap();
        fs::write(source.join("attachment"), "synthetic").unwrap();
        fs::write(outside.join("untouched"), "preserved").unwrap();
        let source_directory = Dir::open_ambient_dir(&source, ambient_authority()).unwrap();
        let parent_directory = Dir::open_ambient_dir(&parent, ambient_authority()).unwrap();
        parent_directory.create_dir("data").unwrap();
        #[cfg(unix)]
        let actual = {
            let actual = root.path().join("moved");
            fs::rename(&parent, &actual).unwrap();
            std::os::unix::fs::symlink(&outside, &parent).unwrap();
            actual
        };
        #[cfg(windows)]
        let actual = {
            assert!(fs::rename(&parent, root.path().join("moved")).is_err());
            parent
        };
        import_into(
            &database,
            &source_directory,
            &parent_directory,
            Path::new("data"),
        )
        .unwrap();
        let copied = store::open(&actual.join("data/kanban.sqlite")).unwrap();
        assert_eq!(
            Store::new(&copied)
                .document::<serde_json::Value>(Documents::Cards, "preserved")
                .unwrap(),
            Some(json!({"id":"preserved"}))
        );
        assert_eq!(
            fs::read(actual.join("data/attachment")).unwrap(),
            b"synthetic"
        );
        assert_eq!(
            fs::read_dir(actual.join("data/backups")).unwrap().count(),
            1
        );
        assert!(
            import_into(
                &database,
                &source_directory,
                &parent_directory,
                Path::new("data")
            )
            .is_err()
        );
        fs::write(&database, "corrupt").unwrap();
        assert!(
            import_into(
                &database,
                &source_directory,
                &parent_directory,
                Path::new("failed")
            )
            .is_err()
        );
        assert_eq!(fs::read_dir(actual).unwrap().count(), 1);
        assert_eq!(fs::read_dir(&outside).unwrap().count(), 1);
        assert_eq!(fs::read(outside.join("untouched")).unwrap(), b"preserved");
    }

    #[test]
    fn opened_directories_cannot_be_redirected_during_attachment_copy() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        let target = root.path().join("target");
        let outside = root.path().join("outside");
        for path in [&source, &target, &outside] {
            fs::create_dir(path).unwrap();
        }
        fs::create_dir(source.join("nested é")).unwrap();
        fs::write(source.join("nested é/attachment"), "original").unwrap();
        fs::write(outside.join("private"), "must not copy").unwrap();
        let input = Dir::open_ambient_dir(&source, ambient_authority()).unwrap();
        let output = Dir::open_ambient_dir(&target, ambient_authority()).unwrap();
        #[cfg(unix)]
        let actual_target = {
            use std::os::unix::fs::symlink;
            let moved = root.path().join("moved-target");
            fs::rename(&source, root.path().join("moved-source")).unwrap();
            symlink(&outside, &source).unwrap();
            fs::rename(&target, &moved).unwrap();
            symlink(&outside, &target).unwrap();
            moved
        };
        #[cfg(windows)]
        let actual_target = {
            assert!(fs::rename(&source, root.path().join("moved-source")).is_err());
            assert!(fs::rename(&target, root.path().join("moved-target")).is_err());
            target
        };
        copy_tree(&input, &output, true).unwrap();
        assert_eq!(
            fs::read(actual_target.join("nested é/attachment")).unwrap(),
            b"original"
        );
        assert!(!actual_target.join("private").exists());
        assert_eq!(fs::read_dir(outside).unwrap().count(), 1);
    }

    #[cfg(windows)]
    #[test]
    fn attachment_copy_refuses_windows_junctions() {
        use std::{os::windows::process::CommandExt, process::Command};
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        let target = root.path().join("target");
        let outside = root.path().join("outside");
        for path in [&source, &target, &outside] {
            fs::create_dir(path).unwrap();
        }
        fs::write(outside.join("private"), "must not copy").unwrap();
        let result = Command::new(std::env::var_os("ComSpec").unwrap_or_else(|| "cmd.exe".into()))
            .args(["/D", "/C"])
            .raw_arg("mklink /J \"%ATELIER_TEST_JUNCTION%\" \"%ATELIER_TEST_TARGET%\"")
            .env("ATELIER_TEST_JUNCTION", source.join("escape"))
            .env("ATELIER_TEST_TARGET", &outside)
            .creation_flags(0x08000000)
            .output()
            .unwrap();
        assert!(result.status.success());
        let input = Dir::open_ambient_dir(&source, ambient_authority()).unwrap();
        let output = Dir::open_ambient_dir(&target, ambient_authority()).unwrap();
        assert!(copy_tree(&input, &output, true).is_err());
        assert_eq!(fs::read_dir(&target).unwrap().count(), 0);
        assert_eq!(fs::read(outside.join("private")).unwrap(), b"must not copy");
    }

    #[test]
    fn import_includes_wal_and_attachments_without_sessions_or_overwriting_data() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source é");
        create_directory(&source).unwrap();
        let _lock = crate::infrastructure::data_lock::DataLock::acquire(&source).unwrap();
        let connection = store::open(&source.join("kanban.sqlite")).unwrap();
        let store = Store::new(&connection);
        let card = json!({"id":"preserved", "title":"À conserver 🔑", "extra":null});
        store.put(Documents::Cards, &card).unwrap();
        fs::write(source.join("attachment.txt"), "document").unwrap();
        fs::write(source.join("local-session"), "expired").unwrap();
        fs::write(source.join("local-browser.html"), "expired").unwrap();
        fs::write(
            source.join(super::super::repository_access::FILE),
            "local authorization must not transfer",
        )
        .unwrap();
        let destination = root.path().join("user/data");
        assert!(import(&source, &destination).is_err());
        assert!(!destination.exists());
        // A refused exclusive lock must not disturb the live writer or its transaction state.
        store.put(Documents::Cards, &card).unwrap();
        assert_eq!(
            store
                .document::<serde_json::Value>(Documents::Cards, "preserved")
                .unwrap(),
            Some(card.clone())
        );
        connection
            .set_db_config(
                rusqlite::config::DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE,
                true,
            )
            .unwrap();
        connection.close().unwrap();
        assert!(import(&source, &destination).is_err());
        drop(_lock);
        let originals: Vec<_> = ["kanban.sqlite", "kanban.sqlite-wal", "kanban.sqlite-shm"]
            .into_iter()
            .map(|name| (name, fs::read(source.join(name)).ok()))
            .collect();
        assert!(originals[1].1.as_ref().unwrap().len() > 32);
        // The committed card is in WAL; a main-file-only snapshot cannot satisfy this test.
        let main_only = root.path().join("main-only.sqlite");
        fs::copy(source.join("kanban.sqlite"), &main_only).unwrap();
        let incomplete = Connection::open(&main_only).unwrap();
        assert!(
            Store::new(&incomplete)
                .document::<serde_json::Value>(Documents::Cards, "preserved")
                .unwrap()
                .is_none()
        );
        incomplete.close().unwrap();
        import(&source, &destination).unwrap();
        for (name, bytes) in originals {
            assert_eq!(
                fs::read(source.join(name)).ok(),
                bytes,
                "source changed: {name}"
            );
        }
        let copied = store::open(&destination.join("kanban.sqlite")).unwrap();
        assert_eq!(
            Store::new(&copied)
                .document::<serde_json::Value>(Documents::Cards, "preserved")
                .unwrap(),
            Some(card)
        );
        assert_eq!(
            fs::read_to_string(destination.join("attachment.txt")).unwrap(),
            "document"
        );
        assert!(!destination.join("local-session").exists());
        assert!(!destination.join("local-browser.html").exists());
        assert!(
            !destination
                .join(super::super::repository_access::FILE)
                .exists()
        );
        assert!(
            !destination
                .join(crate::infrastructure::data_lock::LOCK_NAME)
                .exists()
        );
        assert_eq!(
            fs::read_dir(destination.join("backups")).unwrap().count(),
            1
        );
        assert!(import(&source, &destination).is_err());
        for target in [&source, &source.join("nested"), &source.join("..nested")] {
            assert!(import(&source, target).is_err());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&destination).unwrap().permissions().mode() & 0o077,
                0
            );
        }
    }

    #[test]
    fn corrupt_import_and_unexpected_files_leave_no_partial_destination() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        create_directory(&source).unwrap();
        let destination = root.path().join("data");
        fs::write(source.join("kanban.sqlite"), "corrupt").unwrap();
        assert!(import(&source, &destination).is_err());
        assert!(!destination.exists());
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
        fs::remove_file(source.join("kanban.sqlite")).unwrap();
        fs::create_dir(source.join("kanban.sqlite")).unwrap();
        assert!(import(&source, &destination).is_err());
        assert!(!destination.exists());
    }

    #[cfg(unix)]
    #[test]
    fn import_refuses_links_and_resolved_destinations_inside_source() {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        create_directory(&source).unwrap();
        let connection = store::open(&source.join("kanban.sqlite")).unwrap();
        let destination = root.path().join("data");
        symlink(&source, root.path().join("alias")).unwrap();
        assert!(import(&source, &root.path().join("alias/nested")).is_err());
        assert!(!source.join("nested").exists());
        symlink(root.path(), source.join("escape")).unwrap();
        assert!(import(&source, &destination).is_err());
        assert!(!destination.exists());
        drop(connection);
    }
}
