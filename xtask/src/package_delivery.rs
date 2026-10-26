use crate::{Result, command, root, run};
use atelier_app::infrastructure::{
    installation::InstallationMode, installed, launcher, local_http::LocalHttp, package as payload,
    package_archive, session, update_package,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{path::Path, process::Stdio, time::Duration};

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
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    let output = tokio::time::timeout(Duration::from_secs(40), command.output()).await??;
    if !output.status.success() {
        eprintln!("{}", String::from_utf8_lossy(&output.stderr));
        return Err("Le lanceur du paquet réel a refusé la commande de qualification.".into());
    }
    Ok(())
}

async fn portable(archive: &Path, signature: &Path, trust: &Path, temporary: &Path) -> Result<()> {
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
    let result: Result<()> = async {
        cli(&binary, &args).await?;
        let address = format!("http://127.0.0.1:{port}");
        let observed = LocalHttp::new()?
            .authenticated_health(&address, || session::read_optional(&data))
            .await?
            .ok_or("Le paquet n’a pas démarré.")?;
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
        cli(&binary, &args).await?;
        let token = session::read(&data)?;
        if token == first_token {
            return Err("Le paquet n’a pas renouvelé sa session au redémarrage.".into());
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
            return Err("Le paquet a perdu une conversation après redémarrage.".into());
        }
        Ok(())
    }
    .await;
    let stopping = launcher::stop(&data, port).await;
    result?;
    stopping?;
    installed::verify_all(&installation, target)?;
    Ok(())
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
    let binary = root()
        .join("target/release")
        .join(payload::binary_name(target)?);
    let assets = root().join(".artifacts/rust-ui");
    let notices = work.path().join("THIRD-PARTY-NOTICES.txt");
    crate::notices::generate(&notices).await?;
    let staging = work.path().join("payload");
    crate::package::stage(target, &binary, &assets, &notices, &staging)?;
    let archive = delivery.join(format!(
        "atelier-kanban-{}-{target}.zip",
        env!("CARGO_PKG_VERSION")
    ));
    let signature = delivery.join("signed-package.json");
    let keys = work.path().join("synthetic-keys-not-for-delivery");
    update_package::generate_synthetic(&keys)?;
    package_archive::create(&staging, &archive)?;
    update_package::sign(&archive, target, &keys, &signature)?;
    let trust = delivery.join(update_package::PUBLIC_KEY);
    std::fs::copy(keys.join(update_package::PUBLIC_KEY), &trust)?;
    portable(&archive, &signature, &trust, work.path()).await?;
    if !target.contains("apple") {
        crate::installers::build(&archive, &signature, &trust, &delivery.join("installers"))
            .await?;
    }
    let bytes = payload::read(&archive, 512 * 1024 * 1024)?;
    let checksum = format!("{:x}", Sha256::digest(bytes));
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
