use super::{process_tree, task_creation::path_string};
use crate::Result;
use atelier_core::{
    contracts::{AgentAuthentication, AgentDiagnostic},
    document::Document,
};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    path::{Path, PathBuf},
    time::Duration,
};

pub(super) type Environment = BTreeMap<String, OsString>;
const EXECUTABLE_NOT_FOUND: &str = "message.local_executable_not_found_enter_its_absolute_path";
static LOOKUPS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(2);
static DIAGNOSTICS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(2);

fn local_path(path: &Path) -> bool {
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};
        matches!(path.components().next(), Some(Component::Prefix(prefix)) if matches!(prefix.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_)))
            && path.is_absolute()
    }
    #[cfg(unix)]
    path.is_absolute()
}

fn native_extension(path: &Path) -> bool {
    #[cfg(windows)]
    {
        path.extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| {
                extension.eq_ignore_ascii_case("exe") || extension.eq_ignore_ascii_case("com")
            })
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata()
            .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
    }
}

#[cfg(windows)]
fn script_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("cmd") || extension.eq_ignore_ascii_case("bat")
        })
}

fn executable(profile: &Document, environment: &Environment) -> Result<PathBuf> {
    let command = profile.get::<String>("command")?.unwrap_or_default();
    let kind = profile
        .get::<String>("type")?
        .ok_or("message.invalid_agent")?;
    let agent = atelier_core::domain::agent_catalog::find(&kind);
    #[cfg(windows)]
    let allow_script = agent.is_some_and(|agent| {
        agent.protocol != atelier_core::domain::agent_catalog::ProtocolKind::Command
    });
    let name = if command.is_empty() {
        agent
            .and_then(|agent| agent.executable)
            .ok_or("message.executable_required")?
    } else {
        command.as_str()
    };
    let mut candidates = Vec::new();
    if command.is_empty() {
        if let Some(directory) = agent.and_then(|agent| agent.home_directory)
            && let Some(home) = environment.get(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        {
            #[cfg(windows)]
            {
                for extension in ["exe", "com", "cmd", "bat"] {
                    if allow_script || extension == "exe" || extension == "com" {
                        candidates.push(
                            PathBuf::from(home)
                                .join(directory)
                                .join(format!("{name}.{extension}")),
                        );
                    }
                }
            }
            #[cfg(unix)]
            candidates.push(PathBuf::from(home).join(directory).join(name));
        }
        #[cfg(windows)]
        if let Some(directory) = agent.and_then(|agent| agent.local_directory)
            && let Some(local) = environment.get("LOCALAPPDATA")
        {
            candidates.push(
                PathBuf::from(local)
                    .join(directory)
                    .join(format!("{name}.exe")),
            );
        }
        #[cfg(unix)]
        {
            if let Some(home) = environment.get("HOME") {
                candidates.push(PathBuf::from(home).join(".local/bin").join(name));
                candidates.push(PathBuf::from(home).join(".cargo/bin").join(name));
            }
            #[cfg(target_os = "macos")]
            for root in ["/opt/homebrew/bin", "/usr/local/bin"] {
                candidates.push(PathBuf::from(root).join(name));
            }
        }
    }
    let path = Path::new(name);
    if path.is_absolute() {
        candidates.push(path.to_owned());
    } else if !name.contains(['/', '\\', ':']) {
        let filenames = vec![name.to_owned()];
        #[cfg(windows)]
        let filenames = if path.extension().is_none() {
            let mut names = vec![format!("{name}.exe"), format!("{name}.com")];
            if allow_script {
                names.extend([format!("{name}.cmd"), format!("{name}.bat")]);
            }
            names
        } else {
            filenames
        };
        if let Some(search) = environment.get("PATH") {
            candidates.extend(
                std::env::split_paths(search)
                    .take(256)
                    .filter(|path| local_path(path))
                    .flat_map(|directory| filenames.iter().map(move |name| directory.join(name))),
            );
        }
    }
    for candidate in candidates {
        #[cfg(windows)]
        let acceptable =
            |path: &Path| native_extension(path) || (allow_script && script_extension(path));
        #[cfg(unix)]
        let acceptable = native_extension;
        if !local_path(&candidate) || !acceptable(&candidate) {
            continue;
        }
        if let Ok(path) = candidate.canonicalize()
            && local_path(&path)
            && path.is_file()
            && acceptable(&path)
        {
            return Ok(path);
        }
    }
    Err(EXECUTABLE_NOT_FOUND.into())
}

pub(super) fn environment(
    profile: &Document,
    inherited: &Environment,
    authentication: bool,
) -> Result<Environment> {
    let allowed = [
        "PATH",
        "PATHEXT",
        "SYSTEMROOT",
        "WINDIR",
        "COMSPEC",
        "SYSTEMDRIVE",
        "HOME",
        "USERPROFILE",
        "APPDATA",
        "LOCALAPPDATA",
        "PROGRAMDATA",
        "PROGRAMFILES",
        "PROGRAMFILES(X86)",
        "TEMP",
        "TMP",
        "TMPDIR",
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "XDG_STATE_HOME",
        "XDG_CACHE_HOME",
        "LANG",
        "TERM",
        "COLORTERM",
    ];
    let mut selected: Environment = inherited
        .iter()
        .filter(|(key, _)| allowed.contains(&key.as_str()) || key.starts_with("LC_"))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    let kind = profile.get::<String>("type")?.unwrap_or_default();
    if let Some((key, value)) = atelier_core::domain::agent_catalog::find(&kind)
        .and_then(|agent| agent.disable_update_environment)
    {
        selected.insert(key.into(), value.into());
    }
    let config = profile.get::<String>("configDir")?.unwrap_or_default();
    let home_key =
        atelier_core::domain::agent_catalog::find(&kind).and_then(|agent| agent.config_environment);
    if let Some(home_key) = home_key {
        if !config.is_empty() {
            selected.insert(home_key.into(), config.into());
        } else if let Some(value) = inherited.get(home_key) {
            selected.insert(home_key.into(), value.clone());
        }
    }
    if authentication
        && profile
            .get::<String>("configDir")?
            .unwrap_or_default()
            .is_empty()
    {
        let keys = atelier_core::domain::agent_catalog::find(&kind)
            .map(|agent| agent.secret_environment)
            .unwrap_or_default();
        for key in keys {
            if let Some(value) = inherited.get(*key) {
                selected.insert((*key).into(), value.clone());
            }
        }
    }
    Ok(selected)
}

pub(super) async fn capture(
    binary: &Path,
    args: &[String],
    environment: &Environment,
) -> Result<std::process::Output> {
    let mut command = tokio::process::Command::new(binary);
    command
        .args(args)
        .current_dir(
            binary
                .parent()
                .ok_or("message.executable_directory_missing")?,
        )
        .env_clear()
        .envs(environment);
    process_tree::capture(&mut command, &[], Duration::from_secs(10), 65_536).await
}

pub(super) fn version(output: &[u8]) -> Option<String> {
    use std::sync::OnceLock;
    static VERSION: OnceLock<regex::Regex> = OnceLock::new();
    let expression = VERSION.get_or_init(|| {
        regex::Regex::new(
            r"(?:^|\s)v?([0-9]{1,5}\.[0-9]{1,5}\.[0-9]{1,5}(?:-[a-zA-Z0-9.-]{1,40})?)(?:\s|$)",
        )
        .expect("static version expression")
    });
    let text = std::str::from_utf8(output).ok()?;
    expression
        .captures(text)
        .map(|captures| captures[1].to_owned())
}

fn authentication(
    result: &mut AgentDiagnostic,
    kind: &str,
    output: &std::process::Output,
    environment: &Environment,
) -> Result<()> {
    if !matches!(output.status.code(), Some(0 | 1)) {
        return Err("message.login_check_unavailable".into());
    }
    let probe = atelier_core::domain::agent_catalog::find(kind)
        .map(|agent| agent.authentication_probe)
        .ok_or("message.invalid_agent")?;
    if probe == atelier_core::domain::agent_catalog::AuthenticationProbe::ExitStatus {
        result.authentication = if output.status.success() {
            AgentAuthentication::Present
        } else {
            AgentAuthentication::Absent
        };
        // The exit status is the documented contract; raw login output can contain credentials.
        return Ok(());
    }
    if probe == atelier_core::domain::agent_catalog::AuthenticationProbe::None {
        return Err("message.login_check_unavailable".into());
    }
    let value: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    let logged_in = value["loggedIn"]
        .as_bool()
        .ok_or("message.login_response_invalid")?;
    if logged_in && !output.status.success() {
        return Err("message.login_response_inconsistent".into());
    }
    result.authentication = if logged_in {
        AgentAuthentication::Present
    } else {
        AgentAuthentication::Absent
    };
    if logged_in {
        result.account = value["email"]
            .as_str()
            .filter(|email| {
                email.len() <= 200 && email.contains('@') && !email.chars().any(char::is_control)
            })
            .filter(|email| {
                !environment.iter().any(|(key, value)| {
                    (key.contains("KEY") || key.contains("TOKEN"))
                        && value
                            .to_str()
                            .is_some_and(|secret| !secret.is_empty() && email.contains(secret))
                })
            })
            .map(str::to_owned);
        result.auth_method = value["authMethod"]
            .as_str()
            .filter(|method| {
                [
                    "cli",
                    "api",
                    "ChatGPT",
                    "claude.ai",
                    "oauth_token",
                    "api_key",
                    "api_key_helper",
                    "third_party",
                    "none",
                ]
                .contains(method)
            })
            .map(str::to_owned);
        let mut report = atelier_core::application::agent_account::Report::unavailable(
            atelier_core::application::agent_account::State::Unavailable,
        );
        report.subscription = value["subscriptionType"]
            .as_str()
            .filter(|value| {
                !value.is_empty() && value.len() <= 80 && !value.chars().any(char::is_control)
            })
            .filter(|value| {
                !environment.iter().any(|(key, secret)| {
                    (key.contains("KEY") || key.contains("TOKEN"))
                        && secret
                            .to_str()
                            .is_some_and(|secret| !secret.is_empty() && value.contains(secret))
                })
            })
            .map(str::to_owned);
        result.account_report = Some(report);
    }
    Ok(())
}

pub(super) async fn resolve(profile: &Document) -> Result<(PathBuf, Environment)> {
    let permit = LOOKUPS
        .try_acquire()
        .map_err(|_| "message.executable_lookup_is_busy_try_again_shortly")?;
    let inherited: Environment = std::env::vars_os()
        .map(|(key, value)| (key.to_string_lossy().to_ascii_uppercase(), value))
        .collect();
    let input = profile.clone();
    let search = environment(profile, &inherited, false)?;
    let lookup = tokio::task::spawn_blocking(move || {
        // Keep the slot until filesystem I/O returns, even if the HTTP deadline expires.
        let _permit = permit;
        executable(&input, &search)
    });
    let resolved = tokio::time::timeout(Duration::from_secs(3), lookup)
        .await
        .map_err(|_| "message.executable_lookup_timed_out")?;
    let binary = resolved??;
    Ok((binary, inherited))
}

#[cfg(windows)]
fn script_on_path(name: &str, search: &std::ffi::OsStr) -> bool {
    std::env::split_paths(search)
        .take(256)
        .filter(|directory| local_path(directory))
        .flat_map(|directory| {
            ["cmd", "bat"].map(move |extension| directory.join(format!("{name}.{extension}")))
        })
        .any(|candidate| {
            candidate
                .canonicalize()
                .is_ok_and(|path| local_path(&path) && path.is_file())
        })
}

#[cfg(windows)]
async fn script_present(name: &'static str) -> Result<bool> {
    let permit = LOOKUPS
        .try_acquire()
        .map_err(|_| "message.executable_lookup_is_busy_try_again_shortly")?;
    let search = std::env::var_os("PATH").unwrap_or_default();
    let lookup = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        script_on_path(name, &search)
    });
    Ok(tokio::time::timeout(Duration::from_secs(3), lookup)
        .await
        .map_err(|_| "message.executable_lookup_timed_out")??)
}

