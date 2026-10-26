use super::{
    checked_file::{CheckedFile, plain_metadata},
    process_tree,
};
use crate::Result;
use atelier_core::{
    application::extensions::{self as protocol, Binary, Manifest, Method, Reply, Request, Status},
    document::Document,
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    io::Write,
    path::{Component, Path, PathBuf, Prefix},
    time::Duration,
};

pub struct Executable {
    binary: Binary,
    file: CheckedFile,
    pinned: bool,
}
pub struct PreparedCall {
    executable: Executable,
    request: Request,
    input: Vec<u8>,
    home: tempfile::TempDir,
    timeout: Duration,
}
struct HashWriter {
    hash: Sha256,
    size: usize,
}
impl Write for HashWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.size = self
            .size
            .checked_add(bytes.len())
            .ok_or_else(|| std::io::Error::other("Executable size overflow"))?;
        if self.size > 128 * 1024 * 1024 {
            return Err(std::io::Error::other("Executable exceeds 128 MiB"));
        }
        self.hash.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn local(path: &Path) -> bool {
    path.is_absolute()
        && matches!(path.components().next(),Some(Component::Prefix(prefix)) if matches!(prefix.kind(),Prefix::Disk(_)|Prefix::VerbatimDisk(_)))
        && path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| {
                extension.eq_ignore_ascii_case("exe") || extension.eq_ignore_ascii_case("com")
            })
}
fn inspect(path: PathBuf, expected: Option<String>) -> Result<Executable> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ;
    if !local(&path)
        || expected.as_ref().is_some_and(|hash| {
            hash.len() != 64
                || !hash
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
    {
        return Err("Exécutable natif local et empreinte de connecteur valides requis.".into());
    }
    let path = path
        .canonicalize()
        .map_err(|_| "Exécutable de connecteur introuvable.")?;
    if !local(&path) {
        return Err("Le connecteur doit être un exécutable natif local.".into());
    }
    let metadata = plain_metadata(&path)?;
    if !metadata.is_file() || metadata.len() > 128 * 1024 * 1024 {
        return Err("Exécutable de connecteur invalide ou supérieur à 128 Mio.".into());
    }
    // Deny writes/deletion through every hard-link while the executable owns credentials.
    let file = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .open(&path)?;
    if file.metadata()?.len() > 128 * 1024 * 1024 {
        return Err("Exécutable de connecteur invalide ou supérieur à 128 Mio.".into());
    }
    let file = CheckedFile::from_file(file)?;
    let mut digest = HashWriter {
        hash: Sha256::new(),
        size: 0,
    };
    file.copy_to(&mut digest)?;
    let hash = format!("{:x}", digest.hash.finalize());
    if expected.as_ref().is_some_and(|expected| expected != &hash) {
        return Err("Le binaire du connecteur a changé. Réinstalle-le et confirme sa nouvelle portée avant de transmettre des secrets.".into());
    }
    file.open_same(&path)?;
    let binary = Binary {
        path: super::task_creation::path_string(&path).utf8()?,
        sha256: hash,
    };
    if binary.path.len() > 32_000 || binary.path.chars().any(char::is_control) {
        return Err("Chemin d’exécutable de connecteur invalide ou trop long.".into());
    }
    Ok(Executable {
        binary,
        file,
        pinned: expected.is_some(),
    })
}

fn contains_secret(value: &Value, secrets: &std::collections::BTreeMap<String, String>) -> bool {
    match value {
        Value::String(value) => secrets
            .values()
            .any(|secret| !secret.is_empty() && value.contains(secret)),
        Value::Array(values) => values.iter().any(|value| contains_secret(value, secrets)),
        Value::Object(values) => values.iter().any(|(key, value)| {
            secrets
                .values()
                .any(|secret| !secret.is_empty() && key.contains(secret))
                || contains_secret(value, secrets)
        }),
        _ => false,
    }
}
pub(super) fn values(manifest: &Manifest, request: &Request) -> Result<()> {
    protocol::validate_manifest(manifest)?;
    protocol::effect(manifest, request)?;
    for field in manifest
        .fields
        .iter()
        .filter(|field| field.kind == protocol::FieldKind::Url)
    {
        if let Some(value) = request
            .settings
            .get(&field.id)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
        {
            let url =
                reqwest::Url::parse(value).map_err(|_| "Adresse de connecteur HTTPS invalide.")?;
            if value.chars().any(char::is_control)
                || url.scheme() != "https"
                || url.host_str().is_none()
                || !url.username().is_empty()
                || url.password().is_some()
                || url.query().is_some()
                || url.fragment().is_some()
            {
                return Err(
                    "Adresse de connecteur HTTPS requise, sans identifiants ni paramètres.".into(),
                );
            }
        }
    }
    Ok(())
}
impl Executable {
    pub async fn open(path: PathBuf, expected: Option<String>) -> Result<Self> {
        tokio::task::spawn_blocking(move || inspect(path, expected)).await?
    }
    pub fn binary(&self) -> Binary {
        self.binary.clone()
    }
    pub fn prepare(
        self,
        manifest: &Manifest,
        request: Request,
        timeout: Duration,
    ) -> Result<PreparedCall> {
        if !self.pinned || request.method == Method::Describe {
            return Err("Installe le connecteur et vérifie son empreinte avant cet appel.".into());
        }
        values(manifest, &request)?;
        self.prepare_validated(request, timeout)
    }
    fn prepare_validated(self, request: Request, timeout: Duration) -> Result<PreparedCall> {
        if !(Duration::from_millis(100)..=Duration::from_secs(180)).contains(&timeout) {
            return Err("Délai de connecteur invalide.".into());
        }
        let input = serde_json::to_vec(&request)?;
        if input.len() > protocol::INPUT_LIMIT {
            return Err("Requête de connecteur trop volumineuse.".into());
        }
        let home = tempfile::tempdir()?;
        for name in ["home", "appdata", "local", "temp"] {
            std::fs::create_dir(home.path().join(name))?;
        }
        Ok(PreparedCall {
            executable: self,
            request,
            input,
            home,
            timeout,
        })
    }
    pub async fn describe(self, module: &str) -> Result<(Binary, Manifest)> {
        if !protocol::identifier(module) {
            return Err("Identifiant de connecteur invalide.".into());
        }
        let binary = self.binary();
        let request = Request {
            protocol: protocol::PROTOCOL.into(),
            request_id: format!("request-{}", uuid::Uuid::new_v4()),
            module: module.into(),
            method: Method::Describe,
            operation_id: None,
            action: None,
            settings: Default::default(),
            secrets: Default::default(),
            input: Value::Null,
        };
        let reply = self
            .prepare_validated(request, Duration::from_secs(15))?
            .dispatch()
            .await?;
        if reply.status != Status::Ok {
            return Err("Le connecteur n’a pas fourni son manifeste.".into());
        }
        let manifest: Manifest = serde_json::from_value(reply.output)
            .map_err(|_| "Manifeste de connecteur invalide.")?;
        protocol::validate_manifest(&manifest)?;
        if manifest.id != module {
            return Err("Le manifeste appartient à un autre connecteur.".into());
        }
        Ok((binary, manifest))
    }
}

fn isolate(command: &mut tokio::process::Command, home: &Path) {
    let locales = command
        .as_std()
        .get_envs()
        .filter_map(|(key, _)| {
            key.to_str()
                .filter(|key| key.starts_with("LC_"))
                .map(str::to_owned)
        })
        .collect::<Vec<_>>();
    for key in locales {
        command.env_remove(key);
    }
    for key in ["LANG", "TERM", "COLORTERM"] {
        command.env_remove(key);
    }
    command
        .current_dir(home)
        .env("HOME", home.join("home"))
        .env("USERPROFILE", home.join("home"))
        .env("APPDATA", home.join("appdata"))
        .env("LOCALAPPDATA", home.join("local"))
        .env("TEMP", home.join("temp"))
        .env("TMP", home.join("temp"))
        .env("TMPDIR", home.join("temp"));
}

impl PreparedCall {
    /// The durable caller marks dispatch before consuming this single-use call.
    pub async fn dispatch(self) -> Result<Reply> {
        let Self {
            executable,
            request,
            input,
            home,
            timeout,
        } = self;
        executable
            .file
            .open_same(Path::new(&executable.binary.path))?;
        let profile: Document = serde_json::from_str(
            &serde_json::json!({"type":"command","command":executable.binary.path}).to_string(),
        )?;
        let mut command = super::agent_diagnostics::command(&profile).await?;
        isolate(&mut command, home.path());
        if request.module == "azure-tickets" {
            command.arg("connector-azure");
        }
        let output=process_tree::capture(&mut command,&input,timeout,protocol::OUTPUT_LIMIT).await.map_err(|_|"Appel de connecteur interrompu, trop long ou trop volumineux. Vérifie son état avant de réessayer.")?;
        if !output.status.success() {
            return Err(
                "Le connecteur n’a pas terminé correctement. Vérifie son installation.".into(),
            );
        }
        let reply = protocol::decode_reply(&output.stdout, &request)?;
        if contains_secret(&serde_json::to_value(&reply)?, &request.secrets) {
            return Err(
                "La réponse du connecteur contient des données confidentielles inattendues.".into(),
            );
        }
        executable
            .file
            .open_same(Path::new(&executable.binary.path))?;
        home.close()
            .map_err(|_| "Le dossier temporaire du connecteur n’a pas pu être nettoyé.")?;
        Ok(reply)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn connector_homes_and_locale_are_private_without_mutating_the_parent_environment() {
        let mut command = tokio::process::Command::new("synthetic-never-executed.exe");
        command
            .env_clear()
            .env("LC_SYNTHETIC_SECRET", "synthetic-private-value")
            .env("LANG", "synthetic-locale")
            .env("HOME", "C:/synthetic/personal");
        let home = Path::new("C:/synthetic/private-extension");
        isolate(&mut command, home);
        let environment = command
            .as_std()
            .get_envs()
            .filter_map(|(key, value)| value.map(|value| (key.to_owned(), value.to_owned())))
            .collect::<std::collections::BTreeMap<_, _>>();
        assert!(
            !environment
                .keys()
                .any(|key| key.to_string_lossy().starts_with("LC_"))
        );
        assert!(!environment.contains_key(std::ffi::OsStr::new("LANG")));
        for (key, directory) in [
            ("HOME", "home"),
            ("USERPROFILE", "home"),
            ("APPDATA", "appdata"),
            ("TEMP", "temp"),
        ] {
            assert_eq!(
                environment.get(std::ffi::OsStr::new(key)),
                Some(&home.join(directory).into_os_string())
            );
        }
        let mut hash = HashWriter {
            hash: Sha256::new(),
            size: 128 * 1024 * 1024 - 1,
        };
        assert!(hash.write_all(b"xx").is_err());
    }
}
