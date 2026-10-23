use crate::{Result, command, node_executable, run};
use atelier_app::infrastructure::{
    backups,
    data_lock::DataLock,
    database::Database,
    portable_import,
    store::{Documents, Store},
    token_vault::{self, KeyringPlatform},
};
use atelier_core::document::{Document, JsonString};
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
        json!({"projects":store.documents::<serde_json::Value>(Documents::Projects)?, "cards":store.documents::<serde_json::Value>(Documents::Cards)?, "runs":store.documents::<serde_json::Value>(Documents::Runs)?, "times":store.documents::<serde_json::Value>(Documents::Times)?, "settings":store.settings::<serde_json::Value>()?, "logs":logs}),
    )
}

fn node(mode: &str, database: &Path, reference: &Path) -> Result<()> {
    fixture("rust-storage-reference.mjs", mode, database, reference)
}

fn fixture(script: &str, mode: &str, database: &Path, reference: &Path) -> Result<()> {
    run(command(node_executable())
        .arg(format!("test/fixtures/{script}"))
        .arg(mode)
        .arg(database)
        .arg(reference))
}

fn try_node_lock(directory: &Path, expected: i32) -> Result<()> {
    let output = command(node_executable())
        .args(["test/fixtures/rust-lock-reference.mjs", "try"])
        .arg(directory)
        .output()?;
    if output.status.code() != Some(expected) {
        return Err("Exclusion Node/Rust incorrecte.".into());
    }
    Ok(())
}

