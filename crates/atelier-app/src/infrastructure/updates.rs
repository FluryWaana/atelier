use super::{
    checked_file::CheckedFile,
    data_lock::DataLock,
    installed::{self, Current, Installation, Version},
    launcher,
    local_http::LocalHttp,
    package, publish, session, update_package,
};
use crate::Result;
use atelier_core::application::updates::{File, Operation, Phase, Status};
use cap_fs_ext::DirExt;
use cap_std::{ambient_authority, fs::Dir};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{process::Command, time::Instant};

const JOURNAL: &str = "update-operation.json";
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Journal {
    format: String,
    operation: Operation,
    expected: Current,
    next: Version,
    data: PathBuf,
    data_identity: String,
    port: u16,
    restart: bool,
    worker_pid: Option<u32>,
}

fn load(directory: &Dir) -> Result<Option<Journal>> {
    match directory.symlink_metadata(JOURNAL) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
        Ok(_) => {}
    }
    let journal: Journal = serde_json::from_slice(
        &CheckedFile::open_in(directory, Path::new(JOURNAL))?.read(64 * 1024)?,
    )?;
    if journal.format != "atelier-rust-update/1"
        || uuid::Uuid::parse_str(&journal.operation.id).is_err()
        || journal.port == 0
        || !journal.data.is_absolute()
        || journal.data_identity.len() != 64
        || !journal
            .data_identity
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
        || journal.operation.files.len() > 132
        || journal.operation.message.len() > 4000
    {
        return Err("Journal de mise à jour invalide ; aucun remplacement automatique.".into());
    }
    Ok(Some(journal))
}
fn save(installation: &Installation, journal: &Journal) -> Result<()> {
    let bytes = serde_json::to_vec(journal)?;
    if bytes.len() > 64 * 1024 {
        return Err("Journal de mise à jour trop volumineux.".into());
    }
    publish::write_private(installation.directory(), Path::new(JOURNAL), &bytes, false)
}
fn phase(
    installation: &Installation,
    journal: &mut Journal,
    state: Phase,
    message: &str,
) -> Result<()> {
    journal.operation.state = state;
    journal.operation.message = message.into();
    save(installation, journal)
}
fn lock(installation: &Installation) -> Result<DataLock> {
    match installation.directory().create_dir("update-lock") {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.into()),
    }
    let directory = installation.directory().open_dir_nofollow("update-lock")?;
    DataLock::acquire_in(&installation.root().join("update-lock"), &directory)
}
async fn wait_lock(installation: &Installation) -> Result<DataLock> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match lock(installation) {
            Ok(lock) => return Ok(lock),
            Err(error) => {
                if Instant::now() >= deadline || !error.to_string().contains("déjà ouvert") {
                    return Err(error);
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
fn data_identity(data: &Path) -> Result<(PathBuf, String, Dir)> {
    if data.to_str().is_none_or(|value| value.len() > 2000) {
        return Err("Chemin de données de mise à jour invalide ou trop long.".into());
    }
    let data = std::fs::canonicalize(data)?;
    let directory = Dir::open_ambient_dir(&data, ambient_authority())?;
    CheckedFile::open_in(&directory, Path::new(".atelier-rust-prototype"))?.read(0)?;
    let identity = String::from_utf8(
        CheckedFile::open_in(&directory, Path::new(".atelier-prototype-vault"))?
            .private()?
            .read(64)?,
    )?;
    if identity.len() != 64 || !identity.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("Identité du dossier de données Rust invalide.".into());
    }
    Ok((data, identity, directory))
}
fn validate_data(journal: &Journal) -> Result<()> {
    let (path, identity, _directory) = data_identity(&journal.data)?;
    if path != journal.data || identity != journal.data_identity {
        return Err("Le dossier de données a changé depuis la confirmation.".into());
    }
    Ok(())
}
async fn data_lock(journal: &Journal) -> Result<DataLock> {
    let (data, identity, directory) = data_identity(&journal.data)?;
    if identity != journal.data_identity {
        return Err("Le dossier de données a changé depuis la confirmation.".into());
    }
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        match DataLock::acquire_in(&data, &directory) {
            Ok(lock) => return Ok(lock),
            Err(error) => {
                if Instant::now() >= deadline || !error.to_string().contains("déjà ouvert") {
                    return Err(error);
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

pub fn maintenance(root: &Path) -> Result<bool> {
    let directory = Dir::open_ambient_dir(root, ambient_authority())?;
    Ok(load(&directory)?.is_some_and(|journal| journal.operation.state.maintenance()))
}
pub fn expected_data_identity(root: &Path, data: &Path) -> Result<Option<String>> {
    let directory = Dir::open_ambient_dir(root, ambient_authority())?;
    let Some(journal) = load(&directory)?.filter(|journal| journal.operation.state.maintenance())
    else {
        return Ok(None);
    };
    if std::fs::canonicalize(data)? != journal.data {
        return Err("La maintenance est liée à un autre dossier de données.".into());
    }
    validate_data(&journal)?;
    Ok(Some(journal.data_identity))
}
pub fn status(root: Option<&Path>) -> Result<Status> {
    let target = package::native_target()?.to_owned();
    let Some(root) = root else {
        return Ok(Status {
            installed: false,
            current_version: env!("CARGO_PKG_VERSION").into(),
            target,
            operation: None,
        });
    };
    let directory = Dir::open_ambient_dir(root, ambient_authority())?;
    let current: Current = serde_json::from_slice(
        &CheckedFile::open_in(&directory, Path::new("current.json"))?.read(16 * 1024)?,
    )?;
    Ok(Status {
        installed: true,
        current_version: current.active.version,
        target,
        operation: load(&directory)?.map(|journal| journal.operation),
    })
}

pub async fn preview(
    root: &Path,
    archive: &Path,
    metadata: &Path,
    data: &Path,
    port: u16,
) -> Result<Operation> {
    if port == 0 {
        return Err("Port de mise à jour invalide.".into());
    }
    let mut installation = installed::open(root, package::native_target()?)?
        .ok_or("Installation versionnée Rust requise.")?;
    let _lock = lock(&installation)?;
    if load(installation.directory())?.is_some_and(|journal| journal.operation.state.pending()) {
        return Err(
            "Une préparation ou mise à jour existe déjà. Relis-la ou annule-la avant de continuer."
                .into(),
        );
    }
    let (data, data_identity, _directory) = data_identity(data)?;
    let signed = update_package::authenticated_payload(
        &package::read(metadata, 16 * 1024)?,
        installation.trust(),
        &installation.current().active.target,
    )?;
    if package::version_parts(&signed.version)?
        <= package::version_parts(&installation.current().active.version)?
    {
        return Err("Cette mise à jour n’est pas plus récente que la version active.".into());
    }
    let next = Version::from_payload(&signed);
    let operation = Operation {
        id: uuid::Uuid::new_v4().to_string(),
        state: Phase::Preparing,
        from_version: installation.current().active.version.clone(),
        to_version: next.version.clone(),
        key_id: signed.key_id,
        sha256: next.id.clone(),
        created_at: time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)?,
        message: "Préparation de la version signée ; la version active reste disponible.".into(),
        files: Vec::new(),
    };
    let mut journal = Journal {
        format: "atelier-rust-update/1".into(),
        operation: operation.clone(),
        expected: installation.current().clone(),
        next,
        data,
        data_identity,
        port,
        restart: false,
        worker_pid: None,
    };
    save(&installation, &journal)?;
    let result: Result<()> = async {
        let observed = LocalHttp::new()?
            .authenticated_health(&format!("http://127.0.0.1:{port}"), || {
                session::read_optional(&journal.data)
            })
            .await?;
        if let Some(observed) = &observed {
            if observed["protocol"] != "atelier-local/1"
                || observed["version"] != journal.expected.active.version
            {
                return Err(
                    "La version exécutée ne correspond pas à la version active installée.".into(),
                );
            }
        }
        journal.restart = observed.is_some();
        let prepared = update_package::prepare(
            archive,
            metadata,
            installation.trust(),
            &journal.next.target,
            &journal.expected.active.version,
            installation.root(),
        )?;
        if Version::from_payload(prepared.payload()) != journal.next {
            return Err("Le paquet signé a changé pendant sa préparation.".into());
        }
        installation.stage_update(&prepared)?;
        journal.operation.files = prepared.package().manifest()["files"]
            .as_array()
            .ok_or("Inventaire de mise à jour absent.")?
            .iter()
            .map(|file| {
                Ok(File {
                    path: file["path"]
                        .as_str()
                        .ok_or("Chemin de mise à jour absent.")?
                        .into(),
                    size: file["size"]
                        .as_u64()
                        .ok_or("Taille de mise à jour absente.")?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        installation.refresh_current()?;
        if installation.current() != &journal.expected {
            return Err("La version active a changé pendant la préparation.".into());
        }
        phase(
            &installation,
            &mut journal,
            Phase::Prepared,
            "Version vérifiée et préparée. Aucun service arrêté et aucune donnée remplacée.",
        )
    }
    .await;
    if let Err(error) = result {
        phase(
            &installation,
            &mut journal,
            Phase::Failed,
            &format!(
                "Préparation interrompue : {}",
                error.to_string().chars().take(500).collect::<String>()
            ),
        )?;
        return Err(error);
    }
    Ok(journal.operation)
}

pub async fn apply(root: &Path, id: &str, confirmed: bool) -> Result<Operation> {
    if !confirmed {
        return Err("Confirme la mise à jour de la version préparée.".into());
    }
    let installation =
        installed::open(root, package::native_target()?)?.ok_or("Installation Rust absente.")?;
    let _lock = lock(&installation)?;
    let mut journal = load(installation.directory())?
        .filter(|journal| journal.operation.id == id && journal.operation.state == Phase::Prepared)
        .ok_or("Cette préparation de mise à jour n’est plus disponible.")?;
    if installation.current() != &journal.expected {
        return Err("La version active a changé depuis l’aperçu.".into());
    }
    installation.verify_version(&journal.next)?;
    if data_identity(&journal.data)?.1 != journal.data_identity {
        return Err("Le dossier de données a changé depuis l’aperçu.".into());
    }
    phase(
        &installation,
        &mut journal,
        Phase::Waiting,
        "Arrêt contrôlé avant changement de version.",
    )?;
    let executable = installation
        .slot_path(&journal.next)?
        .join(package::binary_name(&journal.next.target)?);
    let mut command = Command::new(executable);
    command
        .arg("update-worker")
        .arg("--root")
        .arg(installation.root())
        .arg("--operation")
        .arg(&journal.operation.id)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    command.creation_flags(0x08000000 | 0x00000200);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.as_std_mut().process_group(0);
    }
    match command.spawn() {
        Ok(child) => {
            journal.worker_pid = child.id();
            save(&installation, &journal)?;
        }
        Err(_) => {
            phase(
                &installation,
                &mut journal,
                Phase::Failed,
                "Le worker n’a pas démarré. La version active est conservée.",
            )?;
            return Err("Démarrage du worker de mise à jour impossible.".into());
        }
    }
    Ok(journal.operation)
}

pub fn cancel(root: &Path, id: &str, confirmed: bool) -> Result<Operation> {
    if !confirmed {
        return Err("Confirme l’annulation de la préparation.".into());
    }
    let installation =
        installed::open(root, package::native_target()?)?.ok_or("Installation Rust absente.")?;
    let _lock = lock(&installation)?;
    let mut journal = load(installation.directory())?
        .filter(|journal| {
            journal.operation.id == id
                && matches!(journal.operation.state, Phase::Preparing | Phase::Prepared)
        })
        .ok_or("Préparation à annuler introuvable.")?;
    phase(
        &installation,
        &mut journal,
        Phase::Cancelled,
        "Préparation annulée. Les données et la version active sont conservées.",
    )?;
    Ok(journal.operation)
}

fn switched(journal: &Journal) -> Current {
    Current {
        format: journal.expected.format.clone(),
        active: journal.next.clone(),
        previous: Some(journal.expected.active.clone()),
    }
}
async fn running_version(journal: &Journal, expected: &str) -> Result<()> {
    let observed = LocalHttp::new()?
        .authenticated_health(&format!("http://127.0.0.1:{}", journal.port), || {
            session::read_optional(&journal.data)
        })
        .await?
        .ok_or("La version démarrée ne répond pas.")?;
    if observed["version"] != expected {
        return Err("La version exécutée ne correspond pas à la version signée.".into());
    }
    Ok(())
}
async fn rollback(
    installation: &mut Installation,
    journal: &mut Journal,
    reason: Option<&str>,
) -> Result<()> {
    installation.refresh_current()?;
    if installation.current() != &switched(journal) && installation.current() != &journal.expected {
        return Err(
            "Le pointeur a changé hors de cette mise à jour. Il est conservé pour examen.".into(),
        );
    }
    validate_data(journal)?;
    phase(
        installation,
        journal,
        Phase::RollingBack,
        "Retour à la version précédente ; les données ne sont ni restaurées ni effacées.",
    )?;
    launcher::stop(&journal.data, journal.port).await?;
    let guard = data_lock(journal).await?;
    installation.refresh_current()?;
    if installation.current() == &switched(journal) {
        installation.write_current(&switched(journal), journal.expected.clone())?;
    } else if installation.current() != &journal.expected {
        return Err(
            "Le pointeur a changé hors de cette mise à jour. Il est conservé pour examen.".into(),
        );
    }
    drop(guard);
    if journal.restart {
        launcher::start(
            &installation.active_executable()?,
            &journal.data,
            &installation.slot_path(&journal.expected.active)?.join("ui"),
            journal.port,
            false,
        )
        .await?;
        running_version(journal, &journal.expected.active.version).await?;
    }
    let message = reason
        .map(|reason| {
            format!(
                "Mise à jour refusée : {reason} Version précédente rétablie ; données conservées."
            )
        })
        .unwrap_or_else(|| {
            "Version précédente rétablie. Aucune donnée restaurée ni supprimée.".into()
        });
    phase(installation, journal, Phase::RolledBack, &message)
}

pub async fn worker(root: &Path, id: &str, recovery: bool) -> Result<()> {
    let mut installation = installed::open_recovery(root, package::native_target()?)?
        .ok_or("Installation Rust absente.")?;
    let _lock = wait_lock(&installation).await?;
    let mut journal = load(installation.directory())?
        .filter(|journal| journal.operation.id == id)
        .ok_or("Journal de cette mise à jour absent.")?;
    if !journal.operation.state.maintenance() {
        return Ok(());
    }
    let executable = std::fs::canonicalize(std::env::current_exe()?)?;
    let bootstrap = std::fs::canonicalize(
        installation
            .root()
            .join(package::binary_name(&journal.next.target)?),
    )?;
    let candidate = installation
        .slot_path(&journal.next)?
        .join(package::binary_name(&journal.next.target)?);
    if executable != bootstrap
        && !std::fs::canonicalize(&candidate).is_ok_and(|candidate| executable == candidate)
    {
        return Err("Ce worker n’appartient pas à cette installation.".into());
    }
    let result = if recovery {
        rollback(&mut installation, &mut journal, None).await
    } else {
        async {
            if journal.operation.state != Phase::Waiting
                || installation.current() != &journal.expected
            {
                return Err("État de départ de mise à jour modifié.".into());
            }
            installation.verify_version(&journal.next)?;
            validate_data(&journal)?;
            launcher::stop(&journal.data, journal.port).await?;
            let guard = data_lock(&journal).await?;
            phase(
                &installation,
                &mut journal,
                Phase::Activating,
                "Activation de la version vérifiée.",
            )?;
            installation.write_current(&journal.expected, switched(&journal))?;
            phase(
                &installation,
                &mut journal,
                Phase::Trying,
                "Vérification du démarrage sans navigateur.",
            )?;
            drop(guard);
            launcher::start(
                &candidate,
                &journal.data,
                &installation.slot_path(&journal.next)?.join("ui"),
                journal.port,
                false,
            )
            .await?;
            running_version(&journal, &journal.next.version).await?;
            installation.refresh_current()?;
            if installation.current() != &switched(&journal) {
                return Err(
                    "Le pointeur a changé pendant la vérification. Il est conservé pour examen."
                        .into(),
                );
            }
            if !journal.restart {
                launcher::stop(&journal.data, journal.port).await?;
            }
            phase(
                &installation,
                &mut journal,
                Phase::Committed,
                "Mise à jour démarrée et vérifiée. Données conservées.",
            )
        }
        .await
    };
    if let Err(error) = result {
        let reason = error.to_string().chars().take(500).collect::<String>();
        if !recovery
            && rollback(&mut installation, &mut journal, Some(&reason))
                .await
                .is_ok()
        {
            return Err(error);
        }
        let message = format!(
            "Récupération interrompue : {}. Aucun fichier de données remplacé.",
            error.to_string().chars().take(500).collect::<String>()
        );
        phase(&installation, &mut journal, Phase::Uncertain, &message)?;
        return Err(error);
    }
    Ok(())
}

pub async fn recover_if_needed(executable: &Path) -> Result<()> {
    let parent = executable.parent().ok_or("Dossier du lanceur absent.")?;
    let root = if parent.join("installation.json").try_exists()? {
        parent.to_owned()
    } else if parent
        .parent()
        .is_some_and(|path| path.file_name().is_some_and(|name| name == "versions"))
    {
        parent
            .parent()
            .and_then(Path::parent)
            .ok_or("Racine d’installation absente.")?
            .to_owned()
    } else {
        return Ok(());
    };
    let directory = Dir::open_ambient_dir(&root, ambient_authority())?;
    if let Some(mut journal) = load(&directory)? {
        if journal.operation.state == Phase::Preparing {
            let installation = installed::open_recovery(&root, package::native_target()?)?
                .ok_or("Installation Rust absente.")?;
            if let Ok(_lock) = lock(&installation) {
                phase(
                    &installation,
                    &mut journal,
                    Phase::Failed,
                    "Préparation interrompue. La version active et les données sont conservées.",
                )?;
            }
        } else if journal.operation.state.maintenance() {
            drop(directory);
            worker(&root, &journal.operation.id, true).await?;
        }
    }
    Ok(())
}

pub async fn worker_command(
    args: impl Iterator<Item = std::ffi::OsString>,
    recovery: bool,
) -> Result<()> {
    let mut values = std::collections::BTreeMap::new();
    let mut args = args;
    while let Some(name) = args.next() {
        let name = name.to_str().ok_or("Option worker invalide.")?.to_owned();
        if !["--root", "--operation"].contains(&name.as_str()) || values.contains_key(&name) {
            return Err("Option worker inconnue ou répétée.".into());
        }
        values.insert(name, args.next().ok_or("Valeur worker absente.")?);
    }
    let root = values
        .get("--root")
        .map(PathBuf::from)
        .ok_or("Racine worker absente.")?;
    let id = values
        .get("--operation")
        .and_then(|value| value.to_str())
        .ok_or("Identifiant worker absent.")?;
    worker(&root, id, recovery).await
}

pub async fn request_recovery(root: &Path, id: &str, confirmed: bool) -> Result<Operation> {
    if !confirmed {
        return Err("Confirme la récupération de cette mise à jour.".into());
    }
    let installation = installed::open_recovery(root, package::native_target()?)?
        .ok_or("Installation Rust absente.")?;
    let _lock = lock(&installation)?;
    let mut journal = load(installation.directory())?
        .filter(|journal| journal.operation.id == id && journal.operation.state.maintenance())
        .ok_or("Aucune mise à jour à récupérer.")?;
    phase(
        &installation,
        &mut journal,
        Phase::RollingBack,
        "Récupération demandée ; arrêt contrôlé avant retour de version.",
    )?;
    let mut command = Command::new(
        installation
            .root()
            .join(package::binary_name(&journal.next.target)?),
    );
    command
        .arg("recover-update")
        .arg("--root")
        .arg(installation.root())
        .arg("--operation")
        .arg(id)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    command.creation_flags(0x08000000 | 0x00000200);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.as_std_mut().process_group(0);
    }
    let child = command.spawn().map_err(
        |_| "Le worker de récupération n’a pas démarré. Relance le lanceur de l’installation.",
    )?;
    journal.worker_pid = child.id();
    save(&installation, &journal)?;
    Ok(journal.operation)
}

pub async fn command(name: &str, args: impl Iterator<Item = std::ffi::OsString>) -> Result<()> {
    let executable = std::env::current_exe()?;
    let installation = installed::locate(&executable, package::native_target()?)?
        .ok_or("Cette commande exige une installation Rust versionnée.")?;
    let mut values = std::collections::BTreeMap::new();
    let mut confirmed = false;
    let mut args = args;
    while let Some(flag) = args.next() {
        let flag = flag
            .to_str()
            .ok_or("Option de mise à jour invalide.")?
            .to_owned();
        if flag == "--confirm" && !confirmed && name != "update-preview" && name != "update-status"
        {
            confirmed = true;
            continue;
        }
        let allowed = if name == "update-preview" {
            ["--archive", "--signature", "--data-dir", "--port"].contains(&flag.as_str())
        } else {
            flag == "--operation" && name != "update-status"
        };
        if !allowed || values.contains_key(&flag) {
            return Err("Option de mise à jour inconnue ou répétée.".into());
        }
        values.insert(
            flag,
            args.next()
                .filter(|value| !value.is_empty())
                .ok_or("Valeur de mise à jour absente.")?,
        );
    }
    let required = |name: &str| {
        values
            .get(name)
            .map(PathBuf::from)
            .ok_or_else(|| crate::Error::from("Argument de mise à jour obligatoire absent."))
    };
    let operation = match name {
        "update-preview" => {
            let port = values
                .get("--port")
                .map(|value| -> Result<u16> {
                    Ok(value.to_str().ok_or("Port invalide.")?.parse::<u16>()?)
                })
                .transpose()?
                .unwrap_or(4317);
            Some(
                preview(
                    installation.root(),
                    &required("--archive")?,
                    &required("--signature")?,
                    &required("--data-dir")?,
                    port,
                )
                .await?,
            )
        }
        "update-status" => {
            println!(
                "{}",
                serde_json::to_string_pretty(&status(Some(installation.root()))?)?
            );
            None
        }
        _ => {
            let id = values
                .get("--operation")
                .and_then(|value| value.to_str())
                .ok_or("Identifiant de mise à jour absent.")?;
            Some(match name {
                "update-apply" => apply(installation.root(), id, confirmed).await?,
                "update-cancel" => cancel(installation.root(), id, confirmed)?,
                "update-recover" => request_recovery(installation.root(), id, confirmed).await?,
                _ => return Err("Commande de mise à jour inconnue.".into()),
            })
        }
    };
    if let Some(operation) = operation {
        println!("{}", serde_json::to_string_pretty(&operation)?);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::{
        installation::InstallationMode, package_archive, prototype::PrototypeDirectory,
        update_package::tests::fixture,
    };
    use super::*;
    fn prepared_installation() -> (tempfile::TempDir, Installation, Journal) {
        let (root, trust) = fixture();
        let destination = root.path().join("installed");
        installed::install(
            &root.path().join("update.zip"),
            &root.path().join("update.json"),
            &root.path().join("keys/synthetic-update-public.json"),
            &destination,
            InstallationMode::Portable,
            package::TARGETS[1],
        )
        .unwrap();
        let installation = installed::open(&destination, package::TARGETS[1])
            .unwrap()
            .unwrap();
        let payload = root.path().join("payload");
        let manifest = package::manifest(&payload, package::TARGETS[1], "0.3.0").unwrap();
        std::fs::write(
            payload.join("atelier-package.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        let archive = root.path().join("next.zip");
        let metadata = root.path().join("next.json");
        package_archive::create(&payload, &archive).unwrap();
        update_package::sign(
            &archive,
            package::TARGETS[1],
            &root.path().join("keys"),
            &metadata,
        )
        .unwrap();
        let prepared = update_package::prepare(
            &archive,
            &metadata,
            &trust,
            package::TARGETS[1],
            "0.2.0",
            root.path(),
        )
        .unwrap();
        let next = installation.stage_update(&prepared).unwrap();
        let data = root.path().join("synthetic-data");
        let handle = PrototypeDirectory::open(&data).unwrap();
        handle.vault_service().unwrap();
        drop(handle);
        std::fs::write(
            data.join("kanban.sqlite"),
            b"Synthetic human bytes; updater must never open or replace this database",
        )
        .unwrap();
        let (data, data_identity, _) = super::data_identity(&data).unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let journal = Journal {
            format: "atelier-rust-update/1".into(),
            operation: Operation {
                id: uuid::Uuid::new_v4().to_string(),
                state: Phase::Prepared,
                from_version: "0.2.0".into(),
                to_version: "0.3.0".into(),
                key_id: trust.key_id().into(),
                sha256: next.id.clone(),
                created_at: "2026-10-26T00:00:00Z".into(),
                message: "Synthetic update only".into(),
                files: Vec::new(),
            },
            expected: installation.current().clone(),
            next,
            data,
            data_identity,
            port,
            restart: false,
            worker_pid: None,
        };
        (root, installation, journal)
    }
    #[tokio::test]
    async fn recovery_at_each_switch_boundary_preserves_exact_data_bytes_and_human_edits() {
        for (state, activated) in [
            (Phase::Waiting, false),
            (Phase::Activating, false),
            (Phase::Activating, true),
            (Phase::Trying, true),
            (Phase::RollingBack, true),
            (Phase::Uncertain, true),
        ] {
            let (_root, mut installation, mut journal) = prepared_installation();
            journal.operation.state = state;
            save(&installation, &journal).unwrap();
            let bytes = std::fs::read(journal.data.join("kanban.sqlite")).unwrap();
            if activated {
                installation
                    .write_current(&journal.expected, switched(&journal))
                    .unwrap();
            }
            if state == Phase::Uncertain {
                std::fs::write(
                    installation
                        .slot_path(&journal.next)
                        .unwrap()
                        .join("ui/style-test.css"),
                    b"Human edit in the broken candidate",
                )
                .unwrap();
            }
            let path = installation.root().to_owned();
            drop(installation);
            let mut installation = installed::open_recovery(&path, package::TARGETS[1])
                .unwrap()
                .unwrap();
            rollback(&mut installation, &mut journal, None)
                .await
                .unwrap();
            assert!(installation.current() == &journal.expected);
            assert_eq!(journal.operation.state, Phase::RolledBack);
            assert!(std::fs::read(journal.data.join("kanban.sqlite")).unwrap() == bytes);
            if state == Phase::Uncertain {
                assert_eq!(
                    std::fs::read(
                        installation
                            .slot_path(&journal.next)
                            .unwrap()
                            .join("ui/style-test.css")
                    )
                    .unwrap(),
                    b"Human edit in the broken candidate"
                );
            }
        }
    }
    #[tokio::test]
    async fn recovery_refuses_changed_data_identity_and_external_pointer_edits_without_overwrite() {
        let (_root, mut installation, mut journal) = prepared_installation();
        let bytes = std::fs::read(journal.data.join("kanban.sqlite")).unwrap();
        let identity = std::fs::read(journal.data.join(".atelier-prototype-vault")).unwrap();
        std::fs::write(
            journal.data.join(".atelier-prototype-vault"),
            "a".repeat(64),
        )
        .unwrap();
        assert!(
            rollback(&mut installation, &mut journal, None)
                .await
                .is_err()
        );
        assert!(installation.current() == &journal.expected);
        std::fs::write(journal.data.join(".atelier-prototype-vault"), identity).unwrap();
        let pointer = installation.root().join("current.json");
        let human = br#"{"format":"human-edit","active":{}}"#;
        std::fs::write(&pointer, human).unwrap();
        assert!(
            rollback(&mut installation, &mut journal, None)
                .await
                .is_err()
        );
        assert!(std::fs::read(&pointer).unwrap() == human);
        assert!(std::fs::read(journal.data.join("kanban.sqlite")).unwrap() == bytes);
    }
    #[test]
    fn corrupted_journals_fail_closed_and_staging_never_changes_the_active_pointer() {
        let (_root, installation, journal) = prepared_installation();
        assert!(installation.current() == &journal.expected);
        assert!(installation.verify_version(&journal.next).is_ok());
        save(&installation, &journal).unwrap();
        assert!(!maintenance(installation.root()).unwrap());
        std::fs::write(
            installation.root().join(JOURNAL),
            b"partial synthetic journal",
        )
        .unwrap();
        assert!(maintenance(installation.root()).is_err());
        assert!(installation.current() == &journal.expected);
    }
}
