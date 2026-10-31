use crate::{Result, command, package, root, run};
use atelier_app::infrastructure::{
    data_lock::DataLock, installation::InstallationMode, installed, launcher,
    local_http::LocalHttp, package as payload, package_archive, session, update_package, updates,
};
use atelier_core::application::updates::Phase;
use serde_json::json;
use std::{
    path::{Component, Path, PathBuf},
    time::{Duration, Instant},
};

struct Stop {
    root: PathBuf,
    data: PathBuf,
    port: u16,
}
impl Drop for Stop {
    fn drop(&mut self) {
        let data = self.data.clone();
        let root = self.root.clone();
        let port = self.port;
        let _ = std::thread::spawn(move || {
            if let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                let _ = runtime.block_on(stop_fixture(&root, &data, port));
            }
        })
        .join();
    }
}

pub(super) async fn stop_fixture(root: &Path, data: &Path, port: u16) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(180);
    loop {
        let operation = updates::status(Some(root))?.operation;
        let Some(operation) = operation.filter(|operation| operation.state.pending()) else {
            break;
        };
        if operation.state.maintenance() {
            let _ = updates::request_recovery(root, &operation.id, true).await;
        } else {
            let _ = updates::cancel(root, &operation.id, true);
        }
        if Instant::now() >= deadline {
            return Err("La fixture de mise à jour n’a pas confirmé son nettoyage ; son état reste à examiner.".into());
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    launcher::stop(data, port).await
}
pub(super) fn next_binary(temporary: &Path, next: &str) -> Result<PathBuf> {
    let source = temporary.join("source");
    std::fs::create_dir(&source)?;
    let output = command("git")
        .args([
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "-z",
            "--",
            "Cargo.toml",
            "Cargo.lock",
            "rust-toolchain.toml",
            "LICENSE",
            ".cargo",
            "crates",
            "xtask",
        ])
        .output()?;
    if !output.status.success() {
        return Err("Inventaire Git de la fixture de mise à jour refusé.".into());
    }
    for path in output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
    {
        let path = Path::new(std::str::from_utf8(path)?);
        if path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
        {
            return Err("Chemin de source de fixture invalide.".into());
        }
        let origin = root().join(path);
        if !atelier_app::infrastructure::checked_file::plain_metadata(&origin)?.is_file() {
            return Err("Source de fixture régulière requise.".into());
        }
        let destination = source.join(path);
        std::fs::create_dir_all(destination.parent().ok_or("Parent de source absent.")?)?;
        std::fs::copy(origin, destination)?;
    }
    let version = env!("CARGO_PKG_VERSION");
    let manifest = source.join("Cargo.toml");
    let text = std::fs::read_to_string(&manifest)?;
    let previous = format!("version = \"{version}\"");
    if !text.contains(&previous) {
        return Err("Version de départ de fixture absente.".into());
    }
    std::fs::write(
        &manifest,
        text.replacen(&previous, &format!("version = \"{next}\""), 1),
    )?;
    let lock = source.join("Cargo.lock");
    let text = std::fs::read_to_string(&lock)?;
    let changed = text
        .split("[[package]]")
        .map(|part| {
            if ["atelier-app", "atelier-core", "atelier-ui", "xtask"]
                .iter()
                .any(|name| {
                    part.lines()
                        .any(|line| line == format!("name = \"{name}\""))
                })
            {
                part.replacen(&previous, &format!("version = \"{next}\""), 1)
            } else {
                part.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("[[package]]");
    std::fs::write(lock, changed)?;
    // Reuse release dependencies without overwriting the original version in target/release.
    let target = crate::target().join("synthetic-update");
    run(command("cargo")
        .current_dir(&source)
        .env("CARGO_TARGET_DIR", &target)
        .args([
            "build",
            "--release",
            "--locked",
            "--offline",
            "-p",
            "atelier-app",
        ]))?;
    let binary = target.join("release").join(if cfg!(windows) {
        "atelier.exe"
    } else {
        "atelier"
    });
    let successor = temporary.join("successor-bin");
    std::fs::create_dir(&successor)?;
    let successor = successor.join(format!("atelier{}", std::env::consts::EXE_SUFFIX));
    std::fs::copy(binary, &successor)?;
    Ok(successor)
}
pub(super) fn archive(
    temporary: &Path,
    name: &str,
    binary: &Path,
    assets: &Path,
    version: &str,
) -> Result<(PathBuf, PathBuf)> {
    let target = payload::native_target()?;
    let directory = temporary.join(name);
    package::stage(
        target,
        binary,
        assets,
        &temporary.join("third-party-notices.txt"),
        &directory,
    )?;
    std::fs::write(
        directory.join("atelier-package.json"),
        serde_json::to_vec_pretty(&payload::manifest(&directory, target, version)?)?,
    )?;
    let archive = temporary.join(format!("{name}.zip"));
    let metadata = temporary.join(format!("{name}.json"));
    package_archive::create(&directory, &archive)?;
    update_package::sign(&archive, target, &temporary.join("keys"), &metadata)?;
    Ok((archive, metadata))
}
async fn terminal(root: &Path, id: &str) -> Result<Phase> {
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        let operation = updates::status(Some(root))?
            .operation
            .ok_or("Journal de qualification absent.")?;
        if operation.id != id {
            return Err("Le journal de qualification a changé.".into());
        }
        if !operation.state.pending() {
            return Ok(operation.state);
        }
        if Instant::now() >= deadline {
            return Err("Délai de qualification de mise à jour dépassé.".into());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

pub(super) async fn test() -> Result<()> {
    test_inner(None, None).await
}

pub(super) async fn test_prepared(original: &Path, successor: &Path) -> Result<()> {
    test_inner(Some(original), Some(successor)).await
}

pub(super) async fn test_installed(
    installation: &Path,
    data: &Path,
    port: u16,
    delivery: &Path,
) -> Result<()> {
    if !cfg!(windows)
        || std::env::var("CI").as_deref() != Ok("true")
        || std::env::var("ATELIER_RUST_FINAL_CAMPAIGN").as_deref() != Ok("1")
        || port == 0
    {
        return Err("Qualification Setup réservée à Windows CI final jetable.".into());
    }
    let base =
        std::fs::canonicalize(std::env::var_os("RUNNER_TEMP").ok_or("Runner temporaire absent.")?)?;
    let installation = std::fs::canonicalize(installation)?;
    let data = std::fs::canonicalize(data)?;
    if !installation.starts_with(&base)
        || !data.starts_with(&base)
        || installation == base
        || data == base
    {
        return Err(
            "Installation et données synthétiques doivent appartenir au temporaire du runner."
                .into(),
        );
    }
    installed::verify_all(&installation, payload::native_target()?)?;
    let original = installed::open(&installation, payload::native_target()?)?
        .ok_or("Installation Setup absente.")?;
    let old = original.current().active.clone();
    launcher::start(
        &original.active_executable()?,
        &data,
        &original.slot_path(&old)?.join("ui"),
        port,
        false,
    )
    .await?;
    let _stop = Stop {
        root: installation.clone(),
        data: data.clone(),
        port,
    };
    let address = format!("http://127.0.0.1:{port}");
    let http = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(10))
        .build()?;
    let token = session::read(&data)?;
    let card: serde_json::Value = http
        .post(format!("{address}/api/brainstorms"))
        .bearer_auth(&token)
        .header("x-local-request", "1")
        .json(&json!({"title":"Synthetic Setup upgrade persistence"}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let preview = updates::preview(
        &installation,
        &delivery.join("successor/next.zip"),
        &delivery.join("successor/next.json"),
        &data,
        port,
    )
    .await?;
    updates::apply(&installation, &preview.id, true).await?;
    if terminal(&installation, &preview.id).await? != Phase::Committed {
        return Err("Upgrade Setup non validé.".into());
    }
    let current = installed::open(&installation, payload::native_target()?)?
        .ok_or("Installation mise à jour absente.")?;
    if current.current().previous.as_ref() != Some(&old) || current.current().active.id == old.id {
        return Err("Historique de code Setup non conservé.".into());
    }
    let health = LocalHttp::new()?
        .authenticated_health(&address, || session::read_optional(&data))
        .await?
        .ok_or("Serveur mis à jour absent.")?;
    if health["version"] != current.current().active.version {
        return Err("Version réellement servie différente du slot activé.".into());
    }
    let next_token = session::read(&data)?;
    if next_token == token {
        return Err("Session Setup non renouvelée.".into());
    }
    let state: serde_json::Value = http
        .get(format!("{address}/api/state"))
        .bearer_auth(&next_token)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    if state["brainstorms"]
        .as_array()
        .is_none_or(|cards| !cards.contains(&card))
    {
        return Err("Conversation perdue pendant upgrade Setup.".into());
    }
    launcher::stop(&data, port).await?;
    installed::verify_all(&installation, payload::native_target()?)?;
    payload::write(
        &delivery.join("native-installed-upgrade-qualification.json"),
        &serde_json::to_vec_pretty(
            &json!({"format":"atelier-rust-installed-upgrade-qualification/1","fromVersion":old.version,"toVersion":current.current().active.version,"runtimeVersionObserved":true,"conversationPreserved":true,"previousSlotRetained":true,"sessionRotated":true,"stopped":true,"published":false}),
        )?,
    )?;
    Ok(())
}

pub(super) async fn test_delivery(delivery: &Path) -> Result<()> {
    if std::env::var("CI").as_deref() != Ok("true")
        || std::env::var("ATELIER_RUST_FINAL_CAMPAIGN").as_deref() != Ok("1")
    {
        return Err("Réutilisation de paquets réservée à la campagne finale CI.".into());
    }
    let target = payload::native_target()?;
    let version = env!("CARGO_PKG_VERSION");
    let trust = update_package::Trust::load(&delivery.join(update_package::PUBLIC_KEY))?;
    let scratch = tempfile::tempdir()?;
    let original = update_package::prepare(
        &delivery.join(format!("atelier-kanban-{version}-{target}.zip")),
        &delivery.join("signed-package.json"),
        &trust,
        target,
        "0.0.0",
        scratch.path(),
    )?;
    if original.payload().version != version {
        return Err(
            "La version du paquet original ne correspond pas au commit de qualification.".into(),
        );
    }
    let successor = update_package::prepare(
        &delivery.join("successor/next.zip"),
        &delivery.join("successor/next.json"),
        &trust,
        target,
        version,
        scratch.path(),
    )?;
    test_prepared(
        &original
            .package()
            .path()
            .join(payload::binary_name(target)?),
        &successor
            .package()
            .path()
            .join(payload::binary_name(target)?),
    )
    .await?;
    payload::write(
        &delivery.join("update-rollback-qualification.json"),
        &serde_json::to_vec_pretty(
            &json!({"format":"atelier-rust-update-qualification/1","target":target,"fromVersion":version,"toVersion":successor.payload().version,"signedPreparedArtifactsReused":true,"realRuntimeVersionsObserved":true,"updateExecuted":true,"wrongRuntimeRollbackExecuted":true,"conversationPreserved":true,"sessionRotated":true,"published":false}),
        )?,
    )?;
    Ok(())
}

async fn test_inner(original: Option<&Path>, prepared_successor: Option<&Path>) -> Result<()> {
    if std::env::var("CI").as_deref() != Ok("true")
        || std::env::var("ATELIER_RUST_FINAL_CAMPAIGN").as_deref() != Ok("1")
    {
        return Err(
            "Ce parcours natif lourd est réservé à la campagne finale CI synthétique.".into(),
        );
    }
    if original.is_none() {
        run(command("cargo").args(["build", "--locked", "-p", "atelier-app"]))?;
    }
    let temporary = tempfile::tempdir()?;
    crate::notices::generate(&temporary.path().join("third-party-notices.txt")).await?;
    let assets = root().join(".artifacts/rust-ui");
    let current = env!("CARGO_PKG_VERSION");
    let mut parts = payload::version_parts(current)?;
    parts[2] = parts[2]
        .checked_add(1)
        .ok_or("Version de fixture excessive.")?;
    let next = format!("{}.{}.{}", parts[0], parts[1], parts[2]);
    let binary = original.map(Path::to_owned).unwrap_or_else(|| {
        crate::target().join("debug").join(if cfg!(windows) {
            "atelier.exe"
        } else {
            "atelier"
        })
    });
    let successor = match prepared_successor {
        Some(successor) => successor.to_owned(),
        None => next_binary(temporary.path(), &next)?,
    };
    update_package::generate_synthetic(&temporary.path().join("keys"))?;
    let (original, original_signature) =
        archive(temporary.path(), "original", &binary, &assets, current)?;
    let (new, new_signature) = archive(temporary.path(), "successor", &successor, &assets, &next)?;
    let installation = temporary.path().join("installed");
    installed::install(
        &original,
        &original_signature,
        &temporary.path().join("keys/synthetic-update-public.json"),
        &installation,
        InstallationMode::Portable,
        payload::native_target()?,
    )?;
    let data = temporary.path().join("synthetic-data");
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    drop(listener);
    let _stop = Stop {
        root: installation.clone(),
        data: data.clone(),
        port,
    };
    let initial = installed::open(&installation, payload::native_target()?)?
        .ok_or("Installation de qualification absente.")?;
    launcher::start(
        &initial.active_executable()?,
        &data,
        &initial.slot_path(&initial.current().active)?.join("ui"),
        port,
        false,
    )
    .await?;
    let address = format!("http://127.0.0.1:{port}");
    let observed_initial = LocalHttp::new()?
        .authenticated_health(&address, || session::read_optional(&data))
        .await?
        .ok_or("Version initiale absente.")?;
    if observed_initial["version"] != current {
        return Err("La version initiale réellement exécutée ne correspond pas au paquet.".into());
    }
    let http = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(10))
        .build()?;
    let old_token = session::read(&data)?;
    let card: serde_json::Value = http
        .post(format!("{address}/api/brainstorms"))
        .bearer_auth(&old_token)
        .header("x-local-request", "1")
        .json(&json!({"title":"Synthetic update persistence"}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let preview = updates::preview(&installation, &new, &new_signature, &data, port).await?;
    if updates::apply(&installation, &preview.id, false)
        .await
        .is_ok()
    {
        return Err("Activation non confirmée acceptée.".into());
    }
    updates::apply(&installation, &preview.id, true).await?;
    if terminal(&installation, &preview.id).await? != Phase::Committed {
        return Err("La mise à jour native réelle n’a pas été validée.".into());
    }
    let observed = LocalHttp::new()?
        .authenticated_health(&address, || session::read_optional(&data))
        .await?
        .ok_or("Nouvelle version absente.")?;
    if observed["version"] != next {
        return Err("La version compilée réelle ne correspond pas au paquet accepté.".into());
    }
    let token = session::read(&data)?;
    if token == old_token {
        return Err("Session non renouvelée après mise à jour.".into());
    }
    if http
        .get(format!("{address}/api/state"))
        .bearer_auth(&old_token)
        .send()
        .await?
        .status()
        != reqwest::StatusCode::UNAUTHORIZED
    {
        return Err("Ancienne session encore acceptée.".into());
    }
    let state: serde_json::Value = http
        .get(format!("{address}/api/state"))
        .bearer_auth(&token)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    if state["brainstorms"]
        .as_array()
        .is_none_or(|cards| !cards.contains(&card))
    {
        return Err("Une conversation a changé pendant la mise à jour.".into());
    }
    parts[2] = parts[2]
        .checked_add(1)
        .ok_or("Version de fixture excessive.")?;
    let broken = format!("{}.{}.{}", parts[0], parts[1], parts[2]);
    let (bad, bad_signature) = archive(
        temporary.path(),
        "wrong-runtime-version",
        &binary,
        &assets,
        &broken,
    )?;
    let preview = updates::preview(&installation, &bad, &bad_signature, &data, port).await?;
    updates::apply(&installation, &preview.id, true).await?;
    if terminal(&installation, &preview.id).await? != Phase::RolledBack {
        return Err("La mauvaise version exécutée n’a pas déclenché le rollback.".into());
    }
    if updates::status(Some(&installation))?
        .operation
        .is_none_or(|operation| {
            !operation
                .message
                .contains("La version exécutée ne correspond pas à la version signée.")
        })
    {
        return Err("Le scénario négatif n’a pas atteint le contrôle de version prévu.".into());
    }
    let observed = LocalHttp::new()?
        .authenticated_health(&address, || session::read_optional(&data))
        .await?
        .ok_or("Version récupérée absente.")?;
    if observed["version"] != next {
        return Err("La récupération n’a pas relancé la version précédente réelle.".into());
    }
    let state: serde_json::Value = http
        .get(format!("{address}/api/state"))
        .bearer_auth(session::read(&data)?)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    if state["brainstorms"]
        .as_array()
        .is_none_or(|cards| !cards.contains(&card))
    {
        return Err("La récupération a modifié une conversation.".into());
    }
    launcher::stop(&data, port).await?;
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(lock) = DataLock::acquire(&data) {
            drop(lock);
            break;
        }
        if Instant::now() >= deadline {
            return Err("Le verrou de données reste occupé après qualification.".into());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    println!(
        "Native synthetic update and rollback journeys succeeded for {}",
        payload::native_target()?
    );
    Ok(())
}
