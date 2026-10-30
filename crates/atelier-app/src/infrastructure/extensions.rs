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
    path::{Path, PathBuf},
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
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};
        path.is_absolute()
            && matches!(path.components().next(),Some(Component::Prefix(prefix)) if matches!(prefix.kind(),Prefix::Disk(_)|Prefix::VerbatimDisk(_)))
            && path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| {
                    extension.eq_ignore_ascii_case("exe") || extension.eq_ignore_ascii_case("com")
                })
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.is_absolute()
            && path.metadata().is_ok_and(|metadata| {
                metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
            })
    }
}
fn inspect(path: PathBuf, expected: Option<String>) -> Result<Executable> {
    if !local(&path)
        || expected.as_ref().is_some_and(|hash| {
            hash.len() != 64
                || !hash
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
    {
        return Err("message.connector_binary_and_hash_required".into());
    }
    let path = path
        .canonicalize()
        .map_err(|_| "message.connector_executable_missing")?;
    if !local(&path) {
        return Err("message.connector_native_executable_required".into());
    }
    let metadata = plain_metadata(&path)?;
    if !metadata.is_file() || metadata.len() > 128 * 1024 * 1024 {
        return Err("message.connector_executable_invalid".into());
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // Windows refuses writes/deletion through all aliases while the connector owns credentials.
        options.share_mode(windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options.open(&path)?;
    if file.metadata()?.len() > 128 * 1024 * 1024 {
        return Err("message.connector_executable_invalid".into());
    }
    let file = CheckedFile::from_file(file)?;
    let mut digest = HashWriter {
        hash: Sha256::new(),
        size: 0,
    };
    file.copy_to(&mut digest)?;
    let hash = crate::infrastructure::session::hexadecimal(&digest.hash.finalize());
    if expected.as_ref().is_some_and(|expected| expected != &hash) {
        return Err("message.connector_binary_changed_reinstall".into());
    }
    file.open_same(&path)?;
    let binary = Binary {
        path: super::task_creation::path_string(&path),
        sha256: hash,
    };
    if binary.path.len() > 32_000 || binary.path.chars().any(char::is_control) {
        return Err("message.connector_executable_path_invalid".into());
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
            let url = reqwest::Url::parse(value)
                .map_err(|_| "message.connector_https_address_invalid")?;
            if value.chars().any(char::is_control)
                || url.scheme() != "https"
                || url.host_str().is_none()
                || !url.username().is_empty()
                || url.password().is_some()
                || url.query().is_some()
                || url.fragment().is_some()
            {
                return Err("message.connector_https_address_required".into());
            }
        }
    }
    Ok(())
}
impl Executable {
    #[cfg(unix)]
    fn snapshot(&self, directory: &Path) -> Result<PathBuf> {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        struct Copy {
            output: std::fs::File,
            hash: HashWriter,
        }
        impl Write for Copy {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.hash.write_all(bytes)?;
                self.output.write_all(bytes)?;
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                self.output.flush()
            }
        }
        let path = directory.join("connector-image");
        let mut copy = Copy {
            output: std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW)
                .open(&path)?,
            hash: HashWriter {
                hash: Sha256::new(),
                size: 0,
            },
        };
        self.file
            .open_same(Path::new(&self.binary.path))?
            .copy_to(&mut copy)?;
        copy.output.sync_all()?;
        if crate::infrastructure::session::hexadecimal(&copy.hash.hash.finalize())
            != self.binary.sha256
        {
            return Err("message.connector_binary_changed_before_copy".into());
        }
        copy.output
            .set_permissions(std::fs::Permissions::from_mode(0o500))?;
        // POSIX permits renaming/writing an opened source; execute only the verified private snapshot.
        Ok(path)
    }
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
            return Err("message.connector_installation_required".into());
        }
        values(manifest, &request)?;
        self.prepare_validated(request, timeout)
    }
    fn prepare_validated(self, request: Request, timeout: Duration) -> Result<PreparedCall> {
        if !(Duration::from_millis(100)..=Duration::from_secs(180)).contains(&timeout) {
            return Err("message.connector_timeout_invalid".into());
        }
        let input = serde_json::to_vec(&request)?;
        if input.len() > protocol::INPUT_LIMIT {
            return Err("message.connector_request_too_large".into());
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
            return Err("message.connector_identifier_invalid".into());
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
            return Err("message.connector_manifest_missing".into());
        }
        let manifest: Manifest = serde_json::from_value(reply.output)
            .map_err(|_| "message.connector_manifest_invalid")?;
        protocol::validate_manifest(&manifest)?;
        if manifest.id != module {
            return Err("message.connector_manifest_wrong_owner".into());
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
        #[cfg(windows)]
        let binary = executable.binary.path.clone();
        #[cfg(unix)]
        let (executable, binary) = {
            let folder = home.path().to_owned();
            tokio::task::spawn_blocking(move || {
                let binary = executable.snapshot(&folder)?;
                Ok::<_, crate::Error>((executable, binary))
            })
            .await??
        };
        let profile: Document = serde_json::from_str(
            &serde_json::json!({"type":"command","command":binary}).to_string(),
        )?;
        let mut command = super::agent_diagnostics::command(&profile).await?;
        isolate(&mut command, home.path());
        if request.module == "azure-tickets" {
            command.arg("connector-azure");
        }
        let output = process_tree::capture(&mut command, &input, timeout, protocol::OUTPUT_LIMIT)
            .await
            .map_err(|_| "message.connector_call_interrupted")?;
        if !output.status.success() {
            return Err("message.connector_execution_failed".into());
        }
        let reply = protocol::decode_reply(&output.stdout, &request)?;
        if contains_secret(&serde_json::to_value(&reply)?, &request.secrets) {
            return Err("message.connector_response_contains_secrets".into());
        }
        executable
            .file
            .open_same(Path::new(&executable.binary.path))?;
        home.close()
            .map_err(|_| "message.connector_temporary_cleanup_failed")?;
        Ok(reply)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    #[test]
    fn posix_snapshots_execute_only_the_approved_bytes_and_refuse_a_source_changed_after_probe() {
        use std::os::unix::fs::PermissionsExt;
        let source = tempfile::tempdir().unwrap();
        let binary = source.path().join("synthetic-native");
        std::fs::write(&binary, b"synthetic image, never executed").unwrap();
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
        let executable = inspect(binary.clone(), None).unwrap();
        let home = tempfile::tempdir().unwrap();
        let copy = executable.snapshot(home.path()).unwrap();
        assert_eq!(
            std::fs::read(&copy).unwrap(),
            b"synthetic image, never executed"
        );
        assert_eq!(
            std::fs::metadata(&copy).unwrap().permissions().mode() & 0o777,
            0o500
        );
        std::fs::write(&binary, b"human replacement preserved").unwrap();
        assert_eq!(
            std::fs::read(copy).unwrap(),
            b"synthetic image, never executed"
        );
        let another = tempfile::tempdir().unwrap();
        assert!(executable.snapshot(another.path()).is_err());
        assert_eq!(
            std::fs::read(binary).unwrap(),
            b"human replacement preserved"
        );
    }
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