fn discovery_profile(agent: &atelier_core::domain::agent_catalog::AgentType) -> Result<Document> {
    let mut profile = Document::default();
    if agent.supported {
        profile.set("type", &agent.id)?;
    } else {
        profile.set("type", &"command")?;
        profile.set("command", &agent.executable)?;
    }
    Ok(profile)
}

pub async fn discover() -> Result<Vec<atelier_core::contracts::DiscoveredAgent>> {
    let mut agents = Vec::new();
    for agent in atelier_core::domain::agent_catalog::AGENTS {
        if agent.executable.is_none() {
            continue;
        }
        let id = agent.id;
        let profile = discovery_profile(agent)?;
        let resolved = match resolve(&profile).await {
            Ok(path) => Some(path),
            Err(error) if error.to_string() == EXECUTABLE_NOT_FOUND => None,
            Err(error) => return Err(error),
        };
        #[cfg(windows)]
        let installed =
            resolved.is_some() || script_present(agent.executable.unwrap_or(id)).await?;
        #[cfg(unix)]
        let installed = resolved.is_some();
        agents.push(atelier_core::contracts::DiscoveredAgent {
            id: id.into(),
            name: agent.name.into(),
            installed,
            supported: agent.supported,
            script_only: installed && resolved.is_none(),
        });
    }
    Ok(agents)
}

