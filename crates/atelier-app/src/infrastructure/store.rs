use crate::Result;
use atelier_core::document::{Document, JsonString};
use rusqlite::{Connection, OptionalExtension, params, types::Value};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::value::RawValue;
use std::{path::Path, time::Duration};

const APPLICATION_ID: i32 = 0x41544b52;
const FORMAT_VERSION: i32 = 1;
const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS time_entries (id TEXT PRIMARY KEY, data TEXT NOT NULL);
    CREATE TABLE IF NOT EXISTS projects (id TEXT PRIMARY KEY, data TEXT NOT NULL);
    CREATE TABLE IF NOT EXISTS cards (id TEXT PRIMARY KEY, data TEXT NOT NULL);
    CREATE TABLE IF NOT EXISTS runs (id TEXT PRIMARY KEY, data TEXT NOT NULL);
    CREATE TABLE IF NOT EXISTS logs (seq INTEGER PRIMARY KEY AUTOINCREMENT, runId TEXT NOT NULL, stream TEXT NOT NULL, text TEXT NOT NULL);
    CREATE INDEX IF NOT EXISTS logs_run ON logs(runId, seq);
    CREATE TABLE IF NOT EXISTS settings (id INTEGER PRIMARY KEY CHECK(id=1), data TEXT NOT NULL);";

#[derive(Clone, Copy, Debug)]
pub enum Documents {
    Projects,
    Cards,
    Runs,
    Times,
}

impl Documents {
    pub(super) fn table(self) -> &'static str {
        match self {
            Self::Projects => "projects",
            Self::Cards => "cards",
            Self::Runs => "runs",
            Self::Times => "time_entries",
        }
    }
}

#[derive(Debug, Deserialize, PartialEq, Serialize)]
pub struct Log {
    pub seq: i64,
    pub stream: String,
    pub text: String,
}

pub struct Store<'a> {
    pub(crate) connection: &'a Connection,
}

pub(super) fn temporary_directory() -> Result<tempfile::TempDir> {
    let mut builder = tempfile::Builder::new();
    builder.prefix("atelier-sqlite-");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(std::fs::Permissions::from_mode(0o700));
    }
    Ok(builder.tempdir()?)
}

pub(super) fn open(path: &Path) -> Result<Connection> {
    let connection = Connection::open(path)?;
    initialize(&connection)?;
    Ok(connection)
}

fn identity(connection: &Connection) -> Result<(i32, i32)> {
    Ok((
        connection.pragma_query_value(None, "application_id", |row| row.get(0))?,
        connection.pragma_query_value(None, "user_version", |row| row.get(0))?,
    ))
}

pub(super) fn validate_format(connection: &Connection) -> Result<()> {
    if identity(connection)? != (APPLICATION_ID, FORMAT_VERSION) {
        return Err("Base incompatible : ce format n’est pas pris en charge par Atelier Rust. Utilisez un nouveau dossier ou une sauvegarde Rust compatible.".into());
    }
    for query in [
        "SELECT id,data FROM cards LIMIT 0",
        "SELECT id,data FROM projects LIMIT 0",
        "SELECT id,data FROM runs LIMIT 0",
        "SELECT id,data FROM time_entries LIMIT 0",
        "SELECT id,data FROM settings LIMIT 0",
        "SELECT seq,runId,stream,text FROM logs LIMIT 0",
    ] {
        connection.prepare(query)?;
    }
    Ok(())
}

fn initialize(connection: &Connection) -> Result<()> {
    if identity(connection)? == (0, 0) {
        let objects: i64 = connection.query_row(
            "SELECT count(*) FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%'",
            [],
            |row| row.get(0),
        )?;
        if objects == 0 {
            let transaction = rusqlite::Transaction::new_unchecked(
                connection,
                rusqlite::TransactionBehavior::Immediate,
            )?;
            // Recheck after the write lock: another opener may have initialized the empty file.
            if identity(&transaction)? == (0, 0) {
                let objects: i64 = transaction.query_row(
                    "SELECT count(*) FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%'",
                    [],
                    |row| row.get(0),
                )?;
                if objects != 0 {
                    return Err("Le fichier SQLite a été modifié pendant son ouverture.".into());
                }
                transaction.execute_batch(SCHEMA)?;
                transaction.pragma_update(None, "application_id", APPLICATION_ID)?;
                transaction.pragma_update(None, "user_version", FORMAT_VERSION)?;
            }
            validate_format(&transaction)?;
            transaction.commit()?;
        }
    }
    validate_format(connection)?;
    connection.execute_batch("PRAGMA journal_mode=WAL;")?;
    Ok(())
}

