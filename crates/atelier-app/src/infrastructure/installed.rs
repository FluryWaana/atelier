use super::{
    checked_file::{CheckedFile, plain_metadata},
    installation::InstallationMode,
    package, publish,
    update_package::{self, Payload, PreparedUpdate, Trust},
};
use crate::Result;
use cap_fs_ext::DirExt;
use cap_std::{ambient_authority, fs::Dir};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    ffi::OsString,
    fs,
    path::{Component, Path, PathBuf},
};

const FORMAT: &str = "atelier-rust-installation/1";
const TRUST: &str = "trusted-update-key.json";
const CURRENT: &str = "current.json";

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Configuration {
    format: String,
    mode: InstallationMode,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Version {
    pub id: String,
    pub version: String,
    pub target: String,
}
impl Version {
    fn validate(&self) -> Result<()> {
        if self.id.len() != 64
            || !self
                .id
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err("Identité de version installée invalide.".into());
        }
        package::version_parts(&self.version)?;
        package::binary_name(&self.target)?;
        Ok(())
    }
    pub fn from_payload(payload: &Payload) -> Self {
        Self {
            id: payload.archive_sha256.clone(),
            version: payload.version.clone(),
            target: payload.target.clone(),
        }
    }
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Current {
    pub format: String,
    pub active: Version,
    pub previous: Option<Version>,
}

pub struct Installation {
    root: PathBuf,
    directory: Dir,
    current: Current,
    trust: Trust,
}
impl Installation {
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn current(&self) -> &Current {
        &self.current
    }
    pub fn trust(&self) -> &Trust {
        &self.trust
    }
    pub fn slot_path(&self, version: &Version) -> Result<PathBuf> {
        version.validate()?;
        Ok(self.root.join("versions").join(&version.id))
    }
    pub fn verify_version(&self, version: &Version) -> Result<Payload> {
        version.validate()?;
        let versions = self.directory.open_dir_nofollow("versions")?;
        let _slot = versions.open_dir_nofollow(&version.id)?;
        let receipts = self.directory.open_dir_nofollow("receipts")?;
        let receipt = CheckedFile::open_in(&receipts, Path::new(&format!("{}.json", version.id)))?
            .read(16 * 1024)?;
        let payload = update_package::verify_installed_payload(
            &self.slot_path(version)?,
            &receipt,
            &self.trust,
            &version.target,
        )?;
        if Version::from_payload(&payload) != *version {
            return Err("Le pointeur de version ne correspond pas à son reçu signé.".into());
        }
        Ok(payload)
    }
    pub fn active_executable(&self) -> Result<PathBuf> {
        Ok(self
            .slot_path(&self.current.active)?
            .join(package::binary_name(&self.current.active.target)?))
    }
    pub(super) fn directory(&self) -> &Dir {
        &self.directory
    }
    pub(super) fn refresh_current(&mut self) -> Result<()> {
        let current: Current = serde_json::from_slice(
            &CheckedFile::open_in(&self.directory, Path::new(CURRENT))?.read(16 * 1024)?,
        )?;
        if current.format != FORMAT
            || current.active.target != self.current.active.target
            || current.previous.as_ref().is_some_and(|previous| {
                previous.target != current.active.target || previous.id == current.active.id
            })
        {
            return Err("Pointeur de version modifié hors du contrat Rust.".into());
        }
        current.active.validate()?;
        if let Some(previous) = &current.previous {
            previous.validate()?;
        }
        self.current = current;
        Ok(())
    }
    pub(super) fn write_current(&mut self, expected: &Current, next: Current) -> Result<()> {
        let actual: Current = serde_json::from_slice(
            &CheckedFile::open_in(&self.directory, Path::new(CURRENT))?.read(16 * 1024)?,
        )?;
        if &actual != expected
            || next.format != FORMAT
            || next.active.target != expected.active.target
            || next.previous.as_ref().is_some_and(|previous| {
                previous.target != next.active.target || previous.id == next.active.id
            })
        {
            return Err("Le choix de version a changé. Aucun pointeur remplacé.".into());
        }
        self.verify_version(&next.active)?;
        if let Some(previous) = &next.previous {
            previous.validate()?;
        }
        publish::write_private(
            &self.directory,
            Path::new(CURRENT),
            &serde_json::to_vec(&next)?,
            false,
        )?;
        self.current = next;
        Ok(())
    }
    pub(super) fn stage_update(&self, prepared: &PreparedUpdate) -> Result<Version> {
        let payload = update_package::verify_installed_payload(
            prepared.package().path(),
            prepared.receipt(),
            &self.trust,
            &self.current.active.target,
        )?;
        if package::version_parts(&payload.version)?
            <= package::version_parts(&self.current.active.version)?
        {
            return Err("Cette version n’est pas plus récente que la version active.".into());
        }
        let version = Version::from_payload(&payload);
        let versions = self.directory.open_dir_nofollow("versions")?;
        let receipts = self.directory.open_dir_nofollow("receipts")?;
        let receipt_name = format!("{}.json", version.id);
        if versions.symlink_metadata(&version.id).is_ok() {
            self.verify_version(&version)?;
            return Ok(version);
        }
        let mut slots = Vec::new();
        for entry in versions.entries()? {
            let name = entry?.file_name();
            if let Some(name) = name.to_str().filter(|name| {
                name.len() == 64 && name.bytes().all(|byte| byte.is_ascii_hexdigit())
            }) {
                slots.push(name.to_owned());
            }
        }
        if slots.len() >= 3 {
            for id in &slots {
                if id == &self.current.active.id
                    || self
                        .current
                        .previous
                        .as_ref()
                        .is_some_and(|version| &version.id == id)
                {
                    continue;
                }
                let receipt = CheckedFile::open_in(&receipts, Path::new(&format!("{id}.json")))?
                    .read(16 * 1024)?;
                let payload = update_package::authenticated_payload(
                    &receipt,
                    &self.trust,
                    &self.current.active.target,
                )?;
                let version = Version::from_payload(&payload);
                if version.id != *id {
                    continue;
                }
                self.verify_version(&version)?;
                publish::rename_new(
                    &versions,
                    Path::new(id),
                    Path::new(&format!(".retired-{id}-{}", uuid::Uuid::new_v4())),
                )?;
            }
            slots.retain(|id| {
                id == &self.current.active.id
                    || self
                        .current
                        .previous
                        .as_ref()
                        .is_some_and(|version| &version.id == id)
            });
        }
        if slots.len() >= 3 {
            return Err("Trois versions ou préparations sont déjà conservées. Examine les anciennes versions avant de poursuivre.".into());
        }
        let mut count = 0usize;
        let mut bytes = 0u64;
        measure(&versions, 0, &mut count, &mut bytes)?;
        let incoming = prepared.package().manifest()["files"]
            .as_array()
            .ok_or("Inventaire de mise à jour absent.")?
            .iter()
            .try_fold(0u64, |total, file| {
                total
                    .checked_add(
                        file["size"]
                            .as_u64()
                            .ok_or("Taille de mise à jour absente.")?,
                    )
                    .ok_or("Taille cumulée de mise à jour invalide.")
            })?;
        if bytes
            .checked_add(incoming)
            .is_none_or(|total| total > 1024 * 1024 * 1024)
        {
            return Err(
                "La réserve de versions dépasserait 1 Gio. Aucun fichier supprimé automatiquement."
                    .into(),
            );
        }
        let temporary = tempfile::Builder::new()
            .prefix(".atelier-version-")
            .tempdir_in(self.root.join("versions"))?;
        copy_payload_contents(prepared, temporary.path())?;
        update_package::verify_installed_payload(
            temporary.path(),
            prepared.receipt(),
            &self.trust,
            &version.target,
        )?;
        match receipts.symlink_metadata(&receipt_name) {
            Ok(_) => {
                if CheckedFile::open_in(&receipts, Path::new(&receipt_name))?.read(16 * 1024)?
                    != prepared.receipt()
                {
                    return Err("Le reçu de cette version a été modifié ; il est conservé.".into());
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                package::write(
                    &self.root.join("receipts").join(&receipt_name),
                    prepared.receipt(),
                )?;
            }
            Err(error) => return Err(error.into()),
        }
        // Publish the verified directory as one unit; an interrupted copy never becomes active.
        let source = temporary
            .path()
            .file_name()
            .ok_or("Nom du temporaire de version absent.")?;
        publish::rename_new(&versions, Path::new(source), Path::new(&version.id))?;
        let _ = temporary.keep();
        Ok(version)
    }
    #[cfg(target_os = "linux")]
    pub(super) fn stage_system_update(&self, prepared: PreparedUpdate) -> Result<Version> {
        if super::installation::mode(&self.root)? != InstallationMode::System {
            return Err("Installation système explicite requise.".into());
        }
        let payload = update_package::verify_installed_payload(
            prepared.package().path(),
            prepared.receipt(),
            &self.trust,
            &self.current.active.target,
        )?;
        if package::version_parts(&payload.version)?
            <= package::version_parts(&self.current.active.version)?
        {
            return Err("Cette version système n’est pas plus récente.".into());
        }
        let next = Version::from_payload(&payload);
        let versions = self.directory.open_dir_nofollow("versions")?;
        if versions.symlink_metadata(&next.id).is_ok() {
            self.verify_version(&next)?;
            return Ok(next);
        }
        let mut count = 0;
        let mut bytes = 0;
        measure(&versions, 0, &mut count, &mut bytes)?;
        let incoming_directory = super::repository_access::absolute(prepared.package().path())?;
        let mut incoming_count = 0;
        let mut incoming = 0;
        measure(&incoming_directory, 0, &mut incoming_count, &mut incoming)?;
        let slots = versions
            .entries()?
            .take(16)
            .collect::<std::io::Result<Vec<_>>>()?;
        if bytes
            .checked_add(incoming)
            .is_none_or(|total| total > 1024 * 1024 * 1024)
            || slots.len() >= 16
        {
            return Err(
                "Réserve système limitée à 1 Gio et seize versions ; aucun contenu supprimé."
                    .into(),
            );
        }
        let receipts = self.directory.open_dir_nofollow("receipts")?;
        let name = format!("{}.json", next.id);
        match receipts.symlink_metadata(&name) {
            Ok(_) => {
                if CheckedFile::open_in(&receipts, Path::new(&name))?.read(16 * 1024)?
                    != prepared.receipt()
                {
                    return Err("Reçu système existant différent ; contenu conservé.".into());
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                publish::write_private(&receipts, Path::new(&name), prepared.receipt(), false)?
            }
            Err(error) => return Err(error.into()),
        }
        receipts.try_clone()?.into_std_file().sync_all()?;
        let retained = prepared.keep_package();
        let source = super::repository_access::absolute(
            retained
                .parent()
                .ok_or("Parent de préparation système absent.")?,
        )?;
        publish::rename_new_between(
            &source,
            retained.file_name().ok_or("Préparation système absente.")?,
            &versions,
            std::ffi::OsStr::new(&next.id),
        )?;
        self.verify_version(&next)?;
        Ok(next)
    }
    fn verify_bootstrap(&self, target: &str) -> Result<()> {
        let receipt = CheckedFile::open_in(&self.directory, Path::new("bootstrap-receipt.json"))?
            .read(16 * 1024)?;
        let payload = update_package::authenticated_payload(&receipt, &self.trust, target)?;
        let bytes = CheckedFile::open_in(&self.directory, Path::new("bootstrap-manifest.json"))?
            .read(1024 * 1024)?;
        if format!("{:x}", Sha256::digest(&bytes)) != payload.manifest_sha256 {
            return Err("Manifeste du lanceur installé modifié.".into());
        }
        let manifest: serde_json::Value = serde_json::from_slice(&bytes)?;
        if manifest["version"] != payload.version || manifest["target"] != payload.target {
            return Err("Identité du lanceur installé incohérente.".into());
        }
        let binary = package::binary_name(target)?;
        let entries = manifest["files"]
            .as_array()
            .ok_or("Inventaire du lanceur absent.")?
            .iter()
            .filter(|record| record["path"] == binary)
            .collect::<Vec<_>>();
        if entries.len() != 1 {
            return Err("Identité du binaire lanceur ambiguë.".into());
        }
        let image =
            CheckedFile::open_in(&self.directory, Path::new(binary))?.read(128 * 1024 * 1024)?;
        package::executable(target, &image)?;
        if entries[0]["size"].as_u64() != Some(image.len() as u64)
            || entries[0]["sha256"] != format!("{:x}", Sha256::digest(&image))
        {
            return Err("Le binaire du lanceur installé a été modifié.".into());
        }
        Ok(())
    }
}

fn measure(directory: &Dir, depth: usize, count: &mut usize, bytes: &mut u64) -> Result<()> {
    if depth > 3 {
        return Err("Arborescence des versions trop profonde.".into());
    }
    for entry in directory.entries()? {
        let entry = entry?;
        *count += 1;
        if *count > 1024 {
            return Err("Trop de fichiers dans la réserve de versions.".into());
        }
        let name = entry.file_name();
        let metadata = directory.symlink_metadata(&name)?;
        super::checked_file::require_plain(&metadata)?;
        if metadata.is_dir() {
            measure(
                &directory.open_dir_nofollow(&name)?,
                depth + 1,
                count,
                bytes,
            )?;
        } else {
            *bytes = bytes
                .checked_add(metadata.len())
                .filter(|size| *size <= 1024 * 1024 * 1024)
                .ok_or("Réserve de versions trop volumineuse.")?;
        }
    }
    Ok(())
}

pub fn open(root: &Path, target: &str) -> Result<Option<Installation>> {
    open_mode(root, target, true)
}

pub fn verify_all(root: &Path, target: &str) -> Result<()> {
    let installation = open(root, target)?.ok_or("Installation Rust absente.")?;
    let mut ids = std::collections::BTreeSet::new();
    for entry in fs::read_dir(root.join("versions"))? {
        let entry = entry?;
        let id = entry
            .file_name()
            .into_string()
            .map_err(|_| "Version installée non reconnue.")?;
        if id.len() != 64
            || !id
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            || !plain_metadata(&entry.path())?.is_dir()
        {
            return Err("Contenu conservé : une version modifiée ou retirée exige un examen avant désinstallation.".into());
        }
        if !ids.insert(id.clone()) || ids.len() > 16 {
            return Err(
                "Trop de versions pour une désinstallation automatique. Aucun contenu supprimé."
                    .into(),
            );
        }
        let receipt = package::read(&root.join("receipts").join(format!("{id}.json")), 16 * 1024)?;
        let payload = update_package::verify_installed_payload(
            &entry.path(),
            &receipt,
            installation.trust(),
            target,
        )?;
        if payload.archive_sha256 != id {
            return Err("Identité de version installée incohérente.".into());
        }
    }
    if !ids.contains(&installation.current().active.id) {
        return Err("Version active absente de l’inventaire installé.".into());
    }
    for entry in fs::read_dir(root.join("receipts"))? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_str().ok_or("Nom de reçu invalide.")?;
        if !name
            .strip_suffix(".json")
            .is_some_and(|id| ids.contains(id))
            || !plain_metadata(&entry.path())?.is_file()
        {
            return Err("Reçu sans version conservé : examen requis avant désinstallation.".into());
        }
    }
    Ok(())
}

pub(super) fn open_recovery(root: &Path, target: &str) -> Result<Option<Installation>> {
    open_mode(root, target, false)
}

fn open_mode(root: &Path, target: &str, verify_active: bool) -> Result<Option<Installation>> {
    package::binary_name(target)?;
    if !plain_metadata(root)?.is_dir() {
        return Err("Dossier d’installation ordinaire requis.".into());
    }
    let root = fs::canonicalize(root)?;
    let directory = Dir::open_ambient_dir(&root, ambient_authority())?;
    match directory.symlink_metadata("installation.json") {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if directory.symlink_metadata(CURRENT).is_ok() {
                return Err("Installation Rust incomplète : identité absente.".into());
            }
            return Ok(None);
        }
        Err(error) => return Err(error.into()),
        Ok(_) => {}
    }
    let configuration: Configuration = serde_json::from_slice(
        &CheckedFile::open_in(&directory, Path::new("installation.json"))?.read(4096)?,
    )?;
    if configuration.format != FORMAT {
        return Err("Format d’installation Rust non pris en charge.".into());
    }
    let current: Current = serde_json::from_slice(
        &CheckedFile::open_in(&directory, Path::new(CURRENT))?.read(16 * 1024)?,
    )?;
    if current.format != FORMAT
        || current.active.target != target
        || current
            .previous
            .as_ref()
            .is_some_and(|previous| previous.target != target || previous.id == current.active.id)
    {
        return Err("Pointeur de version installée invalide.".into());
    }
    current.active.validate()?;
    if let Some(previous) = &current.previous {
        previous.validate()?;
    }
    let trust =
        Trust::from_bytes(&CheckedFile::open_in(&directory, Path::new(TRUST))?.read(4096)?)?;
    let installed = Installation {
        root,
        directory,
        current,
        trust,
    };
    installed.verify_bootstrap(target)?;
    if verify_active {
        installed.verify_version(&installed.current.active)?;
    }
    Ok(Some(installed))
}

pub fn locate(executable: &Path, target: &str) -> Result<Option<Installation>> {
    let parent = executable
        .parent()
        .ok_or("Dossier du lanceur introuvable.")?;
    if let Some(installation) = open(parent, target)? {
        return Ok(Some(installation));
    }
    if let Some(versions) = parent
        .parent()
        .filter(|path| path.file_name().is_some_and(|name| name == "versions"))
    {
        let root = versions
            .parent()
            .ok_or("Racine de version installée absente.")?;
        let installation = open(root, target)?
            .ok_or("Cette version n’appartient pas à une installation Rust complète.")?;
        if fs::canonicalize(parent)?
            != fs::canonicalize(installation.slot_path(&installation.current.active)?)?
        {
            return Err(
                "Cette version n’est plus active. Utilise le lanceur de l’installation.".into(),
            );
        }
        return Ok(Some(installation));
    }
    Ok(None)
}

fn copy_payload(prepared: &PreparedUpdate, destination: &Path) -> Result<()> {
    fs::create_dir(destination)?;
    copy_payload_contents(prepared, destination)
}

fn copy_payload_contents(prepared: &PreparedUpdate, destination: &Path) -> Result<()> {
    fs::create_dir(destination.join("ui"))?;
    let binary = package::binary_name(&prepared.payload().target)?;
    for record in prepared.package().manifest()["files"]
        .as_array()
        .ok_or("Inventaire du paquet absent.")?
    {
        let name = record["path"].as_str().ok_or("Chemin du paquet absent.")?;
        let bytes = package::read(&prepared.package().path().join(name), 128 * 1024 * 1024)?;
        package::write(&destination.join(name), &bytes)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(
                destination.join(name),
                fs::Permissions::from_mode(if name == binary { 0o755 } else { 0o644 }),
            )?;
        }
    }
    #[cfg(not(unix))]
    let _ = binary;
    package::write(
        &destination.join("atelier-package.json"),
        &package::read(
            &prepared.package().path().join("atelier-package.json"),
            1024 * 1024,
        )?,
    )?;
    Ok(())
}