pub(crate) async fn command(profile: &Document) -> Result<tokio::process::Command> {
    let (binary, inherited) = resolve(profile).await?;
    let mut command = tokio::process::Command::new(binary);
    command
        .env_clear()
        .envs(environment(profile, &inherited, false)?);
    Ok(command)
}

pub(crate) async fn agent_command(
    profile: &Document,
    api_key: &str,
) -> Result<(tokio::process::Command, Vec<String>)> {
    let (binary, inherited) = resolve(profile).await?;
    let api = profile.string_equals("auth", "api")?;
    let mut selected = environment(profile, &inherited, !api)?;
    if api {
        for key in ["CODEX_API_KEY", "OPENAI_API_KEY", "CODEX_ACCESS_TOKEN"] {
            selected.remove(key);
        }
    }
    let mut secrets = selected
        .iter()
        .filter(|(key, _)| key.contains("KEY") || key.contains("TOKEN"))
        .filter_map(|(_, value)| value.to_str().map(str::to_owned))
        .collect::<Vec<_>>();
    if !api_key.is_empty() {
        secrets.push(api_key.into());
    }
    let mut command = tokio::process::Command::new(binary);
    command.env_clear().envs(selected);
    Ok((command, secrets))
}

pub async fn diagnose(
    profile: Document,
    probe_model: Option<&str>,
    status_only: bool,
) -> Result<AgentDiagnostic> {
    let _permit = DIAGNOSTICS
        .try_acquire()
        .map_err(|_| atelier_core::application::agent_providers::ProviderFailure::Busy)?;
    let (binary, inherited) = resolve(&profile).await?;
    let kind = profile
        .get::<String>("type")?
        .ok_or("message.invalid_agent")?;
    let api = profile.string_equals("auth", "api")?;
    let mut result = AgentDiagnostic {
        executable: path_string(&binary),
        version: None,
        version_error: None,
        authentication: if api {
            if profile.get::<bool>("apiKeySet")?.unwrap_or(false) {
                AgentAuthentication::Configured
            } else {
                AgentAuthentication::MissingKey
            }
        } else {
            AgentAuthentication::Unavailable
        },
        authentication_error: None,
        account: None,
        auth_method: None,
        account_report: None,
        update_info: None,
        models: Vec::new(),
    };
    let args = if kind == "command" {
        profile
            .get::<Vec<String>>("versionArgs")?
            .unwrap_or_else(|| vec!["--version".into()])
    } else {
        vec!["--version".into()]
    };
    if !args.is_empty() {
        match capture(&binary, &args, &environment(&profile, &inherited, false)?).await {
            Ok(output) if output.status.success() => result.version = version(&output.stdout),
            _ => {
                result.version_error =
                    Some("message.could_not_read_the_version_or_the_check_was_interrupted".into())
            }
        }
        if result.version.is_none() && result.version_error.is_none() {
            result.version_error =
                Some("message.the_command_did_not_return_a_recognized_version_number".into());
        }
    }
    if !api && result.version_error.is_none() {
        let agent =
            atelier_core::domain::agent_catalog::find(&kind).ok_or("message.invalid_agent")?;
        let args = if agent.protocol == atelier_core::domain::agent_catalog::ProtocolKind::Command {
            profile
                .get::<Vec<String>>("statusArgs")?
                .unwrap_or_default()
        } else {
            agent.status_args.iter().map(|arg| (*arg).into()).collect()
        };
        if !args.is_empty() {
            let environment = environment(&profile, &inherited, true)?;
            let success = match capture(&binary, &args, &environment).await {
                Ok(output) => authentication(&mut result, &kind, &output, &environment).is_ok(),
                Err(_) => false,
            };
            if !success {
                result.authentication_error = Some("message.login_check_unavailable".into());
            }
        }
    }
    if status_only {
        return Ok(result);
    }
    if let Some(probe_model) = probe_model
        && result.version_error.is_none()
        && !api
        && let Some(agent) = atelier_core::domain::agent_catalog::find(&kind)
    {
        let args = if kind == "acp" {
            profile
                .get::<Vec<String>>("protocolArgs")?
                .unwrap_or_default()
        } else if agent.protocol == atelier_core::domain::agent_catalog::ProtocolKind::Codex {
            vec!["app-server".into()]
        } else if agent.protocol == atelier_core::domain::agent_catalog::ProtocolKind::Claude {
            [
                "-p",
                "--input-format",
                "stream-json",
                "--output-format",
                "stream-json",
                "--verbose",
                "--no-session-persistence",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect()
        } else {
            agent
                .launch_args
                .iter()
                .map(|arg| (*arg).to_owned())
                .collect()
        };
        result.models = super::agent_models::read(
            &binary,
            &environment(&profile, &inherited, true)?,
            agent.protocol,
            &args,
            probe_model,
        )
        .await
        .unwrap_or_default();
    }
    use atelier_core::application::agent_account::{Report, State};
    if matches!(kind.as_str(), "codex" | "claude") {
        let metadata = result.account_report.take();
        let mut report = if api {
            Report::unavailable(State::Api)
        } else if result.authentication == AgentAuthentication::Absent {
            Report::unavailable(State::Disconnected)
        } else if result.authentication == AgentAuthentication::Present
            && result.version_error.is_none()
        {
            let environment = environment(&profile, &inherited, true)?;
            let read = if kind == "codex" {
                super::agent_account::read(&binary, &environment).await
            } else {
                super::claude_account::from_profile(&profile, environment).await
            };
            read.unwrap_or_else(|_| Report::unavailable(State::Unavailable))
        } else {
            Report::unavailable(State::Unavailable)
        };
        if report.account.is_none() {
            report.account = result.account.clone();
        }
        if report.subscription.is_none() {
            report.subscription = metadata.and_then(|report| report.subscription);
        }
        if report.observed_at.is_empty() {
            report.observed_at = time::OffsetDateTime::now_utc()
                .format(&time::format_description::well_known::Rfc3339)?;
        }
        result.account_report = Some(report);
    } else if atelier_core::domain::agent_catalog::find(&kind).is_some_and(|agent| {
        agent.protocol == atelier_core::domain::agent_catalog::ProtocolKind::Acp
    }) {
        let mut report = Report::unavailable(State::Unavailable);
        report.observed_at = time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)?;
        result.account_report = Some(report);
    }
    #[cfg(windows)]
    {
        result.update_info = Some(
            super::agent_cli::describe(&profile, &binary, result.version.as_deref(), &inherited)
                .await?,
        );
    }
    #[cfg(unix)]
    {
        if !atelier_core::domain::agent_catalog::find(&kind).is_some_and(|agent| {
            agent.protocol == atelier_core::domain::agent_catalog::ProtocolKind::Acp
        }) {
            result.update_info = Some(atelier_core::application::agent_cli::UpdateInfo {
                package: atelier_core::domain::agent_catalog::find(&kind)
                    .and_then(|agent| agent.registry_package)
                    .unwrap_or_default()
                    .into(),
                latest_version: None,
                managed_root: None,
                observed_at: time::OffsetDateTime::now_utc()
                    .format(&time::format_description::well_known::Rfc3339)?,
                availability: atelier_core::application::agent_cli::Availability::Managed,
                installed_version: result.version.clone(),
                image: None,
            });
        }
    }
    #[cfg(unix)]
    if atelier_core::domain::agent_catalog::find(&kind).is_some_and(|agent| {
        agent.protocol == atelier_core::domain::agent_catalog::ProtocolKind::Acp
    }) {
        result.update_info = Some(atelier_core::application::agent_cli::UpdateInfo {
            package: String::new(),
            latest_version: None,
            managed_root: None,
            observed_at: time::OffsetDateTime::now_utc()
                .format(&time::format_description::well_known::Rfc3339)?,
            availability: if atelier_core::domain::agent_catalog::find(&kind)
                .is_some_and(|agent| agent.executable.is_none())
                || !profile
                    .get::<String>("command")?
                    .unwrap_or_default()
                    .is_empty()
            {
                atelier_core::application::agent_cli::Availability::Custom
            } else {
                atelier_core::application::agent_cli::Availability::Managed
            },
            installed_version: result.version.clone(),
            image: None,
        });
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn acp_scripts_are_discovered_without_being_executed() {
        let fixture = tempfile::tempdir().unwrap();
        std::fs::write(fixture.path().join("gemini.cmd"), b"not executed").unwrap();
        assert!(script_on_path("gemini", fixture.path().as_os_str()));
        let profile: Document = serde_json::from_str(r#"{"type":"gemini"}"#).unwrap();
        let environment = Environment::from([("PATH".into(), fixture.path().into())]);
        assert_eq!(
            executable(&profile, &environment).unwrap(),
            fixture.path().join("gemini.cmd").canonicalize().unwrap()
        );
    }

    #[test]
    fn discovery_uses_catalog_names_and_never_executes_candidates() {
        #[cfg(unix)]
        use std::os::unix::fs::PermissionsExt;
        let fixture = tempfile::tempdir().unwrap();
        let name = if cfg!(windows) {
            "opencode.exe"
        } else {
            "opencode"
        };
        let binary = fixture.path().join(name);
        std::fs::write(&binary, b"not an executable image").unwrap();
        #[cfg(unix)]
        {
            std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let environment = Environment::from([("PATH".into(), fixture.path().into())]);
        let agent = atelier_core::domain::agent_catalog::AGENTS
            .iter()
            .find(|a| a.id == "opencode")
            .unwrap();
        assert!(agent.supported);
        assert_eq!(
            executable(&discovery_profile(agent).unwrap(), &environment).unwrap(),
            binary.canonicalize().unwrap()
        );
        let missing = atelier_core::domain::agent_catalog::AGENTS
            .iter()
            .find(|a| a.id == "grok")
            .unwrap();
        assert!(executable(&discovery_profile(missing).unwrap(), &environment).is_err());
        let home_bin = fixture.path().join(".grok/bin");
        std::fs::create_dir_all(&home_bin).unwrap();
        let grok = home_bin.join(if cfg!(windows) { "grok.exe" } else { "grok" });
        std::fs::write(&grok, b"not an executable image").unwrap();
        #[cfg(unix)]
        std::fs::set_permissions(&grok, std::fs::Permissions::from_mode(0o700)).unwrap();
        let home_key = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
        let environment = Environment::from([(home_key.into(), fixture.path().into())]);
        assert_eq!(
            executable(&discovery_profile(missing).unwrap(), &environment).unwrap(),
            grok.canonicalize().unwrap()
        );
        let path_binary = fixture
            .path()
            .join(if cfg!(windows) { "grok.exe" } else { "grok" });
        std::fs::write(&path_binary, b"not an executable image").unwrap();
        #[cfg(unix)]
        std::fs::set_permissions(&path_binary, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut explicit = discovery_profile(missing).unwrap();
        explicit.set("command", &"grok").unwrap();
        let environment = Environment::from([
            (home_key.into(), fixture.path().into()),
            ("PATH".into(), fixture.path().into()),
        ]);
        assert_eq!(
            executable(&explicit, &environment).unwrap(),
            path_binary.canonicalize().unwrap()
        );
    }

    #[test]
    fn acp_environment_uses_configured_home_and_disables_automatic_updates() {
        let profile: Document =
            serde_json::from_str(r#"{"type":"grok","configDir":"C:/synthetic/grok"}"#).unwrap();
        let inherited = Environment::from([
            ("GROK_HOME".into(), "C:/synthetic/old".into()),
            ("XAI_API_KEY".into(), "synthetic-secret".into()),
        ]);
        let selected = environment(&profile, &inherited, true).unwrap();
        assert_eq!(selected["GROK_HOME"], OsString::from("C:/synthetic/grok"));
        assert_eq!(selected["GROK_DISABLE_AUTOUPDATER"], OsString::from("1"));
        assert!(!selected.contains_key("XAI_API_KEY"));
    }

    #[cfg(windows)]
    #[test]
    fn finds_com_executable_on_path() {
        let fixture = tempfile::tempdir().unwrap();
        let binary = fixture.path().join("synthetic.com");
        std::fs::write(&binary, b"not executed").unwrap();
        let profile: Document =
            serde_json::from_str(r#"{"type":"command","command":"synthetic"}"#).unwrap();
        let environment = Environment::from([("PATH".into(), fixture.path().into())]);
        assert_eq!(
            executable(&profile, &environment).unwrap(),
            binary.canonicalize().unwrap()
        );
    }

    #[test]
    fn diagnostics_drop_injection_and_secrets_and_do_not_search_relative_or_shell_commands() {
        let inherited = Environment::from([
            ("PATH".into(), "relative".into()),
            ("NODE_OPTIONS".into(), "injection".into()),
            ("OPENAI_API_KEY".into(), "synthetic-key".into()),
            ("JIRA_TOKEN".into(), "synthetic-jira".into()),
            ("CODEX_HOME".into(), "C:/synthetic/inherited".into()),
            ("SYSTEMROOT".into(), "C:/Windows".into()),
        ]);
        let mut profile: Document =
            serde_json::from_str(r#"{"type":"codex","command":"synthetic.cmd"}"#).unwrap();
        assert!(executable(&profile, &inherited).is_err());
        let version_env = environment(&profile, &inherited, false).unwrap();
        assert!(
            !version_env.contains_key("NODE_OPTIONS")
                && !version_env.contains_key("OPENAI_API_KEY")
                && !version_env.contains_key("JIRA_TOKEN")
        );
        assert!(
            environment(&profile, &inherited, true)
                .unwrap()
                .contains_key("OPENAI_API_KEY")
        );
        profile.set("configDir", &"C:/synthetic/isolated").unwrap();
        let selected = environment(&profile, &inherited, true).unwrap();
        assert!(!selected.contains_key("OPENAI_API_KEY"));
        assert_eq!(
            selected["CODEX_HOME"],
            OsString::from("C:/synthetic/isolated")
        );
        assert_eq!(
            version(b"synthetic-cli 1.2.3-beta.2\n").as_deref(),
            Some("1.2.3-beta.2")
        );
        assert!(version(b"synthetic secret; unrelated failure").is_none());
        assert!(version(b"token-prefix-1.2.3-token").is_none());
        assert!(!local_path(Path::new(r"\\server\share\agent.exe")));
        assert!(!local_path(Path::new(r"C:agent.exe")));
        let fixture = tempfile::tempdir().unwrap();
        let binary = fixture.path().join("synthetic.EXE");
        std::fs::write(&binary, b"not executed").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        profile.set("command", &binary.to_string_lossy()).unwrap();
        assert_eq!(
            executable(&profile, &inherited).unwrap(),
            binary.canonicalize().unwrap()
        );
    }

    #[test]
    fn login_status_is_not_a_network_check_and_unselected_output_is_never_returned() {
        #[cfg(unix)]
        use std::os::unix::process::ExitStatusExt;
        #[cfg(windows)]
        use std::os::windows::process::ExitStatusExt;
        let mut diagnostic = AgentDiagnostic {
            executable: "synthetic.exe".into(),
            version: None,
            version_error: None,
            authentication: AgentAuthentication::Unavailable,
            authentication_error: None,
            account: None,
            auth_method: None,
            account_report: None,
            update_info: None,
            models: Vec::new(),
        };
        let mut output = std::process::Output { status:std::process::ExitStatus::from_raw(0), stdout:br#"{"loggedIn":true,"email":"synthetic@example.invalid","authMethod":"api_key","accessToken":"never expose this"}"#.to_vec(), stderr:b"secret stderr".to_vec() };
        authentication(&mut diagnostic, "claude", &output, &Environment::new()).unwrap();
        assert_eq!(diagnostic.authentication, AgentAuthentication::Present);
        assert_eq!(
            diagnostic.account.as_deref(),
            Some("synthetic@example.invalid")
        );
        assert!(
            !serde_json::to_string(&diagnostic)
                .unwrap()
                .contains("never expose")
        );
        diagnostic.account = None;
        let credentials = Environment::from([(
            "ANTHROPIC_API_KEY".into(),
            "synthetic@example.invalid".into(),
        )]);
        authentication(&mut diagnostic, "claude", &output, &credentials).unwrap();
        assert!(diagnostic.account.is_none());
        output.status = std::process::ExitStatus::from_raw(if cfg!(windows) { 1 } else { 1 << 8 });
        assert!(authentication(&mut diagnostic, "claude", &output, &credentials).is_err());
        authentication(&mut diagnostic, "codex", &output, &credentials).unwrap();
        assert_eq!(diagnostic.authentication, AgentAuthentication::Absent);
        output.status = std::process::ExitStatus::from_raw(0);
        output.stdout = b"malformed synthetic secret".to_vec();
        assert!(authentication(&mut diagnostic, "command", &output, &credentials).is_err());
    }
}
