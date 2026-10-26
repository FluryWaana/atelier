use super::{
    checked_file::{CheckedFile, Identity},
    package,
    package_archive::{self, StagedPackage},
};
use crate::Result;
use base64::{Engine, engine::general_purpose::STANDARD};
use ring::{
    rand::SystemRandom,
    signature::{self, Ed25519KeyPair, KeyPair},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{fs, io::Write, path::Path};

const PRIVATE_KEY: &str = "synthetic-update.pkcs8";
pub const PUBLIC_KEY: &str = "synthetic-update-public.json";

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Trust {
    schema_version: u32,
    application: String,
    key_id: String,
    public_key: String,
    synthetic: bool,
}
impl Trust {
    fn key(&self) -> Result<[u8; 32]> {
        if self.schema_version != 1
            || self.application != "atelier-kanban-rust-update-key"
            || !self.synthetic
            || self.public_key.len() != 44
        {
            return Err("Clé de confiance synthétique de mise à jour invalide.".into());
        }
        let bytes: [u8; 32] = STANDARD
            .decode(&self.public_key)
            .map_err(|_| "Clé publique de mise à jour invalide.")?
            .try_into()
            .map_err(|_| "Taille de clé publique de mise à jour invalide.")?;
        if STANDARD.encode(bytes) != self.public_key || key_id(&bytes) != self.key_id {
            return Err("Empreinte de clé de mise à jour incohérente.".into());
        }
        Ok(bytes)
    }
    pub fn load(path: &Path) -> Result<Self> {
        Self::from_bytes(&package::read(path, 4096)?)
    }
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > 4096 {
            return Err("Clé de confiance trop volumineuse.".into());
        }
        let value: Self = serde_json::from_slice(bytes)?;
        value.key()?;
        Ok(value)
    }
    pub fn key_id(&self) -> &str {
        &self.key_id
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Payload {
    pub schema_version: u32,
    pub application: String,
    pub target: String,
    pub version: String,
    pub archive_size: u64,
    pub archive_sha256: String,
    pub manifest_sha256: String,
    pub key_id: String,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Signed {
    payload: Payload,
    signature: String,
}

pub struct PreparedUpdate {
    package: StagedPackage,
    payload: Payload,
    receipt: Vec<u8>,
}
impl PreparedUpdate {
    pub fn package(&self) -> &StagedPackage {
        &self.package
    }
    pub fn payload(&self) -> &Payload {
        &self.payload
    }
    pub fn receipt(&self) -> &[u8] {
        &self.receipt
    }
}
fn key_id(key: &[u8]) -> String {
    format!("{:x}", Sha256::digest(key))
}
fn private_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

pub fn generate_synthetic(directory: &Path) -> Result<()> {
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(directory)?;
    let document = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new())
        .map_err(|_| "Génération de clé synthétique indisponible.")?;
    let pair = Ed25519KeyPair::from_pkcs8(document.as_ref())
        .map_err(|_| "Clé synthétique générée invalide.")?;
    let public = pair.public_key().as_ref();
    let trust = Trust {
        schema_version: 1,
        application: "atelier-kanban-rust-update-key".into(),
        key_id: key_id(public),
        public_key: STANDARD.encode(public),
        synthetic: true,
    };
    private_write(&directory.join(PRIVATE_KEY), document.as_ref())?;
    private_write(
        &directory.join(PUBLIC_KEY),
        &serde_json::to_vec_pretty(&trust)?,
    )?;
    Ok(())
}

pub fn sign(archive: &Path, target: &str, keys: &Path, output: &Path) -> Result<()> {
    if output.try_exists()? {
        return Err("Le manifeste signé existe déjà. Aucun fichier remplacé.".into());
    }
    let bytes = package::read(archive, package_archive::ARCHIVE_LIMIT)?;
    let scratch = tempfile::tempdir()?;
    let staged = package_archive::unpack(&bytes, target, scratch.path())?;
    let trust = Trust::load(&keys.join(PUBLIC_KEY))?;
    let path = keys.join(PRIVATE_KEY);
    let mut secret = CheckedFile::open(&path, &Identity::inspect(&path)?)?
        .private()?
        .read(4096)?;
    let pair = Ed25519KeyPair::from_pkcs8(&secret)
        .map_err(|_| "Clé privée synthétique de signature invalide.");
    secret.fill(0);
    let pair = pair?;
    if pair.public_key().as_ref() != trust.key()?.as_slice() {
        return Err(
            "La clé de signature ne correspond pas à la confiance synthétique choisie.".into(),
        );
    }
    let manifest = staged.manifest();
    let payload = Payload {
        schema_version: 1,
        application: "atelier-kanban-rust-update".into(),
        target: manifest["target"]
            .as_str()
            .ok_or("Cible à signer absente.")?
            .into(),
        version: manifest["version"]
            .as_str()
            .ok_or("Version à signer absente.")?
            .into(),
        archive_size: bytes.len() as u64,
        archive_sha256: format!("{:x}", Sha256::digest(&bytes)),
        manifest_sha256: format!(
            "{:x}",
            Sha256::digest(package::read(
                &staged.path().join("atelier-package.json"),
                1024 * 1024
            )?)
        ),
        key_id: trust.key_id.clone(),
    };
    let signature = STANDARD.encode(pair.sign(&serde_json::to_vec(&payload)?).as_ref());
    let signed = Signed { payload, signature };
    package::write(output, &serde_json::to_vec_pretty(&signed)?)
}

pub fn authenticated_payload(metadata: &[u8], trust: &Trust, target: &str) -> Result<Payload> {
    if metadata.len() > 16 * 1024 {
        return Err("Manifeste signé trop volumineux.".into());
    }
    let signed: Signed = serde_json::from_slice(metadata)?;
    let payload = &signed.payload;
    if payload.schema_version != 1
        || payload.application != "atelier-kanban-rust-update"
        || payload.key_id != trust.key_id
        || payload.archive_size == 0
        || payload.archive_size > package_archive::ARCHIVE_LIMIT as u64
        || payload.archive_sha256.len() != 64
        || !payload
            .archive_sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        || signed.signature.len() != 88
        || payload.manifest_sha256.len() != 64
        || !payload
            .manifest_sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("Contrat du manifeste signé invalide.".into());
    }
    let key = trust.key()?;
    let signature = STANDARD
        .decode(&signed.signature)
        .map_err(|_| "Signature de mise à jour invalide.")?;
    if signature.len() != 64 || STANDARD.encode(&signature) != signed.signature {
        return Err("Signature de mise à jour invalide.".into());
    }
    signature::UnparsedPublicKey::new(&signature::ED25519, key)
        .verify(&serde_json::to_vec(payload)?, &signature)
        .map_err(|_| "Signature de mise à jour non reconnue par la clé de confiance.")?;
    package::binary_name(&payload.target)?;
    if payload.target != target {
        return Err("Cette mise à jour cible une autre plateforme ou architecture.".into());
    }
    package::version_parts(&payload.version)?;
    Ok(signed.payload)
}

fn authenticate(metadata: &[u8], trust: &Trust, target: &str, current: &str) -> Result<Payload> {
    let payload = authenticated_payload(metadata, trust, target)?;
    if package::version_parts(&payload.version)? <= package::version_parts(current)? {
        return Err("La mise à jour doit être strictement plus récente ; version identique et retour arrière refusés.".into());
    }
    Ok(payload)
}

pub fn verify_installed_payload(
    directory: &Path,
    receipt: &[u8],
    trust: &Trust,
    target: &str,
) -> Result<Payload> {
    let payload = authenticated_payload(receipt, trust, target)?;
    package::verify(directory)?;
    let bytes = package::read(&directory.join("atelier-package.json"), 1024 * 1024)?;
    let manifest: serde_json::Value = serde_json::from_slice(&bytes)?;
    if format!("{:x}", Sha256::digest(&bytes)) != payload.manifest_sha256
        || manifest["version"] != payload.version
        || manifest["target"] != payload.target
    {
        return Err("La version installée ne correspond pas à son manifeste signé.".into());
    }
    Ok(payload)
}

pub fn prepare(
    archive: &Path,
    metadata: &Path,
    trust: &Trust,
    target: &str,
    current: &str,
    parent: &Path,
) -> Result<PreparedUpdate> {
    let metadata = package::read(metadata, 16 * 1024)?;
    let payload = authenticate(&metadata, trust, target, current)?;
    let bytes = package::read(archive, usize::try_from(payload.archive_size)?)?;
    if bytes.len() as u64 != payload.archive_size
        || format!("{:x}", Sha256::digest(&bytes)) != payload.archive_sha256
    {
        return Err("L’archive ne correspond pas à la taille et à l’empreinte signées.".into());
    }
    let package = package_archive::unpack(&bytes, target, parent)?;
    if package.manifest()["version"] != payload.version
        || package.manifest()["target"] != payload.target
        || format!(
            "{:x}",
            Sha256::digest(super::package::read(
                &package.path().join("atelier-package.json"),
                1024 * 1024
            )?)
        ) != payload.manifest_sha256
    {
        return Err(
            "Le contenu du paquet ne correspond pas à la version et à la cible signées.".into(),
        );
    }
    Ok(PreparedUpdate {
        package,
        payload,
        receipt: metadata,
    })
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use serde_json::json;
    pub(crate) fn fixture() -> (tempfile::TempDir, Trust) {
        let root = tempfile::tempdir().unwrap();
        let payload = root.path().join("payload");
        fs::create_dir(&payload).unwrap();
        fs::create_dir(payload.join("ui")).unwrap();
        let mut image = vec![0u8; 64];
        image[..6].copy_from_slice(b"\x7fELF\x02\x01");
        image[18] = 62;
        package::write(&payload.join("atelier"), &image).unwrap();
        package::write(&payload.join("LICENSE"), b"Synthetic license fixture").unwrap();
        package::write(&payload.join("README.txt"), b"Synthetic package").unwrap();
        for (name, bytes) in [
            ("index.html", &b"Synthetic Rust"[..]),
            ("atelier-loader.js", &b"synthetic"[..]),
            ("atelier-ui-test.js", &b"synthetic"[..]),
            ("atelier-ui-test_bg.wasm", &b"\0asm\x01\0\0\0"[..]),
            ("style-test.css", &b"synthetic"[..]),
        ] {
            package::write(&payload.join("ui").join(name), bytes).unwrap();
        }
        let manifest = package::manifest(&payload, package::TARGETS[1], "0.2.0").unwrap();
        package::write(
            &payload.join("atelier-package.json"),
            &serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        package_archive::create(&payload, &root.path().join("update.zip")).unwrap();
        let keys = root.path().join("keys");
        generate_synthetic(&keys).unwrap();
        let trust = Trust::load(&keys.join(PUBLIC_KEY)).unwrap();
        sign(
            &root.path().join("update.zip"),
            package::TARGETS[1],
            &keys,
            &root.path().join("update.json"),
        )
        .unwrap();
        fs::create_dir(root.path().join("staging")).unwrap();
        (root, trust)
    }
    #[test]
    fn signature_target_and_version_are_checked_before_archive_access_and_extraction() {
        let (root, trust) = fixture();
        let archive = root.path().join("update.zip");
        let metadata = root.path().join("update.json");
        let staging = root.path().join("staging");
        let accepted = prepare(
            &archive,
            &metadata,
            &trust,
            package::TARGETS[1],
            "0.1.9",
            &staging,
        )
        .unwrap();
        assert_eq!(accepted.payload().version, "0.2.0");
        assert_eq!(accepted.package().manifest()["version"], "0.2.0");
        drop(accepted);
        assert_eq!(fs::read_dir(&staging).unwrap().count(), 0);
        let missing = root.path().join("must-not-be-read.zip");
        for (target, current) in [
            (package::TARGETS[2], "0.1.9"),
            (package::TARGETS[1], "0.2.0"),
            (package::TARGETS[1], "1.0.0"),
        ] {
            let error = prepare(&missing, &metadata, &trust, target, current, &staging)
                .err()
                .unwrap()
                .to_string();
            assert!(error.contains("plateforme") || error.contains("strictement plus récente"));
        }
        let mut tampered: serde_json::Value =
            serde_json::from_slice(&fs::read(&metadata).unwrap()).unwrap();
        tampered["payload"]["archiveSha256"] = json!("0".repeat(64));
        fs::write(&metadata, serde_json::to_vec(&tampered).unwrap()).unwrap();
        let error = prepare(
            &missing,
            &metadata,
            &trust,
            package::TARGETS[1],
            "0.1.9",
            &staging,
        )
        .err()
        .unwrap()
        .to_string();
        assert!(error.contains("Signature"));
        assert_eq!(fs::read_dir(&staging).unwrap().count(), 0);
    }
    #[test]
    fn foreign_keys_corrupted_archives_and_replacing_existing_outputs_are_refused() {
        let (root, trust) = fixture();
        let archive = root.path().join("update.zip");
        let metadata = root.path().join("update.json");
        let keys = root.path().join("keys");
        let staging = root.path().join("staging");
        let original = fs::read(keys.join(PRIVATE_KEY)).unwrap();
        assert!(generate_synthetic(&keys).is_err());
        assert!(fs::read(keys.join(PRIVATE_KEY)).unwrap() == original);
        let before = fs::read(&metadata).unwrap();
        assert!(sign(&archive, package::TARGETS[1], &keys, &metadata).is_err());
        assert!(fs::read(&metadata).unwrap() == before);
        let foreign = root.path().join("foreign");
        generate_synthetic(&foreign).unwrap();
        let foreign = Trust::load(&foreign.join(PUBLIC_KEY)).unwrap();
        assert!(
            prepare(
                &archive,
                &metadata,
                &foreign,
                package::TARGETS[1],
                "0.1.9",
                &staging
            )
            .is_err()
        );
        let mut corrupted = fs::read(&archive).unwrap();
        corrupted[0] = b'X';
        fs::write(&archive, corrupted).unwrap();
        let error = prepare(
            &archive,
            &metadata,
            &trust,
            package::TARGETS[1],
            "0.1.9",
            &staging,
        )
        .err()
        .unwrap()
        .to_string();
        assert!(error.contains("empreinte signées"));
        assert_eq!(fs::read_dir(&staging).unwrap().count(), 0);
    }

    #[test]
    fn installation_publishes_only_a_verified_new_root_and_preserves_existing_data() {
        use super::super::{
            installation::{self, InstallationMode},
            installed,
        };
        let (root, _) = fixture();
        let destination = root.path().join("installed");
        let archive = root.path().join("update.zip");
        let metadata = root.path().join("update.json");
        let trust = root.path().join("keys").join(PUBLIC_KEY);
        installed::install(
            &archive,
            &metadata,
            &trust,
            &destination,
            InstallationMode::Portable,
            package::TARGETS[1],
        )
        .unwrap();
        assert_eq!(
            installation::mode(&destination).unwrap(),
            InstallationMode::Portable
        );
        let installation = installed::open(&destination, package::TARGETS[1])
            .unwrap()
            .unwrap();
        assert_eq!(installation.current().active.version, "0.2.0");
        installed::verify_all(&destination, package::TARGETS[1]).unwrap();
        assert!(
            installed::locate(&destination.join("atelier"), package::TARGETS[1])
                .unwrap()
                .is_some()
        );
        assert!(
            installed::locate(
                &installation.active_executable().unwrap(),
                package::TARGETS[1]
            )
            .unwrap()
            .is_some()
        );
        fs::create_dir(destination.join("data")).unwrap();
        fs::write(
            destination.join("data/human-synthetic"),
            "Preserve this synthetic data",
        )
        .unwrap();
        assert!(
            installed::install(
                &archive,
                &metadata,
                &trust,
                &destination,
                InstallationMode::User,
                package::TARGETS[1]
            )
            .is_err()
        );
        assert_eq!(
            fs::read_to_string(destination.join("data/human-synthetic")).unwrap(),
            "Preserve this synthetic data"
        );
        let slot = installation
            .slot_path(&installation.current().active)
            .unwrap();
        fs::write(slot.join("ui/style-test.css"), "Human edit").unwrap();
        let changed = package::manifest(&slot, package::TARGETS[1], "0.2.0").unwrap();
        fs::write(
            slot.join("atelier-package.json"),
            serde_json::to_vec(&changed).unwrap(),
        )
        .unwrap();
        assert!(installed::open(&destination, package::TARGETS[1]).is_err());
        assert!(installed::verify_all(&destination, package::TARGETS[1]).is_err());
        assert_eq!(
            fs::read_to_string(slot.join("ui/style-test.css")).unwrap(),
            "Human edit"
        );
        assert_eq!(
            fs::read_to_string(destination.join("data/human-synthetic")).unwrap(),
            "Preserve this synthetic data"
        );
    }
}