async fn test_data_lock(directory: &Path) -> Result<()> {
    use std::{process::Stdio, time::Duration};
    use tokio::io::{AsyncBufReadExt, BufReader};
    let root = directory.join("locks");
    let first = DataLock::acquire(&root)?;
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    try_node_lock(&root.join("."), 2)?;
    if DataLock::acquire(&root).is_ok() {
        return Err("Une seconde connexion Rust a obtenu le verrou.".into());
    }
    // A rejected same-process acquisition must not release POSIX locks held by the first connection.
    try_node_lock(&root, 2)?;
    drop(first);
    try_node_lock(&root, 0)?;
    let mut command = tokio::process::Command::from(command(node_executable()));
    command
        .args(["test/fixtures/rust-lock-reference.mjs", "hold"])
        .arg(&root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true);
    let mut node = command.spawn()?;
    let mut output = BufReader::new(node.stdout.take().ok_or("Sortie de fixture absente.")?);
    let mut ready = String::new();
    tokio::time::timeout(Duration::from_secs(5), output.read_line(&mut ready)).await??;
    if ready.trim() != "ready" {
        return Err("Le verrou Node de test n'a pas démarré.".into());
    }
    if DataLock::acquire(&root.join(".")).is_ok() {
        return Err("Rust a ignoré le verrou Node.".into());
    }
    try_node_lock(&root, 2)?;
    node.kill().await?;
    let after_crash = DataLock::acquire(&root)?;
    try_node_lock(&root, 2)?;
    drop(after_crash);
    drop(listener);
    try_node_lock(&root, 0)?;
    println!(
        "Verrou commun réussi : exclusion Node/Rust sur dossier canonique indépendamment des ports, maintien après refus local et reprise après arrêt brutal Node."
    );
    run(crate::command("cargo").args(["build", "--locked", "-p", "atelier-app"]))?;
    let data = directory.join("locked-server");
    let assets = directory.join("lock-assets");
    std::fs::create_dir(&assets)?;
    std::fs::write(
        assets.join("index.html"),
        "<!doctype html><title>Fixture</title>",
    )?;
    let port = std::net::TcpListener::bind("127.0.0.1:0")?
        .local_addr()?
        .port();
    let executable = crate::root()
        .join("target/debug")
        .join(format!("atelier{}", std::env::consts::EXE_SUFFIX));
    let mut command = tokio::process::Command::from(crate::command(executable));
    command
        .arg("serve-prototype")
        .arg("--data-dir")
        .arg(&data)
        .arg("--assets")
        .arg(&assets)
        .arg("--port")
        .arg(port.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .kill_on_drop(true);
    let mut rust = command.spawn()?;
    let client = atelier_app::infrastructure::local_http::LocalHttp::new()?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        if rust.try_wait()?.is_some() {
            return Err("Le serveur Rust de verrou est arrêté.".into());
        }
        if let Some(health) = client
            .authenticated_health(&format!("http://127.0.0.1:{port}"), || {
                atelier_app::infrastructure::session::read_optional(&data)
            })
            .await?
        {
            if health["pid"].as_u64() != rust.id().map(u64::from) {
                return Err("Identité du processus de test incorrecte.".into());
            }
            break;
        }
        if tokio::time::Instant::now() >= deadline {
            return Err("Délai du serveur Rust de verrou dépassé.".into());
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    try_node_lock(&data, 2)?;
    rust.kill().await?;
    try_node_lock(&data, 0)?;
    println!(
        "Arrêt brutal Rust vérifié : le vrai serveur bloque N1, puis le verrou se libère sans nettoyage manuel."
    );
    Ok(())
}

async fn test_utf16(directory: &Path) -> Result<()> {
    let source = directory.join("utf16.sqlite");
    let reference = directory.join("utf16.json");
    fixture("rust-utf16-reference.mjs", "seed", &source, &reference)?;
    let metadata: Document = serde_json::from_slice(&std::fs::read(&reference)?)?;
    let backup = directory.join("backups").join(
        metadata
            .get::<String>("backupName")?
            .ok_or("Sauvegarde absente.")?,
    );
    let path = directory.join("utf16.sqlite.rust.sqlite");
    let database = Database::open(&path)?;
    database
        .call(|store| {
            store.transaction(|store| {
                let original = store
                    .document::<atelier_core::contracts::Card>(Documents::Cards, "card")?
                    .ok_or("Carte Unicode absente.")?;
                let copied = atelier_core::application::duplicate_card(
                    &original,
                    "copy".into(),
                    "2026-10-24T00:00:00.000Z".into(),
                )?;
                store.put(Documents::Cards, &copied)?;
                for table in [
                    Documents::Projects,
                    Documents::Cards,
                    Documents::Runs,
                    Documents::Times,
                ] {
                    for mut document in store.documents::<Document>(table)? {
                        document.set("rustRoundTrip", &true)?;
                        document.set("rustUnits", &JsonString(vec![0xd800, 0x61, 0xdc00]))?;
                        store.put(table, &document)?;
                    }
                }
                let mut settings = store.settings::<Document>()?;
                settings.set("rustRoundTrip", &true)?;
                settings.set("rustUnits", &JsonString(vec![0xd800, 0x61, 0xdc00]))?;
                store.save_settings(&settings)
            })
        })
        .await?;
    database.close().await?;
    fixture("rust-utf16-reference.mjs", "roundtrip", &path, &reference)?;
    let database = Database::open(&path)?;
    let rescue = directory.join("utf16-rescue.sqlite");
    let rescue_path = rescue.clone();
    database
        .call(move |store| backups::restore(store, &backup, &rescue_path))
        .await?;
    database.close().await?;
    fixture("rust-utf16-reference.mjs", "restored", &path, &reference)?;
    fixture("rust-utf16-reference.mjs", "roundtrip", &rescue, &reference)?;
    println!(
        "Parité UTF-16 : duplication Node, clés/valeurs isolées, réécriture Rust, restauration et secours sans perte."
    );
    Ok(())
}

pub async fn test() -> Result<()> {
    let directory = tempfile::tempdir()?;
    test_data_lock(directory.path()).await?;
    test_utf16(directory.path()).await?;
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
                    for document in store.documents::<serde_json::Value>(table)? {
                        store.put(table, &document)?;
                    }
                }
                let mut card = store
                    .document::<serde_json::Value>(Documents::Cards, "card-0")?
                    .ok_or("Carte de référence absente.")?;
                card["rustRoundTrip"] = json!({"preserved":true,"number":9_007_199_254_740_991u64});
                store.put(Documents::Cards, &card)?;
                let mut settings = store.settings::<serde_json::Value>()?;
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
    let directory = directory.path().join("import");
    let reference = directory.join("expected.json");
    fixture("rust-import-reference.mjs", "seed", &directory, &reference)?;
    let destination = directory.join("rust");
    portable_import::import(&directory.join("source"), &destination)?;
    fixture(
        "rust-import-reference.mjs",
        "verify",
        &destination,
        &reference,
    )?;
    println!(
        "Parité import réussie : documents, logs, pièces jointes et sauvegarde identiques ; sessions périmées écartées."
    );
    Ok(())
}
