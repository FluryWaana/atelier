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
const RELEASE_PUBLIC_KEY: &str = "Me0AEzOtX3PmBRk70z5cCW81EFsYENLNOxcG1qpEWOs=";

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
            || (!self.synthetic && self.public_key != RELEASE_PUBLIC_KEY)
            || self.public_key.len() != 44
        {
            return Err("Clé de confiance de mise à jour invalide.".into());
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
    pub fn release() -> Result<Self> {
        let bytes = STANDARD.decode(RELEASE_PUBLIC_KEY)?;
        let trust = Self {
            schema_version: 1,
            application: "atelier-kanban-rust-update-key".into(),
            key_id: key_id(&bytes),
            public_key: RELEASE_PUBLIC_KEY.into(),
            synthetic: false,
        };
        trust.key()?;
        Ok(trust)
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
    #[cfg(target_os = "linux")]
    pub(super) fn keep_package(self) -> std::path::PathBuf {
        self.package.keep()
    }
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
    crate::infrastructure::session::hexadecimal(&Sha256::digest(key))
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
    let builder = fs::DirBuilder::new();
    #[cfg(unix)]
    let builder = {
        use std::os::unix::fs::DirBuilderExt;
        let mut builder = builder;
        builder.mode(0o700);
        builder
    };
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
    let trust = Trust::load(&keys.join(PUBLIC_KEY))?;
    let payload = signing_payload(archive, target, output, &trust)?;
    let path = keys.join(PRIVATE_KEY);
    let mut secret = CheckedFile::open(&path, &Identity::inspect(&path)?)?
        .private()?
        .read(4096)?;
    let pair = Ed25519KeyPair::from_pkcs8(&secret)
        .map_err(|_| "Clé privée synthétique de signature invalide.");
    secret.fill(0);
    sign_with_pair(payload, &pair?, &trust, output)
}

fn pem_key(pem: &[u8]) -> Result<Ed25519KeyPair> {
    if pem.len() > 4096 {
        return Err("Clé privée de signature trop volumineuse.".into());
    }
    let pem = std::str::from_utf8(pem).map_err(|_| "Clé privée PEM invalide.")?;
    let mut lines = pem.lines();
    if lines.next() != Some("-----BEGIN PRIVATE KEY-----") {
        return Err("Clé privée PEM PKCS8 requise.".into());
    }
    let mut encoded = Vec::new();
    let result = (|| {
        let mut ended = false;
        for line in lines.by_ref() {
            if line == "-----END PRIVATE KEY-----" {
                ended = true;
                break;
            }
            if line.is_empty()
                || !line
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/' | b'='))
            {
                return Err("Clé privée PEM invalide.".into());
            }
            encoded.extend_from_slice(line.as_bytes());
        }
        if !ended || lines.next().is_some() || encoded.is_empty() {
            return Err("Un seul document PEM PKCS8 est requis.".into());
        }
        let mut secret = STANDARD
            .decode(&encoded)
            .map_err(|_| "Encodage de clé privée PEM invalide.")?;
        // OpenSSL exports PKCS8 v1 without the public key. The caller checks the pinned key.
        let pair = Ed25519KeyPair::from_pkcs8_maybe_unchecked(&secret)
            .map_err(|_| "Clé privée Ed25519 PKCS8 invalide.");
        secret.fill(0);
        Ok(pair?)
    })();
    encoded.fill(0);
    result
}

pub fn sign_release(archive: &Path, target: &str, output: &Path) -> Result<()> {
    let trust = Trust::release()?;
    let payload = signing_payload(archive, target, output, &trust)?;
    sign_with_pair(payload, &release_pair()?, &trust, output)
}

fn release_pair() -> Result<Ed25519KeyPair> {
    let mut secret = std::env::var("RELEASE_SIGNING_KEY")
        .map_err(|_| "Variable RELEASE_SIGNING_KEY absente ou invalide.")?
        .into_bytes();
    let pair = pem_key(&secret);
    secret.fill(0);
    let pair = pair?;
    require_key(&pair, &Trust::release()?)?;
    Ok(pair)
}

fn checksum_message(input: &Path) -> Result<Vec<u8>> {
    let checksums = package::read(input, 64 * 1024)?;
    if checksums.is_empty() {
        return Err("La liste des empreintes est vide.".into());
    }
    let mut message = b"atelier-kanban-release-checksums/1\n".to_vec();
    message.extend_from_slice(&checksums);
    Ok(message)
}

pub fn sign_release_checksums(input: &Path, output: &Path) -> Result<()> {
    if output.try_exists()? {
        return Err("La signature existe déjà. Aucun fichier remplacé.".into());
    }
    let message = checksum_message(input)?;
    let pair = release_pair()?;
    package::write(
        output,
        STANDARD.encode(pair.sign(&message).as_ref()).as_bytes(),
    )
}

pub fn verify_release_checksums(input: &Path, signature: &Path) -> Result<()> {
    verify_checksums(input, signature, &Trust::release()?.key()?)
}

fn verify_checksums(input: &Path, signature: &Path, key: &[u8; 32]) -> Result<()> {
    let message = checksum_message(input)?;
    let encoded = package::read(signature, 88)?;
    let signature = STANDARD
        .decode(&encoded)
        .map_err(|_| "Signature des empreintes invalide.")?;
    if signature.len() != 64 || STANDARD.encode(&signature).as_bytes() != encoded.as_slice() {
        return Err("Signature des empreintes invalide.".into());
    }
    signature::UnparsedPublicKey::new(&signature::ED25519, key)
        .verify(&message, &signature)
        .map_err(|_| "Signature des empreintes non reconnue.".into())
}

fn signing_payload(archive: &Path, target: &str, output: &Path, trust: &Trust) -> Result<Payload> {
    if output.try_exists()? {
        return Err("Le manifeste signé existe déjà. Aucun fichier remplacé.".into());
    }
    let bytes = package::read(archive, package_archive::ARCHIVE_LIMIT)?;
    let scratch = tempfile::tempdir()?;
    let staged = package_archive::unpack(&bytes, target, scratch.path())?;
    let manifest = staged.manifest();
    Ok(Payload {
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
        archive_sha256: crate::infrastructure::session::hexadecimal(&Sha256::digest(&bytes)),
        manifest_sha256: crate::infrastructure::session::hexadecimal(&Sha256::digest(
            package::read(&staged.path().join("atelier-package.json"), 1024 * 1024)?,
        )),
        key_id: trust.key_id.clone(),
    })
}

fn sign_with_pair(
    payload: Payload,
    pair: &Ed25519KeyPair,
    trust: &Trust,
    output: &Path,
) -> Result<()> {
    require_key(pair, trust)?;
    let signature = STANDARD.encode(pair.sign(&serde_json::to_vec(&payload)?).as_ref());
    let signed = Signed { payload, signature };
    package::write(output, &serde_json::to_vec_pretty(&signed)?)
}

fn require_key(pair: &Ed25519KeyPair, trust: &Trust) -> Result<()> {
    if pair.public_key().as_ref() != trust.key()?.as_slice() {
        return Err("La clé de signature ne correspond pas à la confiance choisie.".into());
    }
    Ok(())
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
    if crate::infrastructure::session::hexadecimal(&Sha256::digest(&bytes))
        != payload.manifest_sha256
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
        || crate::infrastructure::session::hexadecimal(&Sha256::digest(&bytes))
            != payload.archive_sha256
    {
        return Err("L’archive ne correspond pas à la taille et à l’empreinte signées.".into());
    }
    let package = package_archive::unpack(&bytes, target, parent)?;
    if package.manifest()["version"] != payload.version
        || package.manifest()["target"] != payload.target
        || crate::infrastructure::session::hexadecimal(&Sha256::digest(super::package::read(
            &package.path().join("atelier-package.json"),
            1024 * 1024,
        )?)) != payload.manifest_sha256
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

    fn pem(bytes: &[u8]) -> Vec<u8> {
        format!(
            "-----BEGIN PRIVATE KEY-----\n{}\n-----END PRIVATE KEY-----\n",
            STANDARD.encode(bytes)
        )
        .into_bytes()
    }

    #[test]
    fn release_trust_is_pinned_and_synthetic_keys_cannot_claim_release_authority() {
        let release = Trust::release().unwrap();
        assert!(!release.synthetic);
        assert_eq!(STANDARD.encode(release.key().unwrap()), RELEASE_PUBLIC_KEY);
        let roundtrip = Trust::from_bytes(&serde_json::to_vec(&release).unwrap()).unwrap();
        assert_eq!(roundtrip.key_id(), release.key_id());
        let root = tempfile::tempdir().unwrap();
        generate_synthetic(&root.path().join("keys")).unwrap();
        let mut synthetic = Trust::load(&root.path().join("keys").join(PUBLIC_KEY)).unwrap();
        let document = fs::read(root.path().join("keys").join(PRIVATE_KEY)).unwrap();
        let pair = Ed25519KeyPair::from_pkcs8(&document).unwrap();
        assert!(require_key(&pair, &synthetic).is_ok());
        assert!(require_key(&pair, &release).is_err());
        synthetic.synthetic = false;
        assert!(Trust::from_bytes(&serde_json::to_vec(&synthetic).unwrap()).is_err());
        let mut modified = release;
        modified.key_id = "0".repeat(64);
        assert!(Trust::from_bytes(&serde_json::to_vec(&modified).unwrap()).is_err());
    }

    #[test]
    fn pem_accepts_ed25519_pkcs8_v1_and_v2_and_rejects_other_documents() {
        let v2 = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
        let pair = pem_key(&pem(v2.as_ref())).unwrap();
        assert_eq!(
            pair.public_key().as_ref(),
            Ed25519KeyPair::from_pkcs8(v2.as_ref())
                .unwrap()
                .public_key()
                .as_ref()
        );
        let mut v1 = vec![
            0x30, 0x2e, 0x02, 0x01, 0, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x04, 0x22, 0x04,
            0x20,
        ];
        v1.extend_from_slice(&[7; 32]);
        let pair = pem_key(&pem(&v1)).unwrap();
        assert_eq!(
            pair.public_key().as_ref(),
            Ed25519KeyPair::from_seed_unchecked(&[7; 32])
                .unwrap()
                .public_key()
                .as_ref()
        );
        let crlf = String::from_utf8(pem(&v1)).unwrap().replace('\n', "\r\n");
        assert!(pem_key(crlf.as_bytes()).is_ok());
        let valid = pem(v2.as_ref());
        for invalid in [
            vec![b'x'; 4097],
            vec![0xff],
            pem(b"not a private key"),
            [valid.clone(), valid.clone()].concat(),
            [valid.clone(), b"trailing data".to_vec()].concat(),
            b"-----BEGIN PRIVATE KEY-----\n-----END PRIVATE KEY-----\n".to_vec(),
            b"-----BEGIN PRIVATE KEY-----\n!\n-----END PRIVATE KEY-----\n".to_vec(),
            b"-----BEGIN PRIVATE KEY-----\nAAAA\n".to_vec(),
            b"-----BEGIN RSA PRIVATE KEY-----\nAAAA\n-----END RSA PRIVATE KEY-----\n".to_vec(),
        ] {
            assert!(pem_key(&invalid).is_err());
        }
    }

    #[test]
    fn checksum_signatures_are_bounded_pinned_and_separated_from_update_signatures() {
        let root = tempfile::tempdir().unwrap();
        let input = root.path().join("SHA256SUMS");
        let signature = root.path().join("SHA256SUMS.sig");
        let checksums = format!("{}  synthetic-setup.exe\n", "a".repeat(64));
        fs::write(&input, &checksums).unwrap();
        let pair = Ed25519KeyPair::from_seed_unchecked(&[7; 32]).unwrap();
        let key: [u8; 32] = pair.public_key().as_ref().try_into().unwrap();
        let signed = STANDARD.encode(pair.sign(&checksum_message(&input).unwrap()).as_ref());
        fs::write(&signature, &signed).unwrap();
        verify_checksums(&input, &signature, &key).unwrap();
        assert!(verify_release_checksums(&input, &signature).is_err());
        fs::write(&input, checksums.replace('a', "b")).unwrap();
        assert!(verify_checksums(&input, &signature, &key).is_err());
        fs::write(&input, &checksums).unwrap();
        fs::write(
            &signature,
            STANDARD.encode(pair.sign(checksums.as_bytes()).as_ref()),
        )
        .unwrap();
        assert!(verify_checksums(&input, &signature, &key).is_err());
        fs::write(&signature, format!("{signed}\n")).unwrap();
        assert!(verify_checksums(&input, &signature, &key).is_err());
        for bytes in [Vec::new(), vec![b'x'; 64 * 1024 + 1]] {
            fs::write(&input, bytes).unwrap();
            assert!(checksum_message(&input).is_err());
        }
    }

    #[test]
    fn release_signing_checks_inputs_and_preserves_outputs_before_accessing_the_secret() {
        let root = tempfile::tempdir().unwrap();
        let missing = root.path().join("missing");
        let output = root.path().join("signature");
        fs::write(&output, b"preserve").unwrap();
        assert!(
            sign_release(&missing, package::TARGETS[1], &output)
                .unwrap_err()
                .to_string()
                .contains("existe déjà")
        );
        assert!(
            sign_release_checksums(&missing, &output)
                .unwrap_err()
                .to_string()
                .contains("existe déjà")
        );
        assert_eq!(fs::read(&output).unwrap(), b"preserve");
        let empty = root.path().join("empty");
        let fresh = root.path().join("new-signature");
        fs::write(&empty, []).unwrap();
        assert!(
            sign_release_checksums(&empty, &fresh)
                .unwrap_err()
                .to_string()
                .contains("vide")
        );
        assert!(!fresh.exists());
    }
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
