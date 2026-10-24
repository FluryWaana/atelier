use super::{
    checked_file::{CheckedFile, Identity, plain_metadata, require_plain},
    store::Store,
};
use crate::Result;
use cap_fs_ext::DirExt;
use cap_std::{ambient_authority, fs::Dir};
use rusqlite::{Connection, OpenFlags};
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
    require_plain(&source.dir_metadata()?)?;
    require_plain(&destination.dir_metadata()?)?;
    for entry in source.entries()? {
        let entry = entry?;
        let name = entry.file_name();
        if root && name.to_str().is_some_and(super::data_lock::is_lock_file) {
            continue;
        }
        if root && matches!(name.to_str(), Some("local-session" | "local-browser.html")) {
            continue;
        }
        let metadata = source.symlink_metadata(&name)?;
        require_plain(&metadata)?;
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
            copy_tree(
                &child,
                &private_directory(destination, Path::new(&name))?,
                false,
            )?;
        } else if metadata.is_file() {
            let checked = CheckedFile::open_in(source, Path::new(&name))?;
            let mut options = cap_std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use cap_std::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut output = destination.open_with(&name, &options)?;
            checked.copy_to(&mut output)?;
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

fn import_into(database: &Path, source: &Dir, parent: &Dir, destination: &Path) -> Result<()> {
    require_empty_in(parent, destination)?;
    let stage_name = format!(".atelier-import-{}", uuid::Uuid::new_v4());
    let stage = private_directory(parent, Path::new(&stage_name))?;
    let prepared = (|| -> Result<()> {
        copy_tree(source, &stage, true)?;
        // shortcut: source SQLite still opens a path; qualify its ancestor races in M02 before real-data use.
        let identity = Identity::inspect(database)?;
        let connection = Connection::open_with_flags(database, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let _checked = CheckedFile::open(database, &identity)?;
        let integrity: String = connection.query_row("PRAGMA quick_check", [], |row| row.get(0))?;
        if integrity != "ok" {
            return Err("La base portable est endommagée.".into());
        }
        for table in ["cards", "projects", "settings"] {
            connection.prepare(&format!("SELECT * FROM {table} LIMIT 0"))?;
        }
        // SQLite includes committed WAL; copy the finished snapshot without reopening its path.
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
    use crate::infrastructure::store::{self, Documents};
    use serde_json::json;

    #[test]
    fn staging_snapshot_and_publication_stay_in_the_opened_destination_parent() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
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
        drop(connection);
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
        let destination = root.path().join("user/data");
        import(&source, &destination).unwrap();
        assert_eq!(
            store
                .document::<serde_json::Value>(Documents::Cards, "preserved")
                .unwrap(),
            Some(card.clone())
        );
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