pub(super) fn open_checked(path: &Path, directory: &cap_std::fs::Dir) -> Result<Connection> {
    let name = Path::new(path.file_name().ok_or("Nom de base SQLite absent.")?);
    let mut options = cap_std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use cap_std::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    match directory.open_with(name, &options) {
        Ok(file) => drop(file),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.into()),
    }
    #[cfg(windows)]
    let selected = super::checked_file::CheckedFile::open_in(directory, name)?;
    #[cfg(unix)]
    let selected = super::checked_file::Identity::sqlite_source(directory, name)?;
    // Creation is anchored above: SQLite may only open the selected existing main file.
    let connection = Connection::open_with_flags(
        path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE | rusqlite::OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )?;
    selected.verify_sqlite(&connection)?;
    Ok(connection)
}

pub(super) fn open_in(path: &Path, directory: &cap_std::fs::Dir) -> Result<Connection> {
    let connection = open_checked(path, directory)?;
    initialize(&connection)?;
    Ok(connection)
}

impl<'a> Store<'a> {
    pub(super) fn new(connection: &'a Connection) -> Self {
        Self { connection }
    }

    pub fn documents<T: DeserializeOwned>(&self, table: Documents) -> Result<Vec<T>> {
        let mut statement = self.connection.prepare(&format!(
            "SELECT data FROM {} ORDER BY rowid DESC",
            table.table()
        ))?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        rows.map(|row| Ok(serde_json::from_str(&row?)?)).collect()
    }

    pub fn document<T: DeserializeOwned>(&self, table: Documents, id: &str) -> Result<Option<T>> {
        let data: Option<String> = self
            .connection
            .query_row(
                &format!("SELECT data FROM {} WHERE id=?", table.table()),
                [id],
                |row| row.get(0),
            )
            .optional()?;
        data.map(|data| serde_json::from_str(&data).map_err(Into::into))
            .transpose()
    }

    pub(crate) fn document_input<T: DeserializeOwned>(
        &self,
        table: Documents,
        id: Option<&RawValue>,
    ) -> Result<Option<T>> {
        // node:sqlite binds JS numbers as REAL, booleans as INTEGER and objects as named parameters.
        let id = id.ok_or("Provided value cannot be bound to SQLite parameter 1.")?;
        let parameter = match id.get().as_bytes()[0] {
            b'"' => Value::Text(String::from_utf16_lossy(
                &serde_json::from_str::<JsonString>(id.get())?.0,
            )),
            b'n' => Value::Null,
            b't' => Value::Integer(1),
            b'f' => Value::Integer(0),
            b'{' => {
                let object: Document = serde_json::from_str(id.get())?;
                if let Some(key) = object.keys().next() {
                    return Err(format!(
                        "Unknown named parameter '{}'",
                        String::from_utf16_lossy(&key.0)
                    )
                    .into());
                }
                Value::Null
            }
            b'[' => {
                if !serde_json::from_str::<Vec<Box<RawValue>>>(id.get())?.is_empty() {
                    return Err("Unknown named parameter '0'".into());
                }
                Value::Null
            }
            _ => Value::Real(serde_json::from_str(id.get())?),
        };
        let data: Option<String> = self
            .connection
            .query_row(
                &format!("SELECT data FROM {} WHERE id=?", table.table()),
                [parameter],
                |row| row.get(0),
            )
            .optional()?;
        data.map(|data| serde_json::from_str(&data).map_err(Into::into))
            .transpose()
    }
    pub fn put(&self, table: Documents, document: &impl Serialize) -> Result<()> {
        let encoded = serde_json::to_string(document)?;
        let document: Document =
            serde_json::from_str(&encoded).map_err(|_| "Identifiant de document invalide.")?;
        let id = document
            .get::<String>("id")?
            .ok_or("Identifiant de document invalide.")?;
        self.connection.execute(&format!("INSERT INTO {}(id,data) VALUES (?,?) ON CONFLICT(id) DO UPDATE SET data=excluded.data", table.table()), params![id, encoded])?;
        Ok(())
    }

