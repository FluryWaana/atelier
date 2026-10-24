use super::{
    checked_file::{CheckedFile, require_plain},
    store::{Documents, Store},
};
use crate::Result;
use atelier_core::{application::backups::BackupFailure, document::Document};
use cap_fs_ext::DirExt;
use cap_std::{ambient_authority, fs::Dir};
use rusqlite::{Connection, OpenFlags, params};
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Serialize)]
pub struct Backup {
    pub name: String,
    pub date: String,
    pub bytes: u64,
}

#[derive(Serialize)]
pub struct CreatedBackup {
    pub name: String,
    pub directory: PathBuf,
}

#[derive(Serialize)]
pub struct RestoredBackup {
    pub restored: String,
    pub rescue: String,
}

pub struct BackupDirectory {
    pub directory: PathBuf,
    handle: Dir,
}

pub(super) fn new_name() -> Result<String> {
    Ok(format!(
        "atelier-{}-{}.sqlite",
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis(),
        uuid::Uuid::new_v4()
    ))
}

fn valid_name(name: &str) -> bool {
    let Some(value) = name
        .strip_prefix("atelier-")
        .and_then(|name| name.strip_suffix(".sqlite"))
    else {
        return false;
    };
    let bytes = value.as_bytes();
    bytes.len() == 50
        && bytes[..13].iter().all(u8::is_ascii_digit)
        && bytes[13] == b'-'
        && bytes[14..]
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte) || *byte == b'-')
}

impl BackupDirectory {
    pub fn open(data_directory: &Path) -> Result<Self> {
        fs::create_dir_all(data_directory)?;
        let root = Dir::open_ambient_dir(data_directory, ambient_authority())?;
        Self::open_in(data_directory, &root)
    }

    pub(super) fn open_in(data_directory: &Path, root: &Dir) -> Result<Self> {
        let directory = data_directory.join("backups");
        if let Err(error) = root.create_dir("backups")
            && error.kind() != std::io::ErrorKind::AlreadyExists
        {
            return Err(error.into());
        }
        let handle = root.open_dir_nofollow("backups")?;
        require_plain(&handle.dir_metadata()?)?;
        Ok(Self { directory, handle })
    }

    pub fn list(&self) -> Result<Vec<Backup>> {
        let mut backups = Vec::new();
        for entry in self.handle.entries()? {
            let entry = entry?;
            let Some(name) = entry
                .file_name()
                .to_str()
                .map(str::to_owned)
                .filter(|name| valid_name(name))
            else {
                continue;
            };
            let metadata = self.handle.symlink_metadata(&name)?;
            if !metadata.is_file() || require_plain(&metadata).is_err() {
                continue;
            }
            let date = time::OffsetDateTime::from(metadata.modified()?.into_std()).format(
                time::macros::format_description!(
                    "[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z"
                ),
            )?;
            backups.push(Backup {
                name,
                date,
                bytes: metadata.len(),
            });
        }
        backups.sort_by(|left, right| right.name.cmp(&left.name));
        Ok(backups)
    }

    pub fn create(&self, store: &Store<'_>) -> Result<CreatedBackup> {
        let name = new_name()?;
        store.backup_in(&self.handle, Path::new(&name))?;
        Ok(CreatedBackup {
            name,
            directory: self.directory.clone(),
        })
    }

    pub fn restore(&self, store: &Store<'_>, name: &str) -> Result<RestoredBackup> {
        self.restore_with(store, name, |_| Ok(()))
    }

