use crate::{Result, command, root, run};
use atelier_app::infrastructure::{
    installation::InstallationMode, installed, launcher, local_http::LocalHttp, package as payload,
    package_archive, session, update_package,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    path::Path,
    process::Stdio,
    time::{Duration, Instant},
};

fn campaign() -> Result<()> {
    if std::env::var("CI").as_deref() != Ok("true")
        || std::env::var("ATELIER_RUST_FINAL_CAMPAIGN").as_deref() != Ok("1")
    {
        return Err("La livraison synthétique est réservée à la campagne finale CI.".into());
    }
    Ok(())
}

async fn cli(binary: &Path, args: &[std::ffi::OsString]) -> Result<()> {
    let mut command = tokio::process::Command::new(binary);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    // The detached Windows server can retain inherited handles after its launcher exits.
    let status = tokio::time::timeout(Duration::from_secs(40), command.status())
        .await
        .map_err(|_| format!("Délai du lanceur dépassé pour {:?}.", args.first()))??;
    if !status.success() {
        return Err("Le lanceur du paquet réel a refusé la commande de qualification.".into());
    }
    Ok(())
}

async fn portable(
    archive: &Path,
    signature: &Path,
    trust: &Path,
    temporary: &Path,
) -> Result<serde_json::Value> {
    let target = payload::native_target()?;
    let installation = temporary.join("portable");
    installed::install(
        archive,
        signature,
        trust,
        &installation,
        InstallationMode::Portable,
        target,
    )?;
    installed::verify_all(&installation, target)?;
    if installed::install(
        archive,
        signature,
        trust,
        &installation,
        InstallationMode::Portable,
        target,
    )
    .is_ok()
    {
        return Err("Une installation existante a été remplacée.".into());
    }
    let data = temporary.join("synthetic-data");
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    drop(listener);
    let binary = installation.join(payload::binary_name(target)?);
    let args = [
        "launch".into(),
        "--data-dir".into(),
        data.clone().into_os_string(),
        "--port".into(),
        port.to_string().into(),
        "--no-browser".into(),
    ];
    let result: Result<serde_json::Value> = async {
        let started=Instant::now();
        cli(&binary, &args).await?;
        let address = format!("http://127.0.0.1:{port}");
        let observed = LocalHttp::new()?
            .authenticated_health(&address, || session::read_optional(&data))
            .await?
            .ok_or("Le paquet n’a pas démarré.")?;
        let startup_ms=started.elapsed().as_millis();
        if observed["version"] != env!("CARGO_PKG_VERSION")
            || observed["protocol"] != "atelier-local/1"
        {
            return Err("Le paquet lancé ne correspond pas à la version livrée.".into());
        }
        let http = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(10))
            .build()?;
        let first_token = session::read(&data)?;
        let card: serde_json::Value = http
            .post(format!("{address}/api/brainstorms"))
            .bearer_auth(&first_token)
            .header("x-local-request", "1")
            .json(&json!({"title":"Synthetic packaged application persistence"}))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let stopping=Instant::now();
        cli(
            &binary,
            &[
                "stop".into(),
                "--data-dir".into(),
                data.clone().into_os_string(),
                "--port".into(),
                port.to_string().into(),
            ],
        )
        .await?;
        let initial_stop_ms=stopping.elapsed().as_millis();
        let restarting=Instant::now();
        cli(&binary, &args).await?;
        let restart_ms=restarting.elapsed().as_millis();
        let token = session::read(&data)?;
        if token == first_token {
            return Err("Le paquet n’a pas renouvelé sa session au redémarrage.".into());
        }
        let reading=Instant::now();
        let state: serde_json::Value = http
            .get(format!("{address}/api/state"))
            .bearer_auth(&token)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let state_read_ms=reading.elapsed().as_millis();
        if state["brainstorms"]
            .as_array()
            .is_none_or(|cards| !cards.contains(&card))
        {
            return Err("Le paquet a perdu une conversation après redémarrage.".into());
        }
        Ok(json!({"startupMs":startup_ms,"initialStopMs":initial_stop_ms,"restartMs":restart_ms,"stateReadMs":state_read_ms,"scope":"single synthetic journey on this native runner","baseline":null,"speedupClaimed":false}))
    }
    .await;
    let stopping_at = Instant::now();
    let stopping = launcher::stop(&data, port).await;
    let mut observations = result?;
    stopping?;
    observations["finalStopMs"] = json!(stopping_at.elapsed().as_millis());
    installed::verify_all(&installation, target)?;
    Ok(observations)
}