    pub fn delete(&self, table: Documents, id: &str) -> Result<()> {
        self.connection
            .execute(&format!("DELETE FROM {} WHERE id=?", table.table()), [id])?;
        Ok(())
    }

    pub fn settings<T: DeserializeOwned>(&self) -> Result<T> {
        let data: Option<String> = self
            .connection
            .query_row("SELECT data FROM settings WHERE id=1", [], |row| row.get(0))
            .optional()?;
        match data {
            Some(data) => Ok(serde_json::from_str(&data)?),
            None => Ok(serde_json::from_str("{}")?),
        }
    }

    pub fn save_settings(&self, settings: &impl Serialize) -> Result<()> {
        self.connection.execute(
            "INSERT INTO settings VALUES (1,?) ON CONFLICT(id) DO UPDATE SET data=excluded.data",
            [serde_json::to_string(settings)?],
        )?;
        Ok(())
    }

    pub fn log(&self, id: &str, stream: &str, text: &str) -> Result<()> {
        // Node slices UTF-16, then SQLite receives UTF-8 with replacement for a split surrogate.
        let text = String::from_utf16_lossy(&text.encode_utf16().take(65_536).collect::<Vec<_>>());
        self.connection.execute(
            "INSERT INTO logs(runId,stream,text) VALUES (?,?,?)",
            params![id, stream, text],
        )?;
        Ok(())
    }

    pub fn logs(&self, id: &str, after: i64) -> Result<Vec<Log>> {
        let mut statement = self.connection.prepare(
            "SELECT seq,stream,text FROM logs WHERE runId=? AND seq>? ORDER BY seq LIMIT 500",
        )?;
        Ok(statement
            .query_map(params![id, after], |row| {
                Ok(Log {
                    seq: row.get(0)?,
                    stream: row.get(1)?,
                    text: row.get(2)?,
                })
            })?
            .collect::<std::result::Result<_, _>>()?)
    }

    pub fn transaction<T>(&self, change: impl FnOnce(&Self) -> Result<T>) -> Result<T> {
        let transaction = self.connection.unchecked_transaction()?;
        let result = change(self)?;
        transaction.commit()?;
        Ok(result)
    }

    pub fn backup(&self, destination: &Path) -> Result<()> {
        let parent = destination
            .parent()
            .ok_or("Dossier de sauvegarde absent.")?;
        let directory = cap_std::fs::Dir::open_ambient_dir(parent, cap_std::ambient_authority())?;
        self.backup_in(
            &directory,
            Path::new(destination.file_name().ok_or("Nom de sauvegarde absent.")?),
        )
    }

