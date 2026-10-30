use super::{files::Repository, forge, forge_association};
use crate::Result;
use atelier_core::contracts::ForgeConfiguration;
#[cfg(windows)]
use atelier_core::document::Document;
use base64::{Engine, engine::general_purpose::STANDARD};
use reqwest::Url;
fn null() -> &'static str {
    if cfg!(windows) { "NUL" } else { "/dev/null" }
}

#[cfg(unix)]
pub(super) async fn git_command() -> Result<tokio::process::Command> {
    let mut command = tokio::process::Command::new("git");
    command.env_clear().env(
        "PATH",
        std::env::var_os("PATH").unwrap_or_else(|| "/usr/bin:/bin".into()),
    );
    Ok(command)
}
const DOWNLOAD_LIMIT: u64 = 512 * 1024 * 1024;
fn synthetic_git_failure(stderr: &[u8]) -> &'static str {
    let diagnostic = String::from_utf8_lossy(stderr).to_ascii_lowercase();
    // Return fixed classifications only: Git stderr can contain URLs and credentials.
    if diagnostic.contains("unsupported ssl backend") {
        "Synthetic Git TLS: unsupported SSL backend."
    } else if diagnostic.contains("error setting certificate")
        || diagnostic.contains("could not load pem")
        || diagnostic.contains("couldn't load pem")
        || diagnostic.contains("bad ca certificate")
    {
        "Synthetic Git TLS: explicit CA file or directory could not be loaded."
    } else if diagnostic.contains("subject name")
        || diagnostic.contains("subject alternative")
        || diagnostic.contains("hostname mismatch")
        || diagnostic.contains("no alternative certificate")
    {
        "Synthetic Git TLS: certificate hostname or IP did not match."
    } else if diagnostic.contains("expired") || diagnostic.contains("not yet valid") {
        "Synthetic Git TLS: certificate validity dates were refused."
    } else if diagnostic.contains("self-signed")
        || diagnostic.contains("unable to get local issuer")
        || diagnostic.contains("unable to verify the first")
        || diagnostic.contains("unable to verify first")
    {
        "Synthetic Git TLS: certificate issuer was not anchored by the explicit CA."
    } else if diagnostic.contains("sectrust") {
        "Synthetic Git TLS: Apple native trust rejected the certificate."
    } else if diagnostic.contains("certificate")
        || diagnostic.contains("issuer")
        || diagnostic.contains("ssl peer")
    {
        "Synthetic Git TLS: certificate or authority verification failed."
    } else if diagnostic.contains("ssl") || diagnostic.contains("tls") {
        "Synthetic Git TLS: TLS connection failed."
    } else if diagnostic.contains("401") || diagnostic.contains("403") {
        "Synthetic Git TLS: authentication was refused."
    } else if diagnostic.contains("404") || diagnostic.contains("repository not found") {
        "Synthetic Git TLS: repository or HTTP route was not found."
    } else if diagnostic.contains("500") || diagnostic.contains("502") {
        "Synthetic Git TLS: fixture HTTP backend failed."
    } else if diagnostic.contains("not a git repository")
        || diagnostic.contains("invalid")
        || diagnostic.contains("bad line")
        || diagnostic.contains("protocol")
    {
        "Synthetic Git TLS: Git protocol response was invalid."
    } else if diagnostic.contains("connect") || diagnostic.contains("resolve") {
        "Synthetic Git TLS: loopback connection failed."
    } else if diagnostic.contains("remote-https") || diagnostic.contains("not a git command") {
        "Synthetic Git TLS: HTTPS transport executable was unavailable."
    } else {
        "Synthetic Git TLS: unclassified Git failure; raw diagnostic withheld."
    }
}
fn check_download(path: &std::path::Path, limit: u64) -> Result<()> {
    let mut pending = vec![(path.to_owned(), 0usize)];
    let mut bytes = 0u64;
    let mut entries = 0usize;
    while let Some((directory, depth)) = pending.pop() {
        for entry in std::fs::read_dir(directory)? {
            let entry = entry?;
            entries += 1;
            if entries > 10_000 || depth > 16 {
                return Err("message.git_download_exceeds_storage_limits".into());
            }
            // Git may rename a temporary object while this read-only inspection is running.
            let metadata = super::checked_file::plain_metadata(&entry.path())?;
            if metadata.is_dir() {
                pending.push((entry.path(), depth + 1));
            } else if metadata.is_file() {
                bytes = bytes
                    .checked_add(metadata.len())
                    .ok_or("message.git_download_exceeds_storage_limits")?;
                if bytes > limit {
                    return Err("message.git_download_exceeds_storage_limits".into());
                }
            } else {
                return Err("message.unexpected_file_in_the_git_download".into());
            }
        }
    }
    Ok(())
}
async fn download_guard(path: std::path::PathBuf) -> crate::Error {
    // shortcut: polling can overshoot the temporary-storage threshold; use OS quotas for a hard disk cap.
    loop {
        let directory = path.clone();
        let check =
            tokio::task::spawn_blocking(move || check_download(&directory, DOWNLOAD_LIMIT)).await;
        match check {
            Ok(Ok(())) => {}
            Ok(Err(error))
                if error
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) => {}
            _ => {
                return "message.git_download_stopped_storage_limit_exceeded_or_inspection_unavai"
                    .into();
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
}

pub struct HttpsAuthentication {
    configuration: ForgeConfiguration,
    repository: String,
    synthetic: bool,
    authorization: String,
    synthetic_authority: Option<Vec<u8>>,
}
pub struct PreparedPush {
    home: tempfile::TempDir,
    origin: String,
    reference: String,
    commit: String,
    previous: Option<String>,
}

impl HttpsAuthentication {
    /// Explicit loopback TLS fixture composition; production routes never select this adapter.
    pub fn synthetic_tls(
        configuration: ForgeConfiguration,
        repository: String,
        token: String,
        authority: &[u8],
    ) -> Result<Self> {
        // Reuse the strict HTTPS/IP/token/explicit-authority boundary of the HTTP adapters.
        let _ = super::synthetic_tls::client(&configuration.base, &token, authority)?;
        let mut authentication = Self::prepare(configuration, repository, token, false)?;
        authentication.synthetic_authority = Some(authority.to_vec());
        Ok(authentication)
    }
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
            return Err("message.a_synthetic_loopback_git_transport_is_required".into());
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
            || configuration.base.contains(&token)
            || repository.contains(&token)
            || token.chars().any(|c| c.is_control() || c.is_whitespace())
        {
            return Err("message.invalid_git_https_token".into());
        }
        Ok(Self {
            configuration,
            repository,
            synthetic,
            synthetic_authority: None,
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
            return Err("message.invalid_https_origin".into());
        }
        let mut requested = Url::parse(origin).map_err(|_| "message.invalid_https_origin")?;
        let mut configuration = self.configuration.clone();
        if self.synthetic {
            if requested.scheme() != "http" {
                return Err("message.a_loopback_http_test_origin_is_required".into());
            }
            let mut base = Url::parse(&configuration.base)?;
            base.set_scheme("https")
                .map_err(|_| "message.invalid_test_origin")?;
            configuration.base = base.into();
            requested
                .set_scheme("https")
                .map_err(|_| "message.invalid_test_origin")?;
        }
        if requested.scheme() != "https"
            || !forge_association::matches_origin(
                &configuration,
                &self.repository,
                requested.as_str(),
            )?
        {
            return Err(
                "message.the_https_origin_does_not_match_the_selected_forge_connection_an".into(),
            );
        }
        let mut url = Url::parse(origin)?;
        url.set_username("")
            .map_err(|_| "message.invalid_https_origin")?;
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
                return Err(
                    "message.authenticated_git_transport_refuses_local_transport_or_credentia"
                        .into(),
                );
            }
        }
        Ok(())
    }
    fn isolate(command: &mut tokio::process::Command, home: &std::path::Path) {
        command
            .current_dir(home)
            .env("HOME", home)
            .env("USERPROFILE", home)
            .env("APPDATA", home)
            .env("LOCALAPPDATA", home)
            .env("XDG_CONFIG_HOME", home)
            .env("GIT_CONFIG_GLOBAL", null())
            .env("GIT_CONFIG_SYSTEM", null())
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_ASKPASS", null())
            .env_remove("SSH_ASKPASS")
            .env("GCM_INTERACTIVE", "Never")
            .env("GIT_CONFIG_COUNT", "0");
    }
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
            return Err("message.invalid_remote_git_reference".into());
        }
        if !private_ref.starts_with("refs/atelier-fetch/")
            || private_ref
                .strip_prefix("refs/atelier-fetch/")
                .and_then(|value| uuid::Uuid::parse_str(value).ok())
                .is_none()
        {
            return Err("message.invalid_temporary_git_reference".into());
        }
        let scope = self.configure_network(command, home, origin)?;
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
    fn configure_network(
        &self,
        command: &mut tokio::process::Command,
        home: &std::path::Path,
        origin: &str,
    ) -> Result<String> {
        let scope = self.origin(origin)?;
        Self::isolate(command, home);
        if let Some(authority) = &self.synthetic_authority {
            // Only the public CA is written; the token remains solely in the scoped environment.
            let path = home.join("synthetic-authority.pem");
            std::fs::write(&path, authority)?;
            let empty = home.join("synthetic-empty-authorities");
            std::fs::create_dir_all(&empty)?;
            command.arg("-c").arg(format!(
                "http.{scope}.sslCAInfo={}",
                path.to_str()
                    .ok_or("message.synthetic_authority_path_invalid")?
            ));
            command.arg("-c").arg(format!(
                "http.{scope}.sslCAPath={}",
                empty
                    .to_str()
                    .ok_or("message.synthetic_authority_path_invalid")?
            ));
            // Ubuntu Git uses GnuTLS, which consumes the explicit CA bundle and
            // empty CA directory above. Its curl cannot select OpenSSL. Windows
            // and macOS qualification instead provision an OpenSSL backend.
            #[cfg(not(target_os = "linux"))]
            command.args(["-c", "http.sslBackend=openssl"]);
            command.args(["-c", "http.schannelUseSSLCAInfo=true"]);
        }
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
            command.args(["-c", &option.replace("NUL", null())]);
        }
        if self.synthetic {
            command.args(["-c", "protocol.http.allow=always"]);
        }
        Ok(scope)
    }
    pub async fn remote_head(&self, origin: &str, reference: &str) -> Result<String> {
        self.remote_head_optional(origin, reference)
            .await?
            .ok_or_else(|| "message.remote_git_branch_absent".into())
    }
    pub async fn remote_head_optional(
        &self,
        origin: &str,
        reference: &str,
    ) -> Result<Option<String>> {
        atelier_core::application::worktrees::validate_reference(reference)?;
        if !reference.starts_with("refs/heads/") || reference.contains(':') {
            return Err("message.invalid_remote_git_branch".into());
        }
        let home = tempfile::Builder::new()
            .prefix("atelier-git-read-")
            .tempdir()?;
        let result = async {
            let mut command = git_command().await?;
            let scope = self.configure_network(&mut command, home.path(), origin)?;
            command.args([
                "ls-remote",
                "--refs",
                "--exit-code",
                "--",
                &scope,
                reference,
            ]);
            let output = super::process_tree::capture(
                &mut command,
                &[],
                std::time::Duration::from_secs(30),
                1024 * 1024,
            )
            .await
            .map_err(|_| "message.https_branch_read_interrupted")?;
            if output.status.code() == Some(2) {
                if !output.stdout.is_empty() {
                    return Err("message.ambiguous_git_branch_response".into());
                }
                return Ok(None);
            }
            if !output.status.success() {
                return Err(if self.synthetic_authority.is_some() {
                    synthetic_git_failure(&output.stderr)
                } else {
                    "message.https_branch_missing_or_inaccessible_no_authentication_details_a"
                }
                .into());
            }
            let text = String::from_utf8(output.stdout)?;
            let mut lines = text.lines();
            let (commit, actual) = lines
                .next()
                .and_then(|line| line.split_once('\t'))
                .ok_or("message.invalid_git_branch_response")?;
            validate_commit(commit)?;
            if actual != reference || lines.next().is_some() {
                return Err("message.ambiguous_git_branch_response".into());
            }
            Ok(Some(commit.to_owned()))
        }
        .await;
        home.close()
            .map_err(|_| "message.unable_to_clean_up_the_temporary_git_environment")?;
        result
    }
    pub async fn prepare_push(
        &self,
        source: &Repository,
        origin: &str,
        reference: &str,
        commit: &str,
        previous: Option<&str>,
    ) -> Result<PreparedPush> {
        source.verify_root().await?;
        validate_commit(commit)?;
        if let Some(previous) = previous {
            validate_commit(previous)?;
        }
        if previous.is_some_and(|previous| commit == previous || commit.len() != previous.len()) {
            return Err("message.no_compatible_new_commit_to_push".into());
        }
        source.git(&["check-ref-format", reference]).await?;
        if !reference.starts_with("refs/heads/") {
            return Err("message.invalid_remote_git_branch".into());
        }
        let keys = String::from_utf8(
            source
                .git(&[
                    "config",
                    "--local",
                    "--no-includes",
                    "--null",
                    "--name-only",
                    "--list",
                ])
                .await?,
        )?;
        Self::validate_configuration(&keys)?;
        let origin = self.origin(origin)?;
        let home = tempfile::Builder::new()
            .prefix("atelier-git-push-")
            .tempdir()?;
        let prepared = async {
            let format =
                String::from_utf8(source.git(&["rev-parse", "--show-object-format"]).await?)?;
            let format = format.trim();
            if !["sha1", "sha256"].contains(&format) {
                return Err("message.unsupported_git_object_format".into());
            }
            let mut initialize = git_command().await?;
            Self::isolate(&mut initialize, home.path());
            initialize.args([
                "init",
                "--bare",
                "--quiet",
                "--template=",
                &format!("--object-format={format}"),
            ]);
            let output = super::process_tree::capture(
                &mut initialize,
                &[],
                std::time::Duration::from_secs(15),
                1024 * 1024,
            )
            .await
            .map_err(|_| "message.unable_to_prepare_the_isolated_push")?;
            if !output.status.success() {
                return Err("message.unable_to_prepare_the_isolated_push".into());
            }
            let source_path = super::task_creation::path_string(&source.path);
            let mut import = git_command().await?;
            Self::isolate(&mut import, home.path());
            import.args([
                "--no-lazy-fetch",
                "--no-replace-objects",
                "-c",
                &format!("core.hooksPath={}", null()),
                "-c",
                "protocol.allow=never",
                "-c",
                "protocol.file.allow=always",
                "-c",
                "gc.auto=0",
                "-c",
                "pack.threads=1",
                "fetch",
                "--no-tags",
                "--no-write-fetch-head",
                "--no-recurse-submodules",
                "--no-auto-maintenance",
                "--",
            ]);
            // Build the path as an argument; no shell interpolation or remote source transport is used.
            import
                .arg(source_path)
                .arg(format!("{commit}:refs/atelier-push/commit"));
            let output = super::process_tree::capture_guarded(
                &mut import,
                &[],
                std::time::Duration::from_secs(120),
                4 * 1024 * 1024,
                download_guard(home.path().to_owned()),
            )
            .await
            .map_err(|_| "message.commit_import_for_the_push_interrupted_or_too_large")?;
            if !output.status.success() {
                return Err("message.unable_to_import_commits_for_the_push".into());
            }
            let imported = home.path().to_owned();
            tokio::task::spawn_blocking(move || check_download(&imported, DOWNLOAD_LIMIT))
                .await??;
            if let Some(previous) = previous {
                let mut ancestry = git_command().await?;
                Self::isolate(&mut ancestry, home.path());
                ancestry.args([
                    "--no-replace-objects",
                    "merge-base",
                    "--is-ancestor",
                    previous,
                    commit,
                ]);
                let output = super::process_tree::capture(
                    &mut ancestry,
                    &[],
                    std::time::Duration::from_secs(15),
                    1024 * 1024,
                )
                .await
                .map_err(|_| "message.unable_to_inspect_git_history")?;
                if !output.status.success() {
                    return Err(
                        "message.push_refused_the_remote_branch_must_not_lose_any_commit_fetch_an"
                            .into(),
                    );
                }
            }
            if self
                .remote_head_optional(&origin, reference)
                .await?
                .as_deref()
                != previous
            {
                return Err("message.the_remote_branch_changed_reload_the_push_preview".into());
            }
            Ok::<(), crate::Error>(())
        }
        .await;
        if let Err(error) = prepared {
            home.close()
                .map_err(|_| "message.unable_to_clean_up_the_temporary_git_environment")?;
            return Err(error);
        }
        Ok(PreparedPush {
            home,
            origin,
            reference: reference.into(),
            commit: commit.into(),
            previous: previous.map(str::to_owned),
        })
    }
    pub async fn push(&self, prepared: PreparedPush) -> Result<()> {
        let PreparedPush {
            home,
            origin,
            reference,
            commit,
            previous,
        } = prepared;
        let result = async {
            let mut command = git_command().await?;
            let scope = self.configure_network(&mut command, home.path(), &origin)?;
            command.args([
                "push",
                "--porcelain",
                "--no-verify",
                "--recurse-submodules=no",
                &format!(
                    "--force-with-lease={reference}:{}",
                    previous.as_deref().unwrap_or("")
                ),
                "--",
                &scope,
                &format!("{commit}:{reference}"),
            ]);
            let output = super::process_tree::capture_guarded(
                &mut command,
                &[],
                std::time::Duration::from_secs(120),
                4 * 1024 * 1024,
                download_guard(home.path().to_owned()),
            )
            .await
            .map_err(|_| "message.https_push_uncertain_read_the_remote_branch_before_any_retry")?;
            if !output.status.success() {
                return Err(
                    "message.https_push_refused_or_uncertain_read_the_remote_branch_before_an"
                        .into(),
                );
            }
            if self.remote_head(&origin, &reference).await? != commit {
                return Err(
                    "message.https_push_uncertain_read_the_remote_branch_before_any_retry".into(),
                );
            }
            Ok(())
        }
        .await;
        home.close()
            .map_err(|_| "message.unable_to_clean_up_the_temporary_git_environment")?;
        result
    }
    pub(super) async fn fetch(
        &self,
        source: &Repository,
        origin: &str,
        reference: &str,
        private_ref: &str,
    ) -> Result<()> {
        let home = tempfile::Builder::new()
            .prefix("atelier-git-home-")
            .tempdir()?;
        // Network Git sees only this private bare repository, never source transport configuration.
        let captured = async {
            let format =
                String::from_utf8(source.git(&["rev-parse", "--show-object-format"]).await?)
                    .map_err(|_| "message.invalid_git_object_format")?;
            let format = format.trim();
            if !["sha1", "sha256"].contains(&format) {
                return Err("message.unsupported_git_object_format".into());
            }
            let mut initialize = git_command().await?;
            Self::isolate(&mut initialize, home.path());
            initialize.args([
                "init",
                "--bare",
                "--quiet",
                "--template=",
                &format!("--object-format={format}"),
            ]);
            let initialized = super::process_tree::capture(
                &mut initialize,
                &[],
                std::time::Duration::from_secs(15),
                1024 * 1024,
            )
            .await
            .map_err(|_| "message.unable_to_prepare_the_isolated_git_fetch")?;
            if !initialized.status.success() {
                return Err("message.unable_to_prepare_the_isolated_git_fetch".into());
            }
            let mut command = git_command().await?;
            self.configure_command(&mut command, home.path(), origin, reference, private_ref)?;
            let output = super::process_tree::capture_guarded(
                &mut command,
                &[],
                std::time::Duration::from_secs(120),
                4 * 1024 * 1024,
                download_guard(home.path().to_owned()),
            )
            .await
            .map_err(
                |_| "message.https_fetch_was_interrupted_or_exceeded_time_or_size_limits_no_a",
            )?;
            if !output.status.success() {
                return Err(
                    "message.https_fetch_was_refused_check_the_exact_origin_url_token_and_rep"
                        .into(),
                );
            }
            let downloaded = home.path().to_owned();
            tokio::task::spawn_blocking(move || check_download(&downloaded, DOWNLOAD_LIMIT))
                .await?
                .map_err(
                    |_| "message.git_download_exceeds_storage_limits_or_cannot_be_inspected",
                )?;
            let path = super::task_creation::path_string(home.path());
            source
                .without_programs(&[
                    "-c",
                    "protocol.allow=never",
                    "-c",
                    "protocol.file.allow=always",
                    "-c",
                    "pack.threads=1",
                    "-c",
                    "gc.auto=0",
                    "fetch",
                    "--no-tags",
                    "--no-write-fetch-head",
                    "--no-recurse-submodules",
                    "--no-auto-maintenance",
                    "--no-prune",
                    "--refmap=",
                    "--",
                    &path,
                    &format!("{private_ref}:{private_ref}"),
                ])
                .await?;
            Ok::<(), crate::Error>(())
        }
        .await;
        if home.close().is_err() {
            return Err("message.unable_to_clean_up_the_temporary_git_environment".into());
        }
        captured
    }
}
#[cfg(windows)]
pub(super) async fn git_command() -> Result<tokio::process::Command> {
    let mut profile = Document::default();
    profile.set("type", &"command")?;
    profile.set("command", &"git")?;
    super::agent_diagnostics::command(&profile).await
}
fn validate_commit(value: &str) -> Result<()> {
    if ![40, 64].contains(&value.len()) || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("message.invalid_git_commit".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn synthetic_git_failure_classification_never_returns_raw_credentials_or_urls() {
        for diagnostic in [
            "fatal: Unsupported SSL backend with synthetic-private-token",
            "SSL certificate problem at https://synthetic-private-token@127.0.0.1/repo",
            "SSL: no alternative certificate subject name matches 127.0.0.1 synthetic-private-token",
            "SSL certificate problem: unable to get local issuer certificate synthetic-private-token",
            "error setting certificate file: /private/synthetic-private-token",
            "certificate has expired synthetic-private-token",
            "HTTP 403 synthetic-private-token",
            "unexpected synthetic-private-token at https://127.0.0.1/private",
        ] {
            let message = synthetic_git_failure(diagnostic.as_bytes());
            assert!(!message.contains("synthetic-private-token"));
            assert!(!message.contains("127.0.0.1"));
            assert!(!message.contains("https://"));
        }
    }

    #[test]
    fn synthetic_tls_keeps_explicit_ca_trust_without_selecting_an_unavailable_linux_backend() {
        let mut authentication = HttpsAuthentication::new(
            ForgeConfiguration {
                kind: atelier_core::contracts::ForgeKind::Github,
                base: "https://github.example.test".into(),
            },
            "owner/repo".into(),
            "synthetic-private-token".into(),
        )
        .unwrap();
        authentication.synthetic_authority = Some(b"synthetic-public-ca".to_vec());
        let home = tempfile::tempdir().unwrap();
        let mut command = tokio::process::Command::new("git");
        authentication
            .configure_network(
                &mut command,
                home.path(),
                "https://github.example.test/owner/repo.git",
            )
            .unwrap();
        let arguments = command
            .as_std()
            .get_args()
            .map(|arg| arg.to_string_lossy())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(arguments.contains(".sslCAInfo="));
        assert!(arguments.contains(".sslCAPath="));
        assert!(arguments.contains("http.sslVerify=true"));
        assert_eq!(
            arguments.contains("http.sslBackend=openssl"),
            !cfg!(target_os = "linux")
        );
        assert_eq!(
            std::fs::read(home.path().join("synthetic-authority.pem")).unwrap(),
            b"synthetic-public-ca"
        );
        assert_eq!(
            std::fs::read_dir(home.path().join("synthetic-empty-authorities"))
                .unwrap()
                .count(),
            0
        );
        assert!(!arguments.contains("synthetic-private-token"));
    }

    #[test]
    fn download_limits_include_all_objects_and_prevent_import_of_excessive_storage() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::create_dir(directory.path().join("objects")).unwrap();
        std::fs::write(directory.path().join("objects/one"), [1u8; 8]).unwrap();
        std::fs::write(directory.path().join("objects/two"), [2u8; 8]).unwrap();
        check_download(directory.path(), 16).unwrap();
        assert!(check_download(directory.path(), 15).is_err());
        let mut nested = directory.path().to_owned();
        for _ in 0..18 {
            nested.push("nested");
            std::fs::create_dir(&nested).unwrap();
        }
        std::fs::write(nested.join("leaf"), [0]).unwrap();
        assert!(check_download(directory.path(), DOWNLOAD_LIMIT).is_err());
    }

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
