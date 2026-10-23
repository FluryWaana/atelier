use crate::Result;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
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
    fn table(self) -> &'static str {
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

    pub fn documents(&self, table: Documents) -> Result<Vec<Value>> {
        let mut statement = self.connection.prepare(&format!(
            "SELECT data FROM {} ORDER BY rowid DESC",
            table.table()
        ))?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        rows.map(|row| Ok(serde_json::from_str(&row?)?)).collect()
    }

    pub fn document(&self, table: Documents, id: &str) -> Result<Option<Value>> {
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

    pub fn put(&self, table: Documents, document: &Value) -> Result<()> {
        let id = document
            .get("id")
            .and_then(Value::as_str)
            .ok_or("Identifiant de document invalide.")?;
        self.connection.execute(&format!("INSERT INTO {}(id,data) VALUES (?,?) ON CONFLICT(id) DO UPDATE SET data=excluded.data", table.table()), params![id, serde_json::to_string(document)?])?;
        Ok(())
    }

    pub fn delete(&self, table: Documents, id: &str) -> Result<()> {
        self.connection
            .execute(&format!("DELETE FROM {} WHERE id=?", table.table()), [id])?;
        Ok(())
    }

    pub fn settings(&self) -> Result<Value> {
        let data: Option<String> = self
            .connection
            .query_row("SELECT data FROM settings WHERE id=1", [], |row| row.get(0))
            .optional()?;
        match data {
            Some(data) => Ok(serde_json::from_str(&data)?),
            None => Ok(json!({})),
        }
    }

    pub fn save_settings(&self, settings: &Value) -> Result<()> {
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
        let temporary = tempfile::NamedTempFile::new_in(parent)?;
        let mut copy = Connection::open(temporary.path())?;
        {
            let backup = rusqlite::backup::Backup::new(self.connection, &mut copy)?;
            backup.run_to_completion(100, Duration::from_millis(10), None)?;
        }
        copy.close().map_err(|(_, error)| error)?;
        temporary.as_file().sync_all()?;
        temporary.persist_noclobber(destination)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
                store.documents(table).unwrap(),
                vec![json!({"id":"two"}), first]
            );
            assert_eq!(store.document(table, "missing").unwrap(), None);
        }
        assert_eq!(store.settings().unwrap(), json!({}));
        let failed: Result<()> = store.transaction(|store| {
            store.delete(Documents::Cards, "one")?;
            store.save_settings(&json!({"unexpected":true}))?;
            Err("Injected failure".into())
        });
        assert!(failed.is_err());
        assert!(store.document(Documents::Cards, "one").unwrap().is_some());
        assert_eq!(store.settings().unwrap(), json!({}));
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
        store.delete(Documents::Cards, "one").unwrap();
        let copy = open(&saved).unwrap();
        let saved = Store::new(&copy);
        assert!(saved.document(Documents::Cards, "one").unwrap().is_some());
        assert_eq!(saved.logs("run", 0).unwrap(), page);
        assert_eq!(saved.settings().unwrap(), json!({"extra":null}));
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
        assert_eq!(store.settings().unwrap(), json!({}));
        assert!(store.documents(Documents::Cards).unwrap().is_empty());
        connection
            .execute("INSERT INTO cards VALUES ('bad', '{')", [])
            .unwrap();
        assert!(store.documents(Documents::Cards).is_err());
        assert!(store.document(Documents::Cards, "bad").is_err());
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