    pub fn restore_with(
        &self,
        store: &Store<'_>,
        name: &str,
        after: impl FnOnce(&Store<'_>) -> Result<()>,
    ) -> Result<RestoredBackup> {
        if !valid_name(name) || !self.list()?.iter().any(|backup| backup.name == name) {
            return Err(BackupFailure::Invalid("Sauvegarde locale introuvable.").into());
        }
        let rescue = new_name()?;
        restore_checked(
            store,
            CheckedFile::open_in(&self.handle, Path::new(name))?,
            || store.backup_in(&self.handle, Path::new(&rescue)),
            after,
        )?;
        Ok(RestoredBackup {
            restored: name.into(),
            rescue,
        })
    }
}

struct SavedDocument {
    id: String,
    data: String,
    value: Document,
}

fn documents(source: &Connection, table: Documents) -> Result<Vec<SavedDocument>> {
    let mut statement = source.prepare(&format!(
        "SELECT id,data FROM {} ORDER BY rowid",
        table.table()
    ))?;
    let rows = statement.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    rows.map(|row| {
        let (id, data) = row?;
        let value = serde_json::from_str(&data)?;
        Ok(SavedDocument { id, data, value })
    })
    .collect()
}

pub fn restore(store: &Store<'_>, source: &Path, rescue: &Path) -> Result<()> {
    let directory = Dir::open_ambient_dir(
        source.parent().ok_or("Dossier de sauvegarde absent.")?,
        ambient_authority(),
    )?;
    let source = CheckedFile::open_in(
        &directory,
        Path::new(source.file_name().ok_or("Nom de sauvegarde absent.")?),
    )?;
    restore_checked(store, source, || store.backup(rescue), |_| Ok(()))
}

fn restore_checked(
    store: &Store<'_>,
    checked: CheckedFile,
    rescue: impl FnOnce() -> Result<()>,
    after: impl FnOnce(&Store<'_>) -> Result<()>,
) -> Result<()> {
    let private = super::store::temporary_directory()?;
    let mut snapshot = tempfile::NamedTempFile::new_in(private.path())?;
    checked.copy_to(snapshot.as_file_mut())?;
    let source = Connection::open_with_flags(snapshot.path(), OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    super::store::validate_format(&source).map_err(|_| {
        BackupFailure::Invalid("Sauvegarde incompatible avec cette version d’Atelier Rust.")
    })?;
    let integrity: String = source.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
    if integrity != "ok" {
        return Err(BackupFailure::Invalid("Sauvegarde endommagée.").into());
    }
    let projects = documents(&source, Documents::Projects)?;
    let cards = documents(&source, Documents::Cards)?;
    let runs = documents(&source, Documents::Runs)?;
    let entries = documents(&source, Documents::Times)?;
    let times: HashMap<_, _> = entries
        .iter()
        .map(|row| (row.id.as_str(), &row.value))
        .collect();
    for current in store.documents::<Document>(Documents::Times)? {
        if current.string_equals("exportState", "draft")? {
            continue;
        }
        let id = current.get::<String>("id")?;
        let saved = id.as_deref().and_then(|id| times.get(id));
        let same = if let Some(saved) = saved {
            saved.get::<Value>("exportState")? == current.get::<Value>("exportState")?
                && saved.get::<Value>("tempoId")? == current.get::<Value>("tempoId")?
                && saved.string_equals("exportState", "exported")?
        } else {
            false
        };
        if !same {
            return Err(BackupFailure::Invalid("Restauration refusée : des envois Tempo plus récents ou incertains doivent être conservés. Choisissez une sauvegarde récente.").into());
        }
    }
    let unfinished = "Cette sauvegarde contient des opérations inachevées.";
    for run in &runs {
        if run.value.string_equals("status", "running")? {
            return Err(BackupFailure::Invalid(unfinished).into());
        }
    }
    for card in &cards {
        if let Some(raw) = card.value.raw("automation")
            && Document::spread(raw)?.string_equals("state", "running")?
        {
            return Err(BackupFailure::Invalid(unfinished).into());
        }
    }
    for time in times.values() {
        if time.truthy("runningSince")?
            || time.string_equals("exportState", "sending")?
            || time.string_equals("exportState", "uncertain")?
        {
            return Err(BackupFailure::Invalid(unfinished).into());
        }
    }
    let mut statement = source.prepare("SELECT seq,runId,stream,text FROM logs ORDER BY seq")?;
    let logs = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let mut statement = source.prepare("SELECT id,data FROM settings")?;
    let settings = statement
        .query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    rescue()?;
    store.transaction(|store| {
        for (kind, rows) in [(Documents::Projects, projects), (Documents::Cards, cards), (Documents::Runs, runs), (Documents::Times, entries)] {
            let table = kind.table();
            store.connection.execute(&format!("DELETE FROM {table}"), [])?;
            let mut insert = store.connection.prepare(&format!("INSERT INTO {table}(id,data) VALUES (?,?)"))?;
            for row in rows { insert.execute(params![row.id, row.data])?; }
        }
        store.connection.execute_batch("DELETE FROM logs; DELETE FROM settings;")?;
        for (seq, id, stream, text) in logs { store.connection.execute("INSERT INTO logs(seq,runId,stream,text) VALUES (?,?,?,?)", params![seq, id, stream, text])?; }
        for (id, data) in settings { store.connection.execute("INSERT INTO settings(id,data) VALUES (?,?)", params![id, data])?; }
        let vault: bool = store.connection.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='token_vault')", [], |row| row.get(0))?;
        if vault { store.connection.execute("DELETE FROM token_vault", [])?; }
        let mut settings = store.settings::<Document>()?;
        settings.set("credentialsReset", &true)?;
        store.save_settings(&settings)?;
        for mut run in store.documents::<Document>(Documents::Runs)? {
            if run.string_equals("kind", "pr-review")? {
                run.set("reviewStaleReason", &"Données restaurées : relancez la review pour vérifier les commits actuels.")?;
                store.put(Documents::Runs, &run)?;
            }
        }
        for mut card in store.documents::<Document>(Documents::Cards)? {
            if card.truthy("automation")? {
                let mut automation = Document::spread(card.raw("automation").ok_or("Automatisation absente.")?)?;
                automation.set("state", &"interrupted")?;
                automation.set("step", &"tests")?;
                automation.set("message", &"Données restaurées : reprends les vérifications et la review sur le worktree actuel.")?;
                card.set("automation", &automation)?;
                if !card.truthy("archived")? && !card.string_equals("status", "done")? { card.set("status", &"standby")?; }
                store.put(Documents::Cards, &card)?;
            }
        }
        after(store)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::store::open;

    #[test]
    fn incompatible_restore_preserves_current_data_and_creates_no_rescue() {
        let directory = tempfile::tempdir().unwrap();
        let current = open(&directory.path().join("current.sqlite")).unwrap();
        let source = directory.path().join("foreign.sqlite");
        let foreign = open(&source).unwrap();
        foreign.pragma_update(None, "user_version", 999).unwrap();
        foreign.close().unwrap();
        let store = Store::new(&current);
        store
            .put(
                Documents::Cards,
                &serde_json::json!({"id":"kept", "title":"Synthetic"}),
            )
            .unwrap();
        let rescue = directory.path().join("rescue.sqlite");
        assert!(restore(&store, &source, &rescue).is_err());
        assert!(!rescue.exists());
        assert_eq!(
            store.documents::<Document>(Documents::Cards).unwrap().len(),
            1
        );
    }
    use serde_json::json;

    #[test]
    fn backup_catalogue_creation_and_restore_survive_replaced_parent_paths() {
        let root = tempfile::tempdir().unwrap();
        let data = root.path().join("data");
        fs::create_dir(&data).unwrap();
        let connection = open(&root.path().join("live.sqlite")).unwrap();
        let store = Store::new(&connection);
        store.put(Documents::Cards, &json!({"id":"saved"})).unwrap();
        let backups = BackupDirectory::open(&data).unwrap();
        let saved = backups.create(&store).unwrap();
        store
            .put(Documents::Cards, &json!({"id":"recent"}))
            .unwrap();
        let outside = root.path().join("outside");
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("preserved"), "unchanged").unwrap();
        #[cfg(unix)]
        let actual = {
            let actual = root.path().join("moved");
            fs::rename(&data, &actual).unwrap();
            std::os::unix::fs::symlink(&outside, &data).unwrap();
            actual
        };
        #[cfg(windows)]
        let actual = {
            assert!(fs::rename(&data, root.path().join("moved")).is_err());
            data
        };
        assert_eq!(backups.list().unwrap().len(), 1);
        let created = backups.create(&store).unwrap();
        assert!(actual.join("backups").join(&created.name).is_file());
        let restored = backups.restore(&store, &saved.name).unwrap();
        assert!(actual.join("backups").join(restored.rescue).is_file());
        assert!(
            store
                .document::<Value>(Documents::Cards, "recent")
                .unwrap()
                .is_none()
        );
        assert_eq!(backups.list().unwrap().len(), 3);
        assert_eq!(fs::read_dir(&outside).unwrap().count(), 1);
        assert_eq!(
            fs::read_to_string(outside.join("preserved")).unwrap(),
            "unchanged"
        );
        assert_eq!(fs::read_dir(actual.join("backups")).unwrap().count(), 3);
    }

    #[test]
    fn catalogue_only_restores_named_local_files_and_preserves_a_named_rescue() {
        let root = tempfile::tempdir().unwrap();
        let connection = open(&root.path().join("live.sqlite")).unwrap();
        let store = Store::new(&connection);
        let backups = BackupDirectory::open(root.path()).unwrap();
        store.put(Documents::Cards, &json!({"id":"saved"})).unwrap();
        let saved = backups.create(&store).unwrap();
        assert!(valid_name(&saved.name));
        store
            .put(Documents::Cards, &json!({"id":"recent"}))
            .unwrap();
        for invalid in [
            "../live.sqlite",
            "",
            "atelier-0-unknown.sqlite",
            "/etc/passwd",
        ] {
            assert!(backups.restore(&store, invalid).is_err());
        }
        fs::write(backups.directory.join("unlisted.sqlite"), "ignored").unwrap();
        let directory_name = format!("atelier-0000000000000-{}.sqlite", uuid::Uuid::new_v4());
        fs::create_dir(backups.directory.join(directory_name)).unwrap();
        let listed = backups.list().unwrap();
        assert_eq!(listed.len(), 1);
        assert!(listed[0].bytes > 0);
        assert!(listed[0].date.ends_with('Z'));
        let restored = backups.restore(&store, &saved.name).unwrap();
        assert_eq!(restored.restored, saved.name);
        assert_ne!(restored.rescue, saved.name);
        assert!(backups.directory.join(restored.rescue).is_file());
        assert!(
            store
                .document::<serde_json::Value>(Documents::Cards, "recent")
                .unwrap()
                .is_none()
        );
        assert_eq!(backups.list().unwrap().len(), 2);
    }

    #[test]
    fn restore_keeps_order_invalidates_credentials_and_preserves_a_rescue() {
        let root = tempfile::tempdir().unwrap();
        let connection = open(&root.path().join("live.sqlite")).unwrap();
        let store = Store::new(&connection);
        store.put(Documents::Cards, &json!({"id":"one", "status":"review", "automation":{"state":"ready","extra":null}})).unwrap();
        store
            .put(
                Documents::Cards,
                &json!({"id":"two", "status":"done", "automation":{"state":"ready"}}),
            )
            .unwrap();
        store
            .put(
                Documents::Runs,
                &json!({"id":"review", "kind":"pr-review", "status":"completed"}),
            )
            .unwrap();
        store
            .put(
                Documents::Times,
                &json!({"id":"export", "exportState":"exported", "tempoId":42}),
            )
            .unwrap();
        store.save_settings(&json!({"extra":null})).unwrap();
        store.log("review", "stdout", "saved").unwrap();
        let saved = root.path().join("saved.sqlite");
        store.backup(&saved).unwrap();
        store
            .put(Documents::Cards, &json!({"id":"new", "status":"todo"}))
            .unwrap();
        store.log("review", "stdout", "newer").unwrap();
        connection.execute_batch("CREATE TABLE token_vault(id INTEGER PRIMARY KEY,encrypted TEXT); INSERT INTO token_vault VALUES(1,'synthetic-only');").unwrap();
        let rescue = root.path().join("rescue.sqlite");
        restore(&store, &saved, &rescue).unwrap();
        let cards = store
            .documents::<serde_json::Value>(Documents::Cards)
            .unwrap();
        assert_eq!(cards[0]["id"], "two");
        assert_eq!(cards[0]["status"], "done");
        assert_eq!(cards[1]["status"], "standby");
        assert_eq!(cards[1]["automation"]["state"], "interrupted");
        assert_eq!(cards[1]["automation"]["extra"], Value::Null);
        assert!(
            store
                .document::<serde_json::Value>(Documents::Runs, "review")
                .unwrap()
                .unwrap()
                .get("reviewStaleReason")
                .is_some()
        );
        assert_eq!(
            store.settings::<serde_json::Value>().unwrap(),
            json!({"extra":null,"credentialsReset":true})
        );
        assert_eq!(
            connection
                .query_row::<i64, _, _>("SELECT COUNT(*) FROM token_vault", [], |row| row.get(0))
                .unwrap(),
            0
        );
        assert_eq!(store.logs("review", 0).unwrap().len(), 1);
        store.log("review", "stdout", "after restore").unwrap();
        assert_eq!(store.logs("review", 0).unwrap()[1].seq, 3);
        let rescued = open(&rescue).unwrap();
        assert!(
            Store::new(&rescued)
                .document::<serde_json::Value>(Documents::Cards, "new")
                .unwrap()
                .is_some()
        );
        assert_eq!(
            rescued
                .query_row::<i64, _, _>("SELECT COUNT(*) FROM token_vault", [], |row| row.get(0))
                .unwrap(),
            1
        );
    }

    #[test]
    fn refuses_uncertain_exports_unfinished_backups_and_rolls_back_failed_restore() {
        let root = tempfile::tempdir().unwrap();
        let connection = open(&root.path().join("live.sqlite")).unwrap();
        let store = Store::new(&connection);
        let saved = root.path().join("saved.sqlite");
        store.put(Documents::Cards, &json!({"id":"saved"})).unwrap();
        store.backup(&saved).unwrap();
        store
            .put(Documents::Cards, &json!({"id":"current"}))
            .unwrap();
        let before = store
            .documents::<serde_json::Value>(Documents::Cards)
            .unwrap();
        let rescue = root.path().join("rescue.sqlite");
        for state in ["exported", "sending", "uncertain"] {
            store
                .put(
                    Documents::Times,
                    &json!({"id":"new", "exportState":state,"tempoId":42}),
                )
                .unwrap();
            assert!(restore(&store, &saved, &rescue).is_err());
            assert!(!rescue.exists());
            assert_eq!(
                store
                    .documents::<serde_json::Value>(Documents::Cards)
                    .unwrap(),
                before
            );
        }
        store.delete(Documents::Times, "new").unwrap();
        for (i, (table, value)) in [
            (
                Documents::Runs,
                json!({"id":"unfinished","status":"running"}),
            ),
            (
                Documents::Cards,
                json!({"id":"unfinished","automation":{"state":"running"}}),
            ),
            (
                Documents::Times,
                json!({"id":"unfinished","exportState":"draft","runningSince":"now"}),
            ),
            (
                Documents::Times,
                json!({"id":"unfinished","exportState":"sending"}),
            ),
            (
                Documents::Times,
                json!({"id":"unfinished","exportState":"uncertain"}),
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let candidate = root.path().join(format!("unfinished-{i}.sqlite"));
            store.put(table, &value).unwrap();
            store.backup(&candidate).unwrap();
            store.delete(table, "unfinished").unwrap();
            assert!(restore(&store, &candidate, &rescue).is_err());
            assert!(!rescue.exists());
        }
        connection.execute_batch("CREATE TRIGGER refuse_restore BEFORE INSERT ON cards BEGIN SELECT RAISE(ABORT,'injected failure'); END;").unwrap();
        assert!(restore(&store, &saved, &rescue).is_err());
        assert!(rescue.exists());
        assert_eq!(
            store
                .documents::<serde_json::Value>(Documents::Cards)
                .unwrap(),
            before
        );
    }
}