    pub(super) fn backup_in(&self, directory: &cap_std::fs::Dir, name: &Path) -> Result<()> {
        // SQLite's adjacent journals must be protected along with the temporary database.
        let private = temporary_directory()?;
        let mut temporary = tempfile::NamedTempFile::new_in(private.path())?;
        let mut copy = Connection::open(temporary.path())?;
        {
            let backup = rusqlite::backup::Backup::new(self.connection, &mut copy)?;
            backup.run_to_completion(100, Duration::from_millis(10), None)?;
        }
        copy.close().map_err(|(_, error)| error)?;
        let stage = format!(".atelier-backup-{}", uuid::Uuid::new_v4());
        let mut options = cap_std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use cap_std::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut output = directory.open_with(&stage, &options)?;
        let result = (|| -> Result<()> {
            std::io::copy(temporary.as_file_mut(), &mut output)?;
            output.sync_all()?;
            drop(output);
            super::publish::rename_new(directory, Path::new(&stage), name)
        })();
        if result.is_err() {
            let _ = directory.remove_file(&stage);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn new_database_has_a_rust_identity_and_reopens_with_its_content() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("synthetic.sqlite");
        let connection = open(&path).unwrap();
        assert_eq!(
            identity(&connection).unwrap(),
            (APPLICATION_ID, FORMAT_VERSION)
        );
        Store::new(&connection)
            .put(Documents::Cards, &json!({"id":"kept", "title":"Synthetic"}))
            .unwrap();
        connection.close().unwrap();
        let connection = open(&path).unwrap();
        assert_eq!(
            Store::new(&connection)
                .document::<serde_json::Value>(Documents::Cards, "kept")
                .unwrap()
                .unwrap()["title"],
            "Synthetic"
        );
    }

    #[test]
    fn unidentified_foreign_future_and_incomplete_databases_are_refused_without_writes() {
        for (application, version, schema) in [
            (0, 0, SCHEMA),
            (42, 1, SCHEMA),
            (APPLICATION_ID, 2, SCHEMA),
            (APPLICATION_ID, 1, "CREATE TABLE unrelated(value);"),
        ] {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("synthetic.sqlite");
            let connection = Connection::open(&path).unwrap();
            connection.execute_batch(schema).unwrap();
            connection
                .pragma_update(None, "application_id", application)
                .unwrap();
            connection
                .pragma_update(None, "user_version", version)
                .unwrap();
            connection.close().unwrap();
            let before = std::fs::read(&path).unwrap();
            let root =
                cap_std::fs::Dir::open_ambient_dir(directory.path(), cap_std::ambient_authority())
                    .unwrap();
            assert!(open_in(&path, &root).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), before);
            assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
        }
    }

