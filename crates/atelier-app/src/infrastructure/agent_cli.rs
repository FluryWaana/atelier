use crate::Result;
use atelier_core::{
    application::agent_cli::{Availability, UpdateInfo},
    document::Document,
};
use reqwest::Url;
use std::{collections::BTreeMap, ffi::OsString, path::Path, time::Duration};
pub struct Catalog {
    base: Url,
    client: reqwest::Client,
}
impl Catalog {
    fn prepare(base: Url, synthetic: bool) -> Result<Self> {
        if !base.username().is_empty()
            || base.password().is_some()
            || base.fragment().is_some()
            || base.query().is_some()
            || base.path() != "/"
            || if synthetic {
                base.scheme() != "http"
                    || !base.host_str().is_some_and(|host| {
                        host.parse::<std::net::IpAddr>()
                            .is_ok_and(|ip| ip.is_loopback())
                    })
            } else {
                base.scheme() != "https" || base.host_str() != Some("registry.npmjs.org")
            }
        {
            return Err("message.cli_release_origin_invalid".into());
        }
        Ok(Self {
            base,
            client: reqwest::Client::builder()
                .https_only(!synthetic)
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(3))
                .timeout(Duration::from_secs(5))
                .build()?,
        })
    }
    pub fn production() -> Result<Self> {
        Self::prepare(Url::parse("https://registry.npmjs.org/")?, false)
    }
    pub fn synthetic(base: &str) -> Result<Self> {
        Self::prepare(Url::parse(base)?, true)
    }
    pub async fn latest(&self, kind: &str) -> Result<String> {
        let package = atelier_core::domain::agent_catalog::find(kind)
            .and_then(|agent| agent.registry_package)
            .ok_or("message.cli_release_registry_unsupported")?;
        let mut url = self.base.clone();
        url.path_segments_mut()
            .map_err(|_| "message.cli_release_origin_invalid")?
            .clear()
            .push(package)
            .push("latest");
        let mut response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|_| "message.cli_release_read_unavailable")?;
        if response.status().as_u16() != 200 {
            return Err("message.cli_published_version_unavailable".into());
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| "message.cli_release_read_interrupted")?
        {
            if bytes.len().saturating_add(chunk.len()) > 65_536 {
                return Err("message.cli_version_metadata_too_large".into());
            }
            bytes.extend_from_slice(&chunk);
        }
        let value: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|_| "message.cli_version_metadata_invalid")?;
        if value["name"] != package {
            return Err("message.cli_release_wrong_product".into());
        }
        let version = value["version"]
            .as_str()
            .ok_or("message.cli_version_missing")?;
        super::package::version_parts(version)?;
        Ok(version.into())
    }
    pub async fn prepare_selected(
        &self,
        profile: &Document,
        info: &UpdateInfo,
        binary: std::path::PathBuf,
        environment: BTreeMap<String, OsString>,
    ) -> Result<(
        tokio::process::Command,
        super::checked_file::CheckedFile,
        std::fs::File,
    )> {
        prepare_selected(profile, info, binary, environment, self).await
    }
}
fn package(kind: &str) -> Result<&'static str> {
    atelier_core::domain::agent_catalog::find(kind)
        .and_then(|agent| {
            agent
                .registry_package
                .or_else(|| agent.manual_update.map(|update| update.package))
        })
        .ok_or_else(|| "message.cli_native_update_unsupported".into())
}
fn managed_root(
    binary: &Path,
    kind: &str,
    environment: &BTreeMap<String, OsString>,
) -> Result<Option<String>> {
    let Some(home) = environment
        .get("USERPROFILE")
        .or_else(|| environment.get("HOME"))
    else {
        return Ok(None);
    };
    let home = Path::new(home);
    if !home.is_absolute() {
        return Ok(None);
    }
    let binary = super::task_creation::path_string(binary)
        .utf8()?
        .replace('\\', "/")
        .to_lowercase();
    if let Some(update) =
        atelier_core::domain::agent_catalog::find(kind).and_then(|agent| agent.manual_update)
    {
        let root = home.join(update.root);
        let bin = super::task_creation::path_string(&root.join("bin"))
            .utf8()?
            .replace('\\', "/");
        if binary.starts_with(&(bin.to_lowercase() + "/")) {
            return Ok(Some(
                super::task_creation::path_string(&root)
                    .utf8()?
                    .replace('\\', "/"),
            ));
        }
        return Ok(None);
    }
    for root in [
        home.join(".local/bin"),
        home.join(".local/share").join(kind),
    ] {
        let root = super::task_creation::path_string(&root)
            .utf8()?
            .replace('\\', "/");
        if binary.starts_with(&(root.to_lowercase() + "/")) {
            return Ok(Some(
                super::task_creation::path_string(&home.join(".local"))
                    .utf8()?
                    .replace('\\', "/"),
            ));
        }
    }
    Ok(None)
}