pub fn install(
    archive: &Path,
    metadata: &Path,
    trust_file: &Path,
    destination: &Path,
    mode: InstallationMode,
    target: &str,
) -> Result<()> {
    let destination = std::path::absolute(destination)?;
    let leaf = destination
        .file_name()
        .filter(|leaf| !leaf.is_empty())
        .ok_or("Dossier de destination distinct requis.")?;
    if !matches!(
        Path::new(leaf).components().collect::<Vec<_>>().as_slice(),
        [Component::Normal(_)]
    ) || destination.to_str().is_none()
    {
        return Err("Nom de dossier d’installation invalide.".into());
    }
    let parent = destination
        .parent()
        .ok_or("Dossier parent d’installation absent.")?;
    if !plain_metadata(parent)?.is_dir() {
        return Err("Dossier parent d’installation ordinaire requis.".into());
    }
    let parent = fs::canonicalize(parent)?;
    let directory = Dir::open_ambient_dir(&parent, ambient_authority())?;
    if directory.symlink_metadata(leaf).is_ok() {
        return Err("La destination existe déjà. Aucun dossier ni donnée remplacés.".into());
    }
    let trust = Trust::load(trust_file)?;
    let prepared = update_package::prepare(archive, metadata, &trust, target, "0.0.0", &parent)?;
    let stage = tempfile::Builder::new()
        .prefix(".atelier-install-")
        .tempdir_in(&parent)?;
    fs::create_dir(stage.path().join("versions"))?;
    fs::create_dir(stage.path().join("receipts"))?;
    let version = Version::from_payload(prepared.payload());
    let slot = stage.path().join("versions").join(&version.id);
    copy_payload(&prepared, &slot)?;
    package::write(
        &stage
            .path()
            .join("receipts")
            .join(format!("{}.json", version.id)),
        prepared.receipt(),
    )?;
    package::write(
        &stage.path().join("bootstrap-receipt.json"),
        prepared.receipt(),
    )?;
    package::write(
        &stage.path().join("bootstrap-manifest.json"),
        &package::read(&slot.join("atelier-package.json"), 1024 * 1024)?,
    )?;
    package::write(&stage.path().join(TRUST), &serde_json::to_vec(&trust)?)?;
    package::write(
        &stage.path().join("installation.json"),
        &serde_json::to_vec(&Configuration {
            format: FORMAT.into(),
            mode,
        })?,
    )?;
    package::write(
        &stage.path().join(CURRENT),
        &serde_json::to_vec(&Current {
            format: FORMAT.into(),
            active: version,
            previous: None,
        })?,
    )?;
    let binary = package::binary_name(target)?;
    package::write(
        &stage.path().join(binary),
        &package::read(&slot.join(binary), 128 * 1024 * 1024)?,
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(stage.path().join(binary), fs::Permissions::from_mode(0o755))?;
        fs::set_permissions(stage.path(), fs::Permissions::from_mode(0o755))?;
    }
    drop(open(stage.path(), target)?.ok_or("Préparation d’installation absente.")?);
    let source = stage
        .path()
        .file_name()
        .ok_or("Nom du temporaire d’installation absent.")?;
    publish::rename_new(&directory, Path::new(source), Path::new(leaf))?;
    let _ = stage.keep();
    Ok(())
}

