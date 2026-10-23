use crate::{Result, command, root, run};
use atelier_app::infrastructure::{
    database::Database,
    store::{Documents, Store},
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
        .arg("test/fixtures/rust-storage-reference.mjs")
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
    Ok(())
}