fn manual_root_matches(
    profile: &Document,
    kind: &str,
    inherited: &BTreeMap<String, OsString>,
    root: &str,
) -> Result<bool> {
    let key = atelier_core::domain::agent_catalog::find(kind)
        .and_then(|agent| agent.manual_update)
        .and_then(|update| update.root_environment);
    let Some(key) = key else { return Ok(true) };
    let environment = super::agent_diagnostics::environment(profile, inherited, false)?;
    let Some(value) = environment.get(key) else {
        return Ok(true);
    };
    let Some(value) = value.to_str() else {
        return Ok(false);
    };
    let normalize = |path: &str| {
        path.replace('\\', "/")
            .trim_end_matches('/')
            .to_ascii_lowercase()
    };
    Ok(normalize(value) == normalize(root))
}
pub(super) async fn describe(
    profile: &Document,
    binary: &Path,
    version: Option<&str>,
    environment: &BTreeMap<String, OsString>,
) -> Result<UpdateInfo> {
    let kind = profile.get::<String>("type")?.unwrap_or_default();
    let mut info = UpdateInfo {
        package: package(&kind).unwrap_or_default().into(),
        latest_version: None,
        managed_root: None,
        observed_at: time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)?,
        availability: Availability::Unknown,
        installed_version: version.map(str::to_owned),
        image: None,
    };
    if binary
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("cmd") || extension.eq_ignore_ascii_case("bat")
        })
    {
        info.availability = Availability::Managed;
        return Ok(info);
    }
    if kind == "command"
        || !profile
            .get::<String>("command")?
            .unwrap_or_default()
            .is_empty()
    {
        info.availability = Availability::Custom;
        return Ok(info);
    }
    if info.package.is_empty() {
        info.availability = Availability::Managed;
        return Ok(info);
    }
    info.managed_root = managed_root(binary, &kind, environment)?;
    if info.managed_root.is_none() {
        info.availability = Availability::Managed;
        return Ok(info);
    }
    if !manual_root_matches(
        profile,
        &kind,
        environment,
        info.managed_root.as_deref().unwrap_or_default(),
    )? {
        info.managed_root = None;
        info.availability = Availability::Managed;
        return Ok(info);
    }
    let Some(version) = version.and_then(|version| super::package::version_parts(version).ok())
    else {
        return Ok(info);
    };
    if atelier_core::domain::agent_catalog::find(&kind)
        .is_some_and(|agent| agent.manual_update.is_some())
    {
        if let Ok((image, _)) = pin(binary.to_owned()).await {
            info.image = Some(image);
            info.availability = Availability::Manual;
        }
        return Ok(info);
    }
    if let Ok(latest) = Catalog::production()?.latest(&kind).await {
        let target = super::package::version_parts(&latest)?;
        info.availability = if target > version {
            Availability::Available
        } else {
            Availability::Current
        };
        info.latest_version = Some(latest);
    }
    if info.availability == Availability::Available {
        match pin(binary.to_owned()).await {
            Ok((image, _)) => info.image = Some(image),
            Err(_) => info.availability = Availability::Unknown,
        }
    }
    Ok(info)
}

