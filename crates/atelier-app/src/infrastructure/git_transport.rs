use super::{files::Repository, forge, forge_association};
use crate::Result;
use atelier_core::contracts::ForgeConfiguration;
#[cfg(windows)]
use atelier_core::document::Document;
#[cfg(windows)]
use base64::{Engine, engine::general_purpose::STANDARD};
use reqwest::Url;

pub struct HttpsAuthentication {
    configuration: ForgeConfiguration,
    repository: String,
    synthetic: bool,
    #[cfg(windows)]
    authorization: String,
}

impl HttpsAuthentication {
    pub fn new(
        configuration: ForgeConfiguration,
        repository: String,
        token: String,
    ) -> Result<Self> {
        let configuration = forge::normalize(&configuration)?;
        Self::prepare(configuration, repository, token, false)
    }
    /// Restricted loopback constructor for synthetic Git HTTP doubles; never called by HTTP routes.
    pub fn synthetic(
        configuration: ForgeConfiguration,
        repository: String,
        token: String,
    ) -> Result<Self> {
        let url = Url::parse(&configuration.base)?;
        if url.scheme() != "http"
            || !url
                .host_str()
                .and_then(|host| {
                    host.trim_matches(['[', ']'])
                        .parse::<std::net::IpAddr>()
                        .ok()
                })
                .is_some_and(|host| host.is_loopback())
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || !token.starts_with("synthetic-")
        {
            return Err("Transport Git loopback synthétique requis.".into());
        }
        Self::prepare(configuration, repository, token, true)
    }
    fn prepare(
        configuration: ForgeConfiguration,
        repository: String,
        token: String,
        synthetic: bool,
    ) -> Result<Self> {
        atelier_core::application::forge::repository(configuration.kind, &repository, 1)?;
        if token.is_empty()
            || token.len() > 4000
            || token.chars().any(|c| c.is_control() || c.is_whitespace())
        {
            return Err("Jeton Git HTTPS invalide.".into());
        }
        Ok(Self {
            configuration,
            repository,
            synthetic,
            #[cfg(windows)]
            authorization: format!(
                "Authorization: Basic {}",
                STANDARD.encode(format!("atelier:{token}"))
            ),
        })
    }
    pub fn origin(&self, origin: &str) -> Result<String> {
        let origin = origin.trim_end_matches(['\r', '\n']);
        // URL parsing discards some whitespace; check the original before normalizing it.
        if origin.chars().any(|c| c.is_control() || c.is_whitespace()) {
            return Err("Origin HTTPS invalide.".into());
        }
        let mut requested = Url::parse(origin).map_err(|_| "Origin HTTPS invalide.")?;
        let mut configuration = self.configuration.clone();
        if self.synthetic {
            if requested.scheme() != "http" {
                return Err("Origin de test HTTP loopback requis.".into());
            }
            let mut base = Url::parse(&configuration.base)?;
            base.set_scheme("https")
                .map_err(|_| "Origin de test invalide.")?;
            configuration.base = base.into();
            requested
                .set_scheme("https")
                .map_err(|_| "Origin de test invalide.")?;
        }
        if requested.scheme() != "https"
            || !forge_association::matches_origin(
                &configuration,
                &self.repository,
                requested.as_str(),
            )?
        {
            return Err(
                "Origin HTTPS ne correspond pas à la connexion de forge et au dépôt choisis."
                    .into(),
            );
        }
        let mut url = Url::parse(origin)?;
        url.set_username("").map_err(|_| "Origin HTTPS invalide.")?;
        Ok(url.into())
    }
    pub fn validate_configuration(keys: &str) -> Result<()> {
        for key in keys.split('\0').filter(|key| !key.is_empty()) {
            let key = key.to_ascii_lowercase();
            if key.starts_with("url.")
                || key.starts_with("include.")
                || key.starts_with("includeif.")
                || key.starts_with("http.")
                || key.starts_with("credential.")
                || [
                    "core.askpass",
                    "core.sshcommand",
                    "core.gitproxy",
                    "extensions.worktreeconfig",
                    "extensions.partialclone",
                    "fetch.uriprotocols",
                ]
                .contains(&key.as_str())
                || key.starts_with("remote.")
                    && [".proxy", ".uploadpack", ".receivepack", ".vcs", ".promisor"]
                        .iter()
                        .any(|suffix| key.ends_with(suffix))
            {
                return Err("Le fetch authentifié refuse les surcharges locales de transport, d’identifiants, les inclusions et clones partiels. La configuration est conservée.".into());
            }
        }
        Ok(())
    }
    #[cfg(windows)]
    fn isolate(command: &mut tokio::process::Command, home: &std::path::Path) {
        command
            .current_dir(home)
            .env("HOME", home)
            .env("USERPROFILE", home)
            .env("APPDATA", home)
            .env("LOCALAPPDATA", home)
            .env("XDG_CONFIG_HOME", home)
            .env("GIT_CONFIG_GLOBAL", "NUL")
            .env("GIT_CONFIG_SYSTEM", "NUL")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_ASKPASS", "NUL")
            .env_remove("SSH_ASKPASS")
            .env("GCM_INTERACTIVE", "Never")
            .env("GIT_CONFIG_COUNT", "0");
    }
    #[cfg(windows)]
    fn configure_command(
        &self,
        command: &mut tokio::process::Command,
        home: &std::path::Path,
        origin: &str,
        reference: &str,
        private_ref: &str,
    ) -> Result<()> {
        atelier_core::application::worktrees::validate_reference(reference)?;
        if reference != "HEAD" && (!reference.starts_with("refs/heads/") || reference.contains(':'))
        {
            return Err("Référence distante Git invalide.".into());
        }
        if !private_ref.starts_with("refs/atelier-fetch/")
            || private_ref
                .strip_prefix("refs/atelier-fetch/")
                .and_then(|value| uuid::Uuid::parse_str(value).ok())
                .is_none()
        {
            return Err("Référence temporaire Git invalide.".into());
        }
        let scope = self.origin(origin)?;
        Self::isolate(command, home);
        command
            .env("GIT_CONFIG_COUNT", "1")
            .env("GIT_CONFIG_KEY_0", format!("http.{scope}.extraHeader"))
            .env("GIT_CONFIG_VALUE_0", &self.authorization)
            .args([
                "--no-pager",
                "--no-optional-locks",
                "--literal-pathspecs",
                "--no-lazy-fetch",
                "--no-replace-objects",
            ]);
        for option in [
            "core.fsmonitor=false",
            "core.longpaths=true",
            "core.hooksPath=NUL",
            "core.askPass=NUL",
            "credential.helper=",
            "credential.interactive=false",
            "credential.useHttpPath=true",
            "protocol.allow=never",
            "protocol.https.allow=always",
            "http.followRedirects=false",
            "http.proxy=",
            "http.sslVerify=true",
            "http.cookieFile=",
            "http.saveCookies=false",
            "http.authMethod=basic",
            "http.emptyAuth=false",
            "http.delegation=none",
            "http.maxRetries=0",
            "fetch.uriprotocols=",
            "fetch.recurseSubmodules=false",
            "fetch.writeCommitGraph=false",
            "submodule.recurse=false",
            "gc.auto=0",
            "maintenance.auto=false",
            "pack.threads=1",
            "index.threads=1",
        ] {
            command.args(["-c", option]);
        }
        if self.synthetic {
            command.args(["-c", "protocol.http.allow=always"]);
        }
        command.args([
            "fetch",
            "--no-tags",
            "--no-write-fetch-head",
            "--no-recurse-submodules",
            "--no-auto-maintenance",
            "--no-prune",
            "--refmap=",
            "--",
            &scope,
            &format!("{reference}:{private_ref}"),
        ]);
        Ok(())
    }
    #[cfg(windows)]
    pub(super) async fn fetch(
        &self,
        source: &Repository,
        origin: &str,
        reference: &str,
        private_ref: &str,
    ) -> Result<()> {
        let mut profile = Document::default();
        profile.set("type", &"command")?;
        profile.set("command", &"git")?;
        let home = tempfile::Builder::new()
            .prefix("atelier-git-home-")
            .tempdir()?;
        // Network Git sees only this private bare repository, never source transport configuration.
        let captured = async {
            let format = String::from_utf8(source.git(&["rev-parse", "--show-object-format"]).await?)
                .map_err(|_| "Format d’objets Git invalide.")?;
            let format = format.trim();
            if !["sha1", "sha256"].contains(&format) {
                return Err("Format d’objets Git non pris en charge.".into());
            }
            let mut initialize = super::agent_diagnostics::command(&profile).await?;
            Self::isolate(&mut initialize, home.path());
            initialize.args(["init", "--bare", "--quiet", "--template=", &format!("--object-format={format}")]);
            let initialized = super::process_tree::capture(&mut initialize, &[], std::time::Duration::from_secs(15), 1024 * 1024)
                .await.map_err(|_| "Préparation du téléchargement Git isolé impossible.")?;
            if !initialized.status.success() {
                return Err("Préparation du téléchargement Git isolé impossible.".into());
            }
            let mut command = super::agent_diagnostics::command(&profile).await?;
            self.configure_command(&mut command, home.path(), origin, reference, private_ref)?;
            let output = super::process_tree::capture(&mut command, &[], std::time::Duration::from_secs(120), 4 * 1024 * 1024)
                .await.map_err(|_| "Fetch HTTPS interrompu, trop long ou trop volumineux. Aucun détail d’authentification n’est affiché.")?;
            if !output.status.success() {
                return Err("Fetch HTTPS refusé. Vérifie l’URL exacte d’origin, le jeton et les droits du dépôt ; aucun helper ni dialogue de connexion n’est lancé.".into());
            }
            let path = super::task_creation::path_string(home.path()).utf8()?;
            source.without_programs(&[
                "-c", "protocol.allow=never", "-c", "protocol.file.allow=always",
                "-c", "pack.threads=1", "-c", "gc.auto=0", "fetch", "--no-tags",
                "--no-write-fetch-head", "--no-recurse-submodules", "--no-auto-maintenance",
                "--no-prune", "--refmap=", "--", &path, &format!("{private_ref}:{private_ref}"),
            ]).await?;
            Ok::<(), crate::Error>(())
        }.await;
        if home.close().is_err() {
            return Err("Nettoyage de l’environnement Git temporaire impossible.".into());
        }
        captured
    }
    #[cfg(not(windows))]
    pub(super) async fn fetch(
        &self,
        _source: &Repository,
        _origin: &str,
        _reference: &str,
        _private_ref: &str,
    ) -> Result<()> {
        Err("Le fetch HTTPS natif attend sa qualification sur ce système.".into())
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn credential_is_only_in_scoped_process_environment_not_arguments_or_files() {
        let authentication = HttpsAuthentication::new(
            ForgeConfiguration {
                kind: atelier_core::contracts::ForgeKind::Github,
                base: "https://github.example.test".into(),
            },
            "owner/repo".into(),
            "synthetic-private-token".into(),
        )
        .unwrap();
        let home = tempfile::tempdir().unwrap();
        let mut command = tokio::process::Command::new("git");
        command.env_clear();
        authentication
            .configure_command(
                &mut command,
                home.path(),
                "https://github.example.test/owner/repo.git",
                "refs/heads/main",
                &format!("refs/atelier-fetch/{}", uuid::Uuid::new_v4()),
            )
            .unwrap();
        let command = command.as_std();
        let arguments = command
            .get_args()
            .map(|arg| arg.to_string_lossy())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(!arguments.contains("synthetic-private-token"));
        assert!(!arguments.contains(&authentication.authorization));
        assert!(arguments.contains("--no-write-fetch-head"));
        assert!(arguments.contains("http.followRedirects=false"));
        assert!(arguments.contains("credential.helper="));
        let environment = command
            .get_envs()
            .collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(
            environment[std::ffi::OsStr::new("GIT_CONFIG_KEY_0")].unwrap(),
            "http.https://github.example.test/owner/repo.git.extraHeader"
        );
        assert_eq!(
            environment[std::ffi::OsStr::new("GIT_CONFIG_VALUE_0")].unwrap(),
            authentication.authorization.as_str()
        );
        for key in [
            "GIT_TRACE",
            "GIT_TRACE_CURL",
            "GIT_CONFIG_PARAMETERS",
            "GIT_PROXY_COMMAND",
        ] {
            assert!(!environment.contains_key(std::ffi::OsStr::new(key)));
        }
        assert_eq!(std::fs::read_dir(home.path()).unwrap().count(), 0);
    }
}
