use crate::Result;
use atelier_core::document::{Document, JsonString};
use rusqlite::{Connection, OptionalExtension, params, types::Value};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::value::RawValue;
use std::{path::Path, time::Duration};

const SCHEMA: &str = "PRAGMA journal_mode=WAL;
    CREATE TABLE IF NOT EXISTS time_entries (id TEXT PRIMARY KEY, data TEXT NOT NULL);
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

pub(super) fn open(path: &Path) -> Result<Connection> {
    let connection = Connection::open(path)?;
    connection.execute_batch(SCHEMA)?;
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
        let mut temporary = tempfile::NamedTempFile::new()?;
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