pub(super) async fn pin(
    path: std::path::PathBuf,
) -> Result<(
    atelier_core::application::extensions::Binary,
    super::checked_file::CheckedFile,
)> {
    tokio::task::spawn_blocking(move || {
        use sha2::{Digest, Sha256};
        use std::os::windows::fs::OpenOptionsExt;
        struct Hash {
            digest: Sha256,
            size: usize,
        }
        impl std::io::Write for Hash {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                if bytes.len() > 512 * 1024 * 1024 - self.size {
                    return Err(std::io::Error::other("message.cli_binary_too_large"));
                }
                self.size += bytes.len();
                self.digest.update(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let file = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ)
            .custom_flags(windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT)
            .open(&path)?;
        let checked = super::checked_file::CheckedFile::from_file(file)?.single_link()?;
        let mut digest = Hash {
            digest: Sha256::new(),
            size: 0,
        };
        checked.copy_to(&mut digest)?;
        checked.open_same(&path)?;
        Ok((
            atelier_core::application::extensions::Binary {
                path: super::task_creation::path_string(&path).utf8()?,
                sha256: crate::infrastructure::session::hexadecimal(&digest.digest.finalize()),
            },
            checked,
        ))
    })
    .await?
}
pub(crate) fn read(
    store: &super::store::Store<'_>,
) -> Result<Option<atelier_core::application::agent_cli::Operation>> {
    Ok(atelier_core::application::agent_cli::operation(
        &store.settings::<Document>()?,
    )?)
}
fn save(
    store: &super::store::Store<'_>,
    operation: &atelier_core::application::agent_cli::Operation,
) -> Result<()> {
    if serde_json::to_vec(operation)?.len() > 65_536 {
        return Err("message.cli_update_log_too_large".into());
    }
    let mut settings = store.settings::<Document>()?;
    settings.set("agentCliUpdate", operation)?;
    store.save_settings(&settings)
}
pub(crate) fn reserve(
    store: &super::store::Store<'_>,
    provider: &str,
    info: UpdateInfo,
) -> Result<atelier_core::application::agent_cli::Operation> {
    use super::store::Documents;
    use atelier_core::application::agent_cli::{Operation, Phase};
    if !match info.availability {
        Availability::Available => {
            atelier_core::domain::agent_catalog::update_package_supported(&info.package, false)
        }
        Availability::Manual => {
            atelier_core::domain::agent_catalog::update_package_supported(&info.package, true)
        }
        _ => false,
    } {
        return Err("message.cli_update_preview_invalid".into());
    }
    let image = info
        .image
        .as_ref()
        .ok_or("message.cli_fingerprint_missing")?;
    if image.sha256.len() != 64
        || !image
            .sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        || image.path.len() > 32_000
        || image.path.chars().any(char::is_control)
        || !Path::new(&image.path).is_absolute()
    {
        return Err("message.cli_fingerprint_invalid".into());
    }
    let root = info
        .managed_root
        .as_deref()
        .ok_or("message.cli_native_installation_missing")?;
    if root.len() > 32_000 || root.chars().any(char::is_control) || !Path::new(root).is_absolute() {
        return Err("message.cli_native_installation_invalid".into());
    }
    let from = super::package::version_parts(
        info.installed_version
            .as_deref()
            .ok_or("message.installed_version_missing")?,
    )?;
    if info.availability == Availability::Manual {
        if info.latest_version.is_some() {
            return Err("message.cli_update_preview_invalid".into());
        }
    } else {
        let target = super::package::version_parts(
            info.latest_version
                .as_deref()
                .ok_or("message.target_version_missing")?,
        )?;
        if target <= from {
            return Err("message.no_confirmed_newer_update".into());
        }
    }
    let age = time::OffsetDateTime::now_utc()
        - time::OffsetDateTime::parse(
            &info.observed_at,
            &time::format_description::well_known::Rfc3339,
        )?;
    if age > time::Duration::minutes(10) || age < time::Duration::seconds(-60) {
        return Err("message.reload_cli_diagnostic_before_update".into());
    }
    store.transaction(|store| {
        let settings = store.settings::<Document>()?;
        if settings.raw("jiraImportReservation").is_some()
            || super::connector_effects::read(&settings)?
                .iter()
                .any(super::connector_effects::pending)
        {
            return Err("message.complete_remote_operations_before_cli_update".into());
        }
        if atelier_core::application::agent_cli::busy(&settings)? {
            return Err("message.cli_update_already_running".into());
        }
        for run in store.documents::<Document>(Documents::Runs)? {
            if super::runs::active(&run)? {
                return Err("message.stop_agents_before_cli_update".into());
            }
        }
        for card in store.documents::<Document>(Documents::Cards)? {
            if atelier_core::application::forge::unresolved(&card)?
                || atelier_core::application::jira::unresolved(&card)?
                || card
                    .get::<Document>("automation")?
                    .is_some_and(|flow| flow.string_equals("state", "running").unwrap_or(true))
            {
                return Err("message.complete_task_operations_before_cli_update".into());
            }
        }
        let queue = super::queue::read(store)?;
        if queue.active.is_some() || !queue.paused && !queue.ids.is_empty() {
            return Err("message.pause_queue_before_cli_update".into());
        }
        for entry in store.documents::<Document>(Documents::Times)? {
            if entry.truthy("runningSince")?
                || entry.string_equals("exportState", "sending")?
                || entry.string_equals("exportState", "uncertain")?
            {
                return Err("message.stop_timers_before_cli_update".into());
            }
        }
        if let Some(previous) = read(store)?
            && previous.phase == Phase::Uncertain
            && time::OffsetDateTime::parse(
                &info.observed_at,
                &time::format_description::well_known::Rfc3339,
            )? <= time::OffsetDateTime::parse(
                &previous.updated_at,
                &time::format_description::well_known::Rfc3339,
            )?
        {
            return Err("message.reread_diagnostic_after_interruption".into());
        }
        let now = time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)?;
        let operation = Operation {
            id: format!("cli-{}", uuid::Uuid::new_v4()),
            provider: provider.into(),
            info,
            phase: Phase::Preparing,
            started_at: now.clone(),
            updated_at: now,
            actual_version: None,
            target_reached: None,
            message: "message.cli_update_prepared_command_not_started".into(),
        };
        save(store, &operation)?;
        Ok(operation)
    })
}
pub(crate) fn change(
    store: &super::store::Store<'_>,
    id: &str,
    phase: atelier_core::application::agent_cli::Phase,
    actual: Option<String>,
    message: &str,
) -> Result<atelier_core::application::agent_cli::Operation> {
    use atelier_core::application::agent_cli::Phase;
    store.transaction(|store| {
        let mut operation = read(store)?
            .filter(|operation| operation.id == id)
            .ok_or("message.cli_update_changed")?;
        let allowed = matches!(
            (operation.phase, phase),
            (Phase::Preparing, Phase::Running | Phase::Failed)
                | (Phase::Running, Phase::Observed | Phase::Uncertain)
                | (Phase::Uncertain, Phase::Observed)
        );
        if !allowed {
            return Err("message.cli_update_transition_invalid".into());
        }
        if (phase == Phase::Observed) != actual.is_some() {
            return Err("message.cli_observation_requires_version".into());
        }
        if let Some(version) = &actual {
            let actual = super::package::version_parts(version)?;
            if let Some(expected) = operation.info.latest_version.as_deref() {
                operation.target_reached = Some(actual >= super::package::version_parts(expected)?);
            }
        }
        operation.actual_version = actual;
        operation.phase = phase;
        operation.updated_at = time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)?;
        operation.message = message.into();
        save(store, &operation)?;
        Ok(operation)
    })
}
pub(crate) async fn prepare(
    profile: &Document,
    info: &UpdateInfo,
) -> Result<(
    tokio::process::Command,
    super::checked_file::CheckedFile,
    std::fs::File,
)> {
    let (binary, environment) = super::agent_diagnostics::resolve(profile).await?;
    Catalog::production()?
        .prepare_selected(profile, info, binary, environment)
        .await
}