pub(super) async fn qualify(output: &Path) -> Result<()> {
    campaign()?;
    if output.try_exists()? {
        return Err("Le dossier de livraison existe déjà ; aucun artefact remplacé.".into());
    }
    let parent = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent)?;
    let work = tempfile::Builder::new()
        .prefix("atelier-final-packages-")
        .tempdir_in(parent)?;
    let delivery = work.path().join("delivery");
    std::fs::create_dir(&delivery)?;
    let target = payload::native_target()?;
    run(command("cargo").args(["build", "--release", "--locked", "-p", "atelier-app"]))?;
    let binary = crate::target()
        .join("release")
        .join(payload::binary_name(target)?);
    let assets = root().join(".artifacts/rust-ui");
    let notices = work.path().join("third-party-notices.txt");
    crate::notices::generate(&notices).await?;
    let staging = work.path().join("payload");
    crate::package::stage(target, &binary, &assets, &notices, &staging)?;
    let archive = delivery.join(format!(
        "atelier-kanban-{}-{target}.zip",
        env!("CARGO_PKG_VERSION")
    ));
    let signature = delivery.join("signed-package.json");
    let keys = work.path().join("keys");
    update_package::generate_synthetic(&keys)?;
    package_archive::create(&staging, &archive)?;
    update_package::sign(&archive, target, &keys, &signature)?;
    let trust = delivery.join(update_package::PUBLIC_KEY);
    std::fs::copy(keys.join(update_package::PUBLIC_KEY), &trust)?;
    let observations = portable(&archive, &signature, &trust, work.path()).await?;
    let mut parts = payload::version_parts(env!("CARGO_PKG_VERSION"))?;
    parts[2] = parts[2]
        .checked_add(1)
        .ok_or("Version de fixture excessive.")?;
    let next = format!("{}.{}.{}", parts[0], parts[1], parts[2]);
    let successor = crate::update_flow::next_binary(work.path(), &next)?;
    let (next_archive, next_signature) =
        crate::update_flow::archive(work.path(), "next", &successor, &assets, &next)?;
    let future = delivery.join("successor");
    std::fs::create_dir(&future)?;
    payload::write(
        &future.join("next.zip"),
        &payload::read(&next_archive, package_archive::ARCHIVE_LIMIT)?,
    )?;
    payload::write(
        &future.join("next.json"),
        &payload::read(&next_signature, 16 * 1024)?,
    )?;
    if !target.contains("apple") {
        crate::installers::build(&archive, &signature, &trust, &delivery.join("installers"))
            .await?;
    }
    if target.contains("linux") {
        crate::installers::build(
            &next_archive,
            &next_signature,
            &trust,
            &future.join("installers"),
        )
        .await?;
        let (bad, bad_signature) =
            crate::update_flow::archive(work.path(), "wrong-runtime", &binary, &assets, &next)?;
        crate::installers::build(
            &bad,
            &bad_signature,
            &trust,
            &delivery.join("negative-installers"),
        )
        .await?;
        let foreign = work.path().join("foreign-keys");
        update_package::generate_synthetic(&foreign)?;
        update_package::sign(
            &next_archive,
            target,
            &foreign,
            &delivery.join("foreign-signed-package.json"),
        )?;
        payload::write(
            &delivery.join("foreign-update-public.json"),
            &payload::read(&foreign.join(update_package::PUBLIC_KEY), 4096)?,
        )?;
    }
    let bytes = payload::read(&archive, 512 * 1024 * 1024)?;
    let archive_size = bytes.len();
    let checksum = atelier_app::infrastructure::session::hexadecimal(&Sha256::digest(bytes));
    payload::write(
        &delivery.join("portable.sha256"),
        format!(
            "{checksum}  {}\n",
            archive
                .file_name()
                .ok_or("Nom d’archive absent.")?
                .to_string_lossy()
        )
        .as_bytes(),
    )?;
    let revision = command("git").args(["rev-parse", "HEAD"]).output()?;
    if !revision.status.success() {
        return Err("Commit de livraison indisponible.".into());
    }
    let report = json!({
        "format":"atelier-rust-package-qualification/1",
        "commit":std::str::from_utf8(&revision.stdout)?.trim(),
        "target":target,"version":env!("CARGO_PKG_VERSION"),
        "archiveBytes":archive_size,"observations":observations,
        "nativeUpdateRollbackQualification":"pending explicit test-updates stage","syntheticSuccessorVersion":next,
        "portable":{"sha256":checksum,"signatureVerified":true,"installationVerified":true,"existingDestinationRefused":true,"nativeLauncherExecuted":true,"runtimeVersionObserved":true,"restartPersistenceObserved":true,"sessionRotated":true,"stopped":true},
        "nativeFormats":{"built":!target.contains("apple"),"qualification":"separate native manager scenario required"},
        "syntheticSignatureOnly":true,"published":false
    });
    payload::write(
        &delivery.join("portable-qualification.json"),
        &serde_json::to_vec_pretty(&report)?,
    )?;
    payload::write(&delivery.join("README.txt"), b"Synthetic preproduction qualification artifacts. NOT a release.\nThe signing key is temporary and has no production authority.\nThe private key is excluded. No Node runtime or native feasibility executable is packaged.\nportable-qualification.json records only journeys actually completed by this command.\nNative installer and update/rollback evidence is recorded separately by the final manual workflow.\n")?;
    payload::publish_directory(&delivery, output)?;
    println!(
        "Qualified portable package and prepared native artifacts for {target}: {}",
        output.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn an_existing_delivery_is_preserved_before_any_build_or_key_generation() {
        let root = tempfile::tempdir().unwrap();
        let output = root.path().join("delivery");
        std::fs::create_dir(&output).unwrap();
        std::fs::write(output.join("human-note"), "preserve").unwrap();
        assert!(super::qualify(&output).await.is_err());
        assert_eq!(
            std::fs::read_to_string(output.join("human-note")).unwrap(),
            "preserve"
        );
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
    }
}