pub fn install_command(args: impl Iterator<Item = OsString>) -> Result<()> {
    let mut args = args;
    let mut values = std::collections::BTreeMap::new();
    while let Some(flag) = args.next() {
        let flag = flag
            .to_str()
            .ok_or("Option d’installation invalide.")?
            .to_owned();
        if ![
            "--archive",
            "--signature",
            "--trust",
            "--destination",
            "--mode",
        ]
        .contains(&flag.as_str())
            || values.contains_key(&flag)
        {
            return Err("Option d’installation inconnue ou répétée.".into());
        }
        values.insert(
            flag,
            args.next()
                .filter(|value| !value.is_empty())
                .ok_or("Valeur d’installation absente.")?,
        );
    }
    let required = |name: &str| {
        values.get(name).map(PathBuf::from).ok_or_else(|| {
            crate::Error::from(
                "Archive, signature, confiance et destination d’installation obligatoires.",
            )
        })
    };
    let mode = match values.get("--mode") {
        None => InstallationMode::User,
        Some(value) => match value.to_str() {
            Some("portable") => InstallationMode::Portable,
            Some("user") => InstallationMode::User,
            Some("system") => InstallationMode::System,
            _ => return Err("Mode d’installation inconnu.".into()),
        },
    };
    install(
        &required("--archive")?,
        &required("--signature")?,
        &required("--trust")?,
        &required("--destination")?,
        mode,
        package::native_target()?,
    )
}
