use super::{process_tree, task_creation::path_string};
use crate::Result;
use atelier_core::{
    contracts::{AgentAuthentication, AgentDiagnostic},
    document::Document,
};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    path::{Component, Path, PathBuf, Prefix},
    time::Duration,
};

type Environment = BTreeMap<String, OsString>;

fn local_path(path: &Path) -> bool {
    matches!(path.components().next(), Some(Component::Prefix(prefix)) if matches!(prefix.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_)))
        && path.is_absolute()
}

fn native_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("exe") || extension.eq_ignore_ascii_case("com")
        })
}

fn executable(profile: &Document, environment: &Environment) -> Result<PathBuf> {
    let command = profile.get::<String>("command")?.unwrap_or_default();
    let kind = profile.get::<String>("type")?.ok_or("Agent invalide.")?;
    let name = if command.is_empty() { &kind } else { &command };
    let mut candidates = Vec::new();
    if command.is_empty() {
        match kind.as_str() {
            "codex" => {
                if let Some(local) = environment.get("LOCALAPPDATA") {
                    candidates
                        .push(PathBuf::from(local).join("Programs/OpenAI/Codex/bin/codex.exe"));
                }
            }
            "claude" => {
                if let Some(home) = environment.get("USERPROFILE") {
                    candidates.push(PathBuf::from(home).join(".local/bin/claude.exe"));
                }
            }
            _ => return Err("Exécutable requis.".into()),
        }
    }
    let path = Path::new(name);
    if path.is_absolute() {
        candidates.push(path.to_owned());
    } else if !name.contains(['/', '\\', ':']) {
        let filename = if path.extension().is_some() {
            name.clone()
        } else {
            format!("{name}.exe")
        };
        if let Some(search) = environment.get("PATH") {
            candidates.extend(
                std::env::split_paths(search)
                    .take(256)
                    .filter(|path| local_path(path))
                    .map(|directory| directory.join(&filename)),
            );
        }
    }
    for candidate in candidates {
        if !local_path(&candidate) || !native_extension(&candidate) {
            continue;
        }
        if let Ok(path) = candidate.canonicalize()
            && local_path(&path)
            && path.is_file()
            && native_extension(&path)
        {
            return Ok(path);
        }
    }
    Err("Exécutable local introuvable. Renseignez son chemin absolu.".into())
}

fn environment(
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
    let config = profile.get::<String>("configDir")?.unwrap_or_default();
    let home_key = match kind.as_str() {
        "codex" => Some("CODEX_HOME"),
        "claude" => Some("CLAUDE_CONFIG_DIR"),
        _ => None,
    };
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
        let keys: &[&str] = match kind.as_str() {
            "codex" => &["CODEX_API_KEY", "OPENAI_API_KEY", "CODEX_ACCESS_TOKEN"],
            "claude" => &[
                "ANTHROPIC_API_KEY",
                "ANTHROPIC_AUTH_TOKEN",
                "CLAUDE_CODE_OAUTH_TOKEN",
            ],
            _ => &[],
        };
        for key in keys {
            if let Some(value) = inherited.get(*key) {
                selected.insert((*key).into(), value.clone());
            }
        }
    }
    Ok(selected)
}

async fn capture(
    binary: &Path,
    args: &[String],
    environment: &Environment,
) -> Result<std::process::Output> {
    let mut command = tokio::process::Command::new(binary);
    command
        .args(args)
        .current_dir(binary.parent().ok_or("Dossier de l’exécutable absent.")?)
        .env_clear()
        .envs(environment);
    process_tree::capture(&mut command, &[], Duration::from_secs(10), 65_536).await
}

fn version(output: &[u8]) -> Option<String> {
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
        return Err("Diagnostic de connexion indisponible.".into());
    }
    if kind == "codex" {
        result.authentication = if output.status.success() {
            AgentAuthentication::Present
        } else {
            AgentAuthentication::Absent
        };
        // The exit status is the documented contract; raw login output can contain credentials.
        return Ok(());
    }
    let value: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    let logged_in = value["loggedIn"]
        .as_bool()
        .ok_or("Réponse de connexion invalide.")?;
    if logged_in && !output.status.success() {
        return Err("Réponse de connexion incohérente.".into());
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
    }
    Ok(())
}

pub async fn diagnose(profile: Document) -> Result<AgentDiagnostic> {
    static LOOKUPS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(2);
    let permit = LOOKUPS
        .try_acquire()
        .map_err(|_| "La recherche des exécutables est occupée. Réessaie dans un instant.")?;
    let inherited: Environment = std::env::vars_os()
        .map(|(key, value)| (key.to_string_lossy().to_ascii_uppercase(), value))
        .collect();
    let input = profile.clone();
    let search = environment(&profile, &inherited, false)?;
    let lookup = tokio::task::spawn_blocking(move || {
        // Keep the slot until filesystem I/O returns, even if the HTTP deadline expires.
        let _permit = permit;
        executable(&input, &search)
    });
    let resolved = tokio::time::timeout(Duration::from_secs(3), lookup)
        .await
        .map_err(|_| "La recherche de l’exécutable a dépassé son délai.")?;
    let binary = resolved??;
    let kind = profile.get::<String>("type")?.ok_or("Agent invalide.")?;
    let api = profile.string_equals("auth", "api")?;
    let mut result = AgentDiagnostic {
        executable: path_string(&binary).utf8()?,
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
                result.version_error = Some("Lecture de version impossible ou interrompue.".into())
            }
        }
        if result.version.is_none() && result.version_error.is_none() {
            result.version_error =
                Some("La commande ne renvoie pas de numéro de version reconnu.".into());
        }
    }
    if !api && result.version_error.is_none() {
        let args = match kind.as_str() {
            "codex" => vec!["login".into(), "status".into()],
            "claude" => vec!["auth".into(), "status".into()],
            "command" => profile
                .get::<Vec<String>>("statusArgs")?
                .unwrap_or_default(),
            _ => return Err("Agent invalide.".into()),
        };
        if !args.is_empty() {
            let environment = environment(&profile, &inherited, true)?;
            let success = match capture(&binary, &args, &environment).await {
                Ok(output) => authentication(&mut result, &kind, &output, &environment).is_ok(),
                Err(_) => false,
            };
            if !success {
                result.authentication_error = Some("Diagnostic de connexion indisponible.".into());
            }
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        profile.set("command", &binary.to_string_lossy()).unwrap();
        assert_eq!(
            executable(&profile, &inherited).unwrap(),
            binary.canonicalize().unwrap()
        );
    }

    #[test]
    fn login_status_is_not_a_network_check_and_unselected_output_is_never_returned() {
        use std::os::windows::process::ExitStatusExt;
        let mut diagnostic = AgentDiagnostic {
            executable: "synthetic.exe".into(),
            version: None,
            version_error: None,
            authentication: AgentAuthentication::Unavailable,
            authentication_error: None,
            account: None,
            auth_method: None,
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
        output.status = std::process::ExitStatus::from_raw(1);
        assert!(authentication(&mut diagnostic, "claude", &output, &credentials).is_err());
        authentication(&mut diagnostic, "codex", &output, &credentials).unwrap();
        assert_eq!(diagnostic.authentication, AgentAuthentication::Absent);
        output.status = std::process::ExitStatus::from_raw(0);
        output.stdout = b"malformed synthetic secret".to_vec();
        assert!(authentication(&mut diagnostic, "command", &output, &credentials).is_err());
    }
}
