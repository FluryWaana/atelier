use super::{
    checked_file::{CheckedFile, Identity, plain_metadata},
    store::Store,
};
use crate::Result;
use rusqlite::{Connection, OpenFlags};
use std::{
    fs::{self, OpenOptions},
    path::Path,
};

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

fn copy_tree(source: &Path, destination: &Path, root: bool) -> Result<()> {
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let name = entry.file_name();
        if root && name.to_str().is_some_and(super::data_lock::is_lock_file) {
            continue;
        }
        if root && matches!(name.to_str(), Some("local-session" | "local-browser.html")) {
            continue;
        }
        let source = entry.path();
        let metadata = plain_metadata(&source)?;
        if root
            && matches!(
                name.to_str(),
                Some("kanban.sqlite" | "kanban.sqlite-wal" | "kanban.sqlite-shm")
            )
        {
            continue;
        }
        let destination = destination.join(name);
        if metadata.is_dir() {
            create_directory(&destination)?;
            copy_tree(&source, &destination, false)?;
        } else if metadata.is_file() {
            let identity = Identity::inspect(&source)?;
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut output = options.open(&destination)?;
            CheckedFile::open(&source, &identity)?.copy_to(&mut output)?;
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
    let mut builder = tempfile::Builder::new();
    builder.prefix(".atelier-import-");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(fs::Permissions::from_mode(0o700));
    }
    let stage = builder.tempdir_in(&parent)?;
    copy_tree(&source, stage.path(), true)?;
    let identity = Identity::inspect(&database)?;
    let connection = Connection::open_with_flags(&database, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let _checked = CheckedFile::open(&database, &identity)?;
    let integrity: String = connection.query_row("PRAGMA quick_check", [], |row| row.get(0))?;
    if integrity != "ok" {
        return Err("La base portable est endommagée.".into());
    }
    for table in ["cards", "projects", "settings"] {
        connection.prepare(&format!("SELECT * FROM {table} LIMIT 0"))?;
    }
    // Snapshot through SQLite includes committed WAL; the source stays open read-only.
    Store::new(&connection).backup(&stage.path().join("kanban.sqlite"))?;
    connection.close().map_err(|(_, error)| error)?;
    let backups = stage.path().join("backups");
    create_directory(&backups)?;
    let name = super::backups::new_name()?;
    let copied = Connection::open_with_flags(
        stage.path().join("kanban.sqlite"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;
    Store::new(&copied).backup(&backups.join(name))?;
    copied.close().map_err(|(_, error)| error)?;
    require_empty(&destination)?;
    if destination.exists() {
        fs::remove_dir(&destination)?;
    }
    fs::rename(stage.path(), &destination)?;
    Ok(())
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
    use crate::infrastructure::store::{self, Documents};
    use serde_json::json;

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
        let destination = root.path().join("user/data");
        import(&source, &destination).unwrap();
        assert_eq!(
            store.document(Documents::Cards, "preserved").unwrap(),
            Some(card.clone())
        );
        let copied = store::open(&destination.join("kanban.sqlite")).unwrap();
        assert_eq!(
            Store::new(&copied)
                .document(Documents::Cards, "preserved")
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