async fn prepare_selected(
    profile: &Document,
    info: &UpdateInfo,
    binary: std::path::PathBuf,
    inherited: BTreeMap<String, OsString>,
    catalog: &Catalog,
) -> Result<(
    tokio::process::Command,
    super::checked_file::CheckedFile,
    std::fs::File,
)> {
    use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt, OpenOptionsSyncExt};
    use cap_std::fs::OpenOptionsExt;
    let date = time::OffsetDateTime::parse(
        &info.observed_at,
        &time::format_description::well_known::Rfc3339,
    )?;
    let age = time::OffsetDateTime::now_utc() - date;
    if !matches!(
        info.availability,
        Availability::Available | Availability::Manual
    ) || age > time::Duration::minutes(10)
        || age < time::Duration::seconds(-60)
        || !profile
            .get::<String>("command")?
            .unwrap_or_default()
            .is_empty()
    {
        return Err("message.reload_cli_diagnostic_before_update".into());
    }
    let kind = profile
        .get::<String>("type")?
        .ok_or("message.agent_type_missing")?;
    let root = managed_root(&binary, &kind, &inherited)?
        .filter(|root| Some(root) == info.managed_root.as_ref())
        .ok_or("message.cli_update_uses_manager")?;
    if !manual_root_matches(profile, &kind, &inherited, &root)? {
        return Err("message.the_cli_update_directory_does_not_match_the_installation".into());
    }
    if package(&kind)? != info.package {
        return Err("message.preview_cli_changed".into());
    }
    let manual =
        atelier_core::domain::agent_catalog::find(&kind).and_then(|agent| agent.manual_update);
    if (info.availability == Availability::Manual) != manual.is_some() {
        return Err("message.cli_update_mode_mismatch".into());
    }
    let directory = super::repository_access::absolute(Path::new(&root))?;
    let mut options = cap_std::fs::OpenOptions::new();
    options
        .read(true)
        .write(true)
        .create(true)
        .follow(FollowSymlinks::No)
        .nonblock(true)
        .share_mode(
            windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ
                | windows_sys::Win32::Storage::FileSystem::FILE_SHARE_WRITE,
        );
    let lock = directory
        .open_with(".atelier-cli-update.lock", &options)?
        .into_std();
    if !lock.metadata()?.is_file() || lock.metadata()?.len() != 0 {
        return Err("message.cli_lock_unexpected".into());
    }
    super::checked_file::CheckedFile::from_file(lock.try_clone()?)?.single_link()?;
    lock.try_lock().map_err(|_| "message.cli_update_locked")?;
    let (image, pin) = pin(binary.clone()).await?;
    if Some(&image) != info.image.as_ref() {
        return Err("message.cli_program_changed".into());
    }
    let mut environment = super::agent_diagnostics::environment(profile, &inherited, false)?;
    if let Some(key) = manual.and_then(|update| update.root_environment) {
        environment.insert(key.into(), root.clone().into());
    }
    let output =
        super::agent_diagnostics::capture(&binary, &["--version".into()], &environment).await?;
    if !output.status.success()
        || super::agent_diagnostics::version(&output.stdout) != info.installed_version
    {
        return Err("message.installed_version_changed".into());
    }
    if info.availability != Availability::Manual
        && catalog.latest(&kind).await?.as_str()
            != info
                .latest_version
                .as_deref()
                .ok_or("message.target_version_missing")?
    {
        return Err("message.published_version_changed".into());
    }
    let installed = super::package::version_parts(
        info.installed_version
            .as_deref()
            .ok_or("message.installed_version_missing")?,
    )?;
    if info.availability == Availability::Manual {
        if info.latest_version.is_some() {
            return Err("message.cli_update_preview_invalid".into());
        }
    } else {
        let target = super::package::version_parts(
            info.latest_version
                .as_deref()
                .ok_or("message.target_version_missing")?,
        )?;
        if target <= installed {
            return Err("message.no_confirmed_newer_update".into());
        }
    }
    let mut command = tokio::process::Command::new(&binary);
    command
        .arg(manual.map_or("update", |update| update.command))
        .current_dir(binary.parent().ok_or("message.cli_directory_missing")?)
        .env_clear()
        .envs(environment);
    Ok((command, pin, lock))
}