    #[test]
    fn checked_database_rejects_another_root_before_schema_or_journal_writes() {
        use cap_std::{ambient_authority, fs::Dir};
        let root = tempfile::tempdir().unwrap();
        let selected = root.path().join("selected");
        let outside = root.path().join("outside");
        std::fs::create_dir(&selected).unwrap();
        std::fs::create_dir(&outside).unwrap();
        let foreign = outside.join("kanban.sqlite");
        let connection = Connection::open(&foreign).unwrap();
        connection
            .execute_batch("CREATE TABLE untouched(value); INSERT INTO untouched VALUES(42)")
            .unwrap();
        connection.close().unwrap();
        let before = std::fs::read(&foreign).unwrap();
        let directory = Dir::open_ambient_dir(&selected, ambient_authority()).unwrap();
        assert!(open_in(&foreign, &directory).is_err());
        assert_eq!(std::fs::read(&foreign).unwrap(), before);
        assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 1);
        open_in(&selected.join("kanban.sqlite"), &directory)
            .unwrap()
            .close()
            .unwrap();
        #[cfg(unix)]
        {
            std::fs::rename(&selected, root.path().join("moved")).unwrap();
            std::os::unix::fs::symlink(&outside, &selected).unwrap();
            assert!(open_in(&selected.join("kanban.sqlite"), &directory).is_err());
            assert_eq!(std::fs::read(&foreign).unwrap(), before);
        }
        #[cfg(windows)]
        assert!(std::fs::rename(&selected, root.path().join("moved")).is_err());
    }

    #[test]
    fn temporary_sqlite_sidecars_stay_inside_a_private_directory() {
        let private = temporary_directory().unwrap();
        let path = private.path().to_owned();
        let temporary = tempfile::NamedTempFile::new_in(&path).unwrap();
        let connection = open(temporary.path()).unwrap();
        Store::new(&connection)
            .put(Documents::Cards, &json!({"id":"private"}))
            .unwrap();
        let wal = std::path::PathBuf::from(format!("{}-wal", temporary.path().display()));
        assert!(wal.is_file());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o700
            );
            for file in [temporary.path(), wal.as_path()] {
                assert_eq!(
                    std::fs::metadata(file).unwrap().permissions().mode() & 0o077,
                    0
                );
            }
        }
        connection.close().unwrap();
        drop(temporary);
        drop(private);
        assert!(!path.exists());
    }

    #[test]
    fn schema_transactions_and_backups_preserve_documents_and_sequence() {
        let directory = tempfile::tempdir().unwrap();
        let connection = open(&directory.path().join("live.sqlite")).unwrap();
        let store = Store::new(&connection);
        for table in [
            Documents::Projects,
            Documents::Cards,
            Documents::Runs,
            Documents::Times,
        ] {
            let first = json!({"id":"one", "extra":{"number":1.23456789012345, "null":null, "unicode":"é 🔧"}});
            store.put(table, &first).unwrap();
            store.put(table, &json!({"id":"two"})).unwrap();
            store.put(table, &first).unwrap();
            assert_eq!(
                store.documents::<serde_json::Value>(table).unwrap(),
                vec![json!({"id":"two"}), first]
            );
            assert_eq!(
                store
                    .document::<serde_json::Value>(table, "missing")
                    .unwrap(),
                None
            );
        }
        assert_eq!(store.settings::<serde_json::Value>().unwrap(), json!({}));
        let failed: Result<()> = store.transaction(|store| {
            store.delete(Documents::Cards, "one")?;
            store.save_settings(&json!({"unexpected":true}))?;
            Err("Injected failure".into())
        });
        assert!(failed.is_err());
        assert!(
            store
                .document::<serde_json::Value>(Documents::Cards, "one")
                .unwrap()
                .is_some()
        );
        assert_eq!(store.settings::<serde_json::Value>().unwrap(), json!({}));
        store
            .transaction(|store| {
                store.save_settings(&json!({"extra":null}))?;
                for i in 0..502 {
                    store.log("run", "stdout", &i.to_string())?;
                }
                store.log("other", "stderr", "excluded")?;
                Ok(())
            })
            .unwrap();
        let page = store.logs("run", 0).unwrap();
        assert_eq!(page.len(), 500);
        assert_eq!(
            store.logs("run", page.last().unwrap().seq).unwrap().len(),
            2
        );
        let saved = directory.path().join("snapshot.sqlite");
        store.backup(&saved).unwrap();
        assert!(store.backup(&saved).is_err());
        assert!(std::fs::read_dir(directory.path()).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".atelier-backup-")
        }));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&saved).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        store.delete(Documents::Cards, "one").unwrap();
        let copy = open(&saved).unwrap();
        let saved = Store::new(&copy);
        assert!(
            saved
                .document::<serde_json::Value>(Documents::Cards, "one")
                .unwrap()
                .is_some()
        );
        assert_eq!(saved.logs("run", 0).unwrap(), page);
        assert_eq!(
            saved.settings::<serde_json::Value>().unwrap(),
            json!({"extra":null})
        );
    }

    #[test]
    fn corrupt_storage_and_full_disk_are_errors_without_partial_transactions() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("bad.sqlite");
        std::fs::write(&path, b"not SQLite").unwrap();
        assert!(open(&path).is_err());
        let connection = open(Path::new(":memory:")).unwrap();
        let store = Store::new(&connection);
        let pages: i64 = connection
            .query_row("PRAGMA page_count", [], |row| row.get(0))
            .unwrap();
        connection
            .pragma_update(None, "max_page_count", pages)
            .unwrap();
        assert!(
            store
                .transaction(|store| {
                    store.save_settings(&json!({"changed":true}))?;
                    store.put(
                        Documents::Cards,
                        &json!({"id":"too-large", "value":"x".repeat(1_000_000)}),
                    )
                })
                .is_err()
        );
        assert_eq!(store.settings::<serde_json::Value>().unwrap(), json!({}));
        assert!(
            store
                .documents::<serde_json::Value>(Documents::Cards)
                .unwrap()
                .is_empty()
        );
        connection
            .execute("INSERT INTO cards VALUES ('bad', '{')", [])
            .unwrap();
        assert!(
            store
                .documents::<serde_json::Value>(Documents::Cards)
                .is_err()
        );
        assert!(
            store
                .document::<serde_json::Value>(Documents::Cards, "bad")
                .is_err()
        );
    }

    #[test]
    fn logs_match_javascript_utf16_slice_boundaries() {
        let connection = open(Path::new(":memory:")).unwrap();
        let store = Store::new(&connection);
        store
            .log("run", "stdout", &format!("{}🔧tail", "a".repeat(65_535)))
            .unwrap();
        let text = &store.logs("run", 0).unwrap()[0].text;
        assert_eq!(text.encode_utf16().count(), 65_536);
        assert!(text.ends_with('\u{fffd}'));
    }
}
