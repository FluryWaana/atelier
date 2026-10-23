use crate::{Result, command, root, run};
use atelier_app::infrastructure::{
    backups,
    database::Database,
    store::{Documents, Store},
    token_vault::{self, KeyringPlatform},
};
use serde_json::{Value, json};
use std::path::Path;

fn snapshot(store: &Store<'_>) -> Result<Value> {
    let mut logs = Vec::new();
    let mut cursor = 0;
    loop {
        let page = store.logs("run-0", cursor)?;
        if page.is_empty() {
            break;
        }
        cursor = page.last().ok_or("Page de logs vide.")?.seq;
        logs.extend(page);
    }
    Ok(
        json!({"projects":store.documents(Documents::Projects)?, "cards":store.documents(Documents::Cards)?, "runs":store.documents(Documents::Runs)?, "times":store.documents(Documents::Times)?, "settings":store.settings()?, "logs":logs}),
    )
}

fn node(mode: &str, database: &Path, reference: &Path) -> Result<()> {
    fixture("rust-storage-reference.mjs", mode, database, reference)
}

fn fixture(script: &str, mode: &str, database: &Path, reference: &Path) -> Result<()> {
    let executable = std::env::var_os("ATELIER_NODE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            root().join(if cfg!(windows) {
                "runtime/node.exe"
            } else {
                "runtime/node"
            })
        });
    run(command(executable)
        .arg(format!("test/fixtures/{script}"))
        .arg(mode)
        .arg(database)
        .arg(reference))
}

pub async fn test() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("reference.sqlite");
    let reference = directory.path().join("expected.json");
    let saved = directory.path().join("snapshot.sqlite");
    node("seed", &path, &reference)?;
    let database = Database::open(&path)?;
    let expected: Value = serde_json::from_slice(&std::fs::read(&reference)?)?;
    if database.call(snapshot).await? != expected {
        return Err("La lecture Rust diffère de la référence Node.".into());
    }
    database
        .call(|store| {
            store.transaction(|store| {
                for table in [
                    Documents::Projects,
                    Documents::Cards,
                    Documents::Runs,
                    Documents::Times,
                ] {
                    for document in store.documents(table)? {
                        store.put(table, &document)?;
                    }
                }
                let mut card = store
                    .document(Documents::Cards, "card-0")?
                    .ok_or("Carte de référence absente.")?;
                card["rustRoundTrip"] = json!({"preserved":true,"number":9_007_199_254_740_991u64});
                store.put(Documents::Cards, &card)?;
                let mut settings = store.settings()?;
                settings["rustRoundTrip"] = json!(true);
                store.save_settings(&settings)?;
                store.log("run-0", "stderr", &format!("{}🔧tail", "a".repeat(65_535)))?;
                Ok(())
            })
        })
        .await?;
    let expected = database.call(snapshot).await?;
    std::fs::write(&reference, serde_json::to_vec(&expected)?)?;
    let backup = saved.clone();
    database.call(move |store| store.backup(&backup)).await?;
    database.close().await?;
    node("verify", &path, &reference)?;
    node("verify", &saved, &reference)?;
    node("change", &path, &reference)?;
    let reopened = Database::open(&path)?;
    let expected: Value = serde_json::from_slice(&std::fs::read(&reference)?)?;
    if reopened.call(snapshot).await? != expected {
        return Err("La relecture après le retour Node a perdu des données.".into());
    }
    reopened.close().await?;
    println!(
        "Parité stockage réussie : Node → Rust → Node → Rust, 810 documents, paramètres, 10 002 logs et snapshot WAL."
    );
    let source = directory.path().join("restore.sqlite");
    let expected_path = directory.path().join("restore.json");
    fixture("rust-backup-reference.mjs", "seed", &source, &expected_path)?;
    let expected: Value = serde_json::from_slice(&std::fs::read(&expected_path)?)?;
    let source = directory.path().join("backups").join(
        expected["backupName"]
            .as_str()
            .ok_or("Nom de sauvegarde absent.")?,
    );
    let rust_path = directory.path().join("restore.sqlite.rust.sqlite");
    let database = Database::open(&rust_path)?;
    let rescue = directory.path().join("rust-rescue.sqlite");
    let rescue_path = rescue.clone();
    database
        .call(move |store| backups::restore(store, &source, &rescue_path))
        .await?;
    if database.call(snapshot).await? != expected["restored"] {
        return Err("La restauration Rust diffère de Node.".into());
    }
    database.close().await?;
    fixture(
        "rust-backup-reference.mjs",
        "verify",
        &rust_path,
        &expected_path,
    )?;
    fixture(
        "rust-backup-reference.mjs",
        "rescue",
        &rescue,
        &expected_path,
    )?;
    println!(
        "Parité restauration réussie : documents, logs, invalidation du coffre/reviews/automatisation et sauvegarde de secours identiques à Node."
    );
    let path = directory.path().join("vault.sqlite");
    let reference = directory.path().join("vault.json");
    fixture("rust-vault-reference.mjs", "seed", &path, &reference)?;
    let mut expected: Value = serde_json::from_slice(&std::fs::read(&reference)?)?;
    for (name, platform) in [
        ("linux", KeyringPlatform::Linux),
        ("darwin", KeyringPlatform::Macos),
    ] {
        let mut key = async |_: &str, _: &[&str], input: Option<&str>| -> Result<(i32, String)> {
            if input.is_some() {
                return Err("La clé synthétique doit déjà exister.".into());
            }
            Ok((0, "11".repeat(32)))
        };
        let envelope = expected["envelopes"][name]
            .as_str()
            .ok_or("Enveloppe absente.")?;
        let decoded = token_vault::keyring_protect(platform, envelope, true, &mut key).await?;
        if serde_json::from_str::<Value>(&decoded)? != expected["tokens"] {
            return Err("Coffre Node illisible par Rust.".into());
        }
        expected["envelopes"][name] =
            json!(token_vault::keyring_protect(platform, &decoded, false, &mut key).await?);
    }
    #[cfg(windows)]
    {
        let database = Database::open(&path)?;
        if token_vault::load(&database).await? != expected["tokens"] {
            return Err("Coffre DPAPI Node illisible par Rust.".into());
        }
        token_vault::save(&database, &expected["tokens"]).await?;
        database.close().await?;
    }
    std::fs::write(&reference, serde_json::to_vec(&expected)?)?;
    fixture("rust-vault-reference.mjs", "verify", &path, &reference)?;
    println!(
        "Parité coffre réussie : enveloppes AES-GCM Node/Rust dans les deux directions ; DPAPI et SQLite également sur Windows. Les trousseaux POSIX sont simulés ici."
    );
    Ok(())
}