pub(crate) async fn observed_version(profile: &Document, info: &UpdateInfo) -> Result<String> {
    let (binary, environment) = super::agent_diagnostics::resolve(profile).await?;
    let kind = profile
        .get::<String>("type")?
        .ok_or("message.agent_type_missing")?;
    if managed_root(&binary, &kind, &environment)? != info.managed_root {
        return Err("message.cli_install_scope_changed".into());
    }
    let output = super::agent_diagnostics::capture(
        &binary,
        &["--version".into()],
        &super::agent_diagnostics::environment(profile, &environment, false)?,
    )
    .await?;
    if !output.status.success() {
        return Err("message.cli_version_reread_failed".into());
    }
    super::agent_diagnostics::version(&output.stdout)
        .filter(|version| super::package::version_parts(version).is_ok())
        .ok_or_else(|| "message.cli_version_unrecognized".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use atelier_core::application::agent_cli::Phase;

    fn info() -> UpdateInfo {
        UpdateInfo {
            package: "@openai/codex".into(),
            latest_version: Some("2.0.0".into()),
            installed_version: Some("1.0.0".into()),
            managed_root: Some(r"C:\synthetic\home\.local".into()),
            observed_at: time::OffsetDateTime::now_utc()
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap(),
            availability: Availability::Available,
            image: Some(atelier_core::application::extensions::Binary {
                path: r"C:\synthetic\home\.local\bin\codex.exe".into(),
                sha256: "a".repeat(64),
            }),
        }
    }

    #[tokio::test]
    async fn invalid_previews_do_not_poison_the_journal_and_uncertain_is_never_replayed() {
        let database = super::super::database::Database::open(Path::new(":memory:")).unwrap();
        database
            .call(|store| {
                let mut invalid = info();
                invalid.image.as_mut().unwrap().sha256 = "invalid".into();
                assert!(reserve(store, "synthetic", invalid).is_err());
                assert!(read(store)?.is_none());
                let operation = reserve(store, "synthetic", info())?;
                assert!(reserve(store, "other", info()).is_err());
                assert!(
                    change(
                        store,
                        &operation.id,
                        Phase::Observed,
                        Some("2.0.0".into()),
                        "invalid shortcut"
                    )
                    .is_err()
                );
                change(
                    store,
                    &operation.id,
                    Phase::Running,
                    None,
                    "synthetic sending",
                )?;
                assert!(
                    change(
                        store,
                        &operation.id,
                        Phase::Observed,
                        None,
                        "missing observation"
                    )
                    .is_err()
                );
                change(
                    store,
                    &operation.id,
                    Phase::Uncertain,
                    None,
                    "synthetic interruption",
                )?;
                assert!(reserve(store, "synthetic", operation.info).is_err());
                assert!(
                    change(store, &operation.id, Phase::Running, None, "invalid replay").is_err()
                );
                let observed = change(
                    store,
                    &operation.id,
                    Phase::Observed,
                    Some("1.0.0".into()),
                    "synthetic observation",
                )?;
                assert_eq!(observed.target_reached, Some(false));
                Ok(())
            })
            .await
            .unwrap();
    }

    #[test]
    fn catalog_rejects_external_or_credentialed_synthetic_origins() {
        for origin in [
            "https://127.0.0.1/",
            "http://example.invalid/",
            "http://user:token@127.0.0.1/",
            "http://127.0.0.1/path",
            "http://127.0.0.1/?token=secret",
        ] {
            assert!(Catalog::synthetic(origin).is_err());
        }
        assert!(Catalog::synthetic("http://127.0.0.1:12345/").is_ok());
    }

    #[tokio::test]
    async fn pin_refuses_modified_paths_and_preserves_the_approved_image() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("synthetic.exe");
        std::fs::write(&path, b"synthetic image, never executable").unwrap();
        let (_, pinned) = pin(path.clone()).await.unwrap();
        assert!(std::fs::write(&path, b"replacement").is_err());
        assert!(std::fs::remove_file(&path).is_err());
        pinned.open_same(&path).unwrap();
        drop(pinned);
        assert_eq!(
            std::fs::read(path).unwrap(),
            b"synthetic image, never executable"
        );
    }
}
