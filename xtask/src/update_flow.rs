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
fn next_binary(temporary: &Path, next: &str) -> Result<PathBuf> {
    let source = temporary.join("source");
    std::fs::create_dir(&source)?;
    let output = command("git")
        .args([
            "ls-files",
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
    let target = temporary.join("next-target");
    run(command("cargo")
        .current_dir(&source)
        .env("CARGO_TARGET_DIR", &target)
        .args(["build", "--locked", "--offline", "-p", "atelier-app"]))?;
    Ok(target.join("debug").join(if cfg!(windows) {
        "atelier.exe"
    } else {
        "atelier"
    }))
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
    package::stage(target, binary, assets, &directory)?;
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
    if std::env::var("CI").as_deref() != Ok("true")
        || std::env::var("ATELIER_RUST_FINAL_CAMPAIGN").as_deref() != Ok("1")
    {
        return Err(
            "Ce parcours natif lourd est réservé à la campagne finale CI synthétique.".into(),
        );
    }
    run(command("cargo").args(["build", "--locked", "-p", "atelier-app"]))?;
    let temporary = tempfile::tempdir()?;
    let assets = root().join(".artifacts/rust-ui");
    let current = env!("CARGO_PKG_VERSION");
    let mut parts = payload::version_parts(current)?;
    parts[2] = parts[2]
        .checked_add(1)
        .ok_or("Version de fixture excessive.")?;
    let next = format!("{}.{}.{}", parts[0], parts[1], parts[2]);
    let binary = root().join("target/debug").join(if cfg!(windows) {
        "atelier.exe"
    } else {
        "atelier"
    });
    let successor = next_binary(temporary.path(), &next)?;
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
    if state["cards"]
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
    if state["cards"]
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
