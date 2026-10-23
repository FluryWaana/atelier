use super::{
    checked_file::{CheckedFile, Identity},
    store::{Documents, Store},
};
use crate::Result;
use rusqlite::{Connection, OpenFlags, params};
use serde_json::{Value, json};
use std::{collections::HashMap, path::Path};

struct SavedDocument {
    id: String,
    data: String,
    value: Value,
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

fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(value)) => *value,
        Some(Value::Number(value)) => value.as_f64() != Some(0.0),
        Some(Value::String(value)) => !value.is_empty(),
        Some(Value::Array(_) | Value::Object(_)) => true,
    }
}

pub fn restore(store: &Store<'_>, source: &Path, rescue: &Path) -> Result<()> {
    let expected = Identity::inspect(source)?;
    let mut snapshot = tempfile::NamedTempFile::new()?;
    CheckedFile::open(source, &expected)?.copy_to(snapshot.as_file_mut())?;
    let source = Connection::open_with_flags(snapshot.path(), OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let integrity: String = source.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
    if integrity != "ok" {
        return Err("Sauvegarde endommagée.".into());
    }
    let projects = documents(&source, Documents::Projects)?;
    let cards = documents(&source, Documents::Cards)?;
    let runs = documents(&source, Documents::Runs)?;
    let entries = documents(&source, Documents::Times)?;
    let times: HashMap<_, _> = entries
        .iter()
        .map(|row| (row.id.as_str(), &row.value))
        .collect();
    for current in store.documents(Documents::Times)? {
        if current.get("exportState") == Some(&json!("draft")) {
            continue;
        }
        let saved = current
            .get("id")
            .and_then(Value::as_str)
            .and_then(|id| times.get(id));
        if saved.is_none_or(|saved| {
            saved.get("exportState") != current.get("exportState")
                || saved.get("tempoId") != current.get("tempoId")
                || saved.get("exportState") != Some(&json!("exported"))
        }) {
            return Err("Restauration refusée : des envois Tempo plus récents ou incertains doivent être conservés. Choisissez une sauvegarde récente.".into());
        }
    }
    if runs
        .iter()
        .any(|run| run.value.get("status") == Some(&json!("running")))
        || cards.iter().any(|card| {
            card.value
                .get("automation")
                .and_then(|automation| automation.get("state"))
                == Some(&json!("running"))
        })
        || times.values().any(|time| {
            truthy(time.get("runningSince"))
                || matches!(
                    time.get("exportState").and_then(Value::as_str),
                    Some("sending" | "uncertain")
                )
        })
    {
        return Err("Cette sauvegarde contient des opérations inachevées.".into());
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
    store.backup(rescue)?;
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
        let mut settings = store.settings()?;
        settings.as_object_mut().ok_or("Paramètres sauvegardés invalides.")?.insert("credentialsReset".into(), json!(true));
        store.save_settings(&settings)?;
        for mut run in store.documents(Documents::Runs)? {
            if run.get("kind") == Some(&json!("pr-review")) {
                run["reviewStaleReason"] = json!("Données restaurées : relancez la review pour vérifier les commits actuels.");
                store.put(Documents::Runs, &run)?;
            }
        }
        for mut card in store.documents(Documents::Cards)? {
            if truthy(card.get("automation")) {
                let mut automation = card["automation"].as_object().cloned().unwrap_or_default();
                automation.insert("state".into(), json!("interrupted"));
                automation.insert("step".into(), json!("tests"));
                automation.insert("message".into(), json!("Données restaurées : reprends les vérifications et la review sur le worktree actuel."));
                card["automation"] = Value::Object(automation);
                if !truthy(card.get("archived")) && card.get("status") != Some(&json!("done")) { card["status"] = json!("standby"); }
                store.put(Documents::Cards, &card)?;
            }
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::store::open;

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
        let cards = store.documents(Documents::Cards).unwrap();
        assert_eq!(cards[0]["id"], "two");
        assert_eq!(cards[0]["status"], "done");
        assert_eq!(cards[1]["status"], "standby");
        assert_eq!(cards[1]["automation"]["state"], "interrupted");
        assert_eq!(cards[1]["automation"]["extra"], Value::Null);
        assert!(
            store
                .document(Documents::Runs, "review")
                .unwrap()
                .unwrap()
                .get("reviewStaleReason")
                .is_some()
        );
        assert_eq!(
            store.settings().unwrap(),
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
                .document(Documents::Cards, "new")
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
        let before = store.documents(Documents::Cards).unwrap();
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
            assert_eq!(store.documents(Documents::Cards).unwrap(), before);
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
        assert_eq!(store.documents(Documents::Cards).unwrap(), before);
    }
}
