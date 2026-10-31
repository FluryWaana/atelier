use super::{
    files::Repository,
    git_transport::{self, HttpsAuthentication},
    repository_access,
    task_creation::path_string,
};
use crate::Result;
use atelier_core::{application::project_clone::CloneRequest, contracts::ForgeKind};
use cap_fs_ext::DirExt;
use cap_std::fs::Dir;
use reqwest::Url;
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

pub fn remote(value: &str) -> Result<Url> {
    if value.len() > 2000 || value.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err("message.clone_invalid_url".into());
    }
    let normalized = if !value.contains("://") {
        let (authority, path) = value.split_once(':').ok_or("message.clone_invalid_url")?;
        if !authority.contains('@') || path.is_empty() {
            return Err("message.clone_invalid_url".into());
        }
        format!("ssh://{authority}/{path}")
    } else {
        value.into()
    };
    let url = Url::parse(&normalized).map_err(|_| "message.clone_invalid_url")?;
    let username_valid = if url.scheme() == "ssh" {
        !url.username().is_empty()
            && !url.username().starts_with('-')
            && url
                .username()
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-' | b'.'))
    } else {
        url.username().is_empty()
    };
    if !matches!(url.scheme(), "https" | "ssh")
        || url.host_str().is_none()
        || url.host_str().is_some_and(|host| host.starts_with('-'))
        || !username_valid
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path().trim_matches('/').is_empty()
        || value.contains('\\')
    {
        return Err("message.clone_invalid_url".into());
    }
    Ok(url)
}

pub fn authentication(
    store: &super::store::Store<'_>,
    input: &CloneRequest,
    protect: &mut impl FnMut(&str, bool) -> Result<String>,
) -> Result<Option<HttpsAuthentication>> {
    let Some(kind) = input.connection else {
        return Ok(None);
    };
    if remote(&input.url)?.scheme() != "https" {
        return Err("message.clone_connection_mismatch".into());
    }
    let revision = super::forge::revision(store)?;
    let (configuration, token) = super::forge::credentials(store, kind, revision, protect)?;
    let url = remote(&input.url)?;
    let base = Url::parse(&configuration.base)?;
    if url.origin() != base.origin() {
        return Err("message.clone_connection_mismatch".into());
    }
    let relative = url
        .path()
        .strip_prefix(base.path().trim_end_matches('/'))
        .and_then(|path| path.strip_prefix('/'))
        .ok_or("message.clone_connection_mismatch")?;
    let repository = relative
        .trim_end_matches('/')
        .strip_suffix(".git")
        .unwrap_or(relative.trim_end_matches('/'));
    let repository = if kind == ForgeKind::Azure {
        repository.replace("/_git/", "/")
    } else {
        repository.into()
    };
    let authentication = HttpsAuthentication::new(configuration, repository, token)?;
    authentication.origin(&input.url)?;
    Ok(Some(authentication))
}

fn null() -> &'static str {
    if cfg!(windows) { "NUL" } else { "/dev/null" }
}

async fn command(home: &Path, directory: &Dir, path: &Path) -> Result<tokio::process::Command> {
    let mut command = git_transport::git_command().await?;
    HttpsAuthentication::isolate(&mut command, home);
    command.current_dir(path);
    #[cfg(unix)]
    {
        use std::os::{fd::AsRawFd, unix::process::CommandExt};
        let descriptor = directory.as_raw_fd();
        // The caller retains the checked directory until the child and its descendants finish.
        unsafe {
            command.as_std_mut().pre_exec(move || {
                if libc::fchdir(descriptor) == 0 {
                    Ok(())
                } else {
                    Err(std::io::Error::last_os_error())
                }
            });
        }
    }
    #[cfg(windows)]
    let _ = directory;
    for option in [
        "core.fsmonitor=false",
        "core.longpaths=true",
        "credential.helper=",
        "credential.interactive=false",
        "protocol.allow=never",
        "protocol.https.allow=always",
        "http.followRedirects=false",
        "http.proxy=",
        "http.sslVerify=true",
        "http.cookieFile=",
        "http.saveCookies=false",
        "http.delegation=none",
        "fetch.uriprotocols=",
        "submodule.recurse=false",
        "fetch.recurseSubmodules=false",
        "gc.auto=0",
        "maintenance.auto=false",
        "pack.threads=1",
        "index.threads=1",
    ] {
        command.args(["-c", option]);
    }
    command.args([
        "-c",
        &format!("core.hooksPath={}", null()),
        "-c",
        &format!("core.askPass={}", null()),
    ]);
    Ok(command)
}

fn configure_ssh(
    command: &mut tokio::process::Command,
    home: &Path,
    user_home: &Path,
) -> Result<()> {
    let known_hosts = path_string(&user_home.join(".ssh/known_hosts")).replace('\\', "/");
    if known_hosts.chars().any(char::is_control) {
        return Err("message.clone_ssh_failed".into());
    }
    let known_hosts = known_hosts.replace('%', "%%").replace('"', "\\\"");
    let config = home.join("ssh-config");
    std::fs::write(
        &config,
        format!(
            "Host *\n  BatchMode yes\n  StrictHostKeyChecking yes\n  UpdateHostKeys no\n  UserKnownHostsFile \"{known_hosts}\"\n  GlobalKnownHostsFile none\n  IdentityFile none\n  CertificateFile none\n  PreferredAuthentications publickey\n  PasswordAuthentication no\n  KbdInteractiveAuthentication no\n  HostbasedAuthentication no\n  ForwardAgent no\n  ClearAllForwardings yes\n  ProxyCommand none\n  ProxyJump none\n  PermitLocalCommand no\n  ControlMaster no\n  ControlPath none\n  ConnectTimeout 15\n  ConnectionAttempts 1\n  RequestTTY no\n"
        ),
    )?;
    // Git interprets GIT_SSH_COMMAND with its shell. Quote only our generated config path.
    let config = path_string(&config)
        .replace('\\', "/")
        .replace('\'', "'\\''");
    command
        .env("GIT_SSH_COMMAND", format!("ssh -F '{config}'"))
        .env("GIT_SSH_VARIANT", "ssh")
        .args(["-c", "protocol.ssh.allow=always"]);
    if let Some(socket) = std::env::var_os("SSH_AUTH_SOCK") {
        command.env("SSH_AUTH_SOCK", socket);
    }
    Ok(())
}

pub struct Destination {
    parent: Dir,
    parent_path: PathBuf,
    stage: String,
    identity: String,
    directory: String,
    #[cfg(test)]
    ssh_fixture: Option<(PathBuf, PathBuf)>,
}

impl Destination {
    pub fn authorize(&self, root: &Dir, path: &str) -> Result<()> {
        let revision = repository_access::read(root)?.revision;
        repository_access::register_checked(root, path, revision, true, Some(&self.identity))?;
        Ok(())
    }
    pub fn reserve(input: &CloneRequest) -> Result<Self> {
        input.validate()?;
        remote(&input.url)?;
        let parent_path = PathBuf::from(&input.parent);
        let parent = repository_access::absolute(&parent_path)?;
        match parent.symlink_metadata(&input.directory) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            _ => return Err("message.clone_destination_exists".into()),
        }
        let stage = format!(".atelier-clone-{}", uuid::Uuid::new_v4());
        parent.create_dir(&stage)?;
        let opened = parent.open_dir_nofollow(&stage)?;
        Ok(Self {
            parent,
            parent_path,
            stage,
            identity: repository_access::identity(&opened)?,
            directory: input.directory.clone(),
            #[cfg(test)]
            ssh_fixture: None,
        })
    }

    fn opened(&self) -> Result<Dir> {
        let directory = self.parent.open_dir_nofollow(&self.stage)?;
        if repository_access::identity(&directory)? != self.identity {
            return Err("message.clone_destination_changed".into());
        }
        Ok(directory)
    }

    pub fn publish(&self) -> Result<String> {
        let _ = self.opened()?;
        let current = repository_access::absolute(&self.parent_path)?;
        if repository_access::identity(&current)? != repository_access::identity(&self.parent)? {
            return Err("message.clone_destination_changed".into());
        }
        super::publish::rename_new(
            &self.parent,
            Path::new(&self.stage),
            Path::new(&self.directory),
        )?;
        Ok(path_string(&self.parent_path.join(&self.directory)))
    }

    pub fn discard(self) -> Result<()> {
        let _ = self.opened()?;
        self.parent.remove_dir_all(&self.stage)?;
        Ok(())
    }

    pub async fn download(
        &self,
        input: &CloneRequest,
        authentication: Option<&HttpsAuthentication>,
        stopped: impl std::future::Future<Output = crate::Error>,
    ) -> Result<()> {
        let opened = self.opened()?;
        let stage = self.parent_path.join(&self.stage);
        let home = tempfile::tempdir()?;
        let mut clone = command(home.path(), &opened, &stage).await?;
        let ssh = remote(&input.url).is_ok_and(|url| url.scheme() == "ssh");
        if ssh {
            if authentication.is_some() || input.connection.is_some() {
                return Err("message.clone_connection_mismatch".into());
            }
            let user_home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
                .ok_or("message.clone_ssh_failed")?;
            configure_ssh(&mut clone, home.path(), Path::new(&user_home))?;
            #[cfg(test)]
            if let Some((executable, source)) = &self.ssh_fixture {
                clone
                    .env(
                        "GIT_SSH_COMMAND",
                        format!(
                            "'{}'",
                            path_string(executable)
                                .replace('\\', "/")
                                .replace('\'', "'\\''")
                        ),
                    )
                    .env("ATELIER_SYNTHETIC_SSH_REPOSITORY", path_string(source));
            }
        }
        let origin = if let Some(authentication) = authentication {
            let origin = authentication.configure_network(&mut clone, home.path(), &input.url)?;
            clone.current_dir(&stage);
            origin
        } else {
            remote(&input.url)?;
            input.url.clone()
        };
        clone.args([
            "clone",
            "--no-checkout",
            "--no-local",
            "--no-recurse-submodules",
            "--template=",
            "--",
            &origin,
            ".",
        ]);
        tokio::pin!(stopped);
        let output = super::process_tree::capture_guarded(&mut clone, &[], Duration::from_secs(120), 1024 * 1024, async {
            tokio::select! { error = &mut stopped => error, error = git_transport::download_guard(stage.clone()) => error }
        }).await.map_err(|_| "message.clone_interrupted")?;
        if !output.status.success() {
            #[cfg(test)]
            if self.ssh_fixture.is_some() {
                eprintln!(
                    "Synthetic SSH fixture: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            return Err(if ssh {
                "message.clone_ssh_failed"
            } else {
                "message.clone_https_failed"
            }
            .into());
        }
        git_transport::check_download(&stage, git_transport::DOWNLOAD_LIMIT)?;
        let repository = Repository {
            path: stage.clone(),
            directory: self.opened()?,
        };
        // Bound the expanded checkout before writing files, including repeated large blobs.
        let tree = repository.git(&["ls-tree", "-r", "-l", "-z", "HEAD"]).await;
        match tree {
            Ok(tree) => validate_tree(&tree)?,
            Err(_) => {
                // Empty repositories have no HEAD commit and need no checkout.
                if !repository
                    .git(&["for-each-ref", "--format=%(refname)"])
                    .await?
                    .is_empty()
                {
                    return Err("message.clone_checkout_failed".into());
                }
                return Ok(());
            }
        }
        let mut checkout = command(home.path(), &opened, &stage).await?;
        checkout.args(["checkout", "--force"]);
        let output = super::process_tree::capture_guarded(
            &mut checkout,
            &[],
            Duration::from_secs(120),
            1024 * 1024,
            &mut stopped,
        )
        .await
        .map_err(|_| "message.clone_interrupted")?;
        if !output.status.success() {
            return Err("message.clone_checkout_failed".into());
        }
        self.opened()?;
        Ok(())
    }
}

fn validate_tree(tree: &[u8]) -> Result<()> {
    let tree = std::str::from_utf8(tree).map_err(|_| "message.clone_checkout_failed")?;
    let mut total = 0u64;
    for (index, entry) in tree
        .split('\0')
        .filter(|entry| !entry.is_empty())
        .enumerate()
    {
        let (metadata, path) = entry
            .split_once('\t')
            .ok_or("message.clone_checkout_failed")?;
        atelier_core::domain::files::validate_path(path, cfg!(windows))?;
        let size = metadata
            .split_whitespace()
            .nth(3)
            .ok_or("message.clone_checkout_failed")?;
        if size != "-" {
            total = total
                .checked_add(size.parse::<u64>()?)
                .ok_or("message.git_download_exceeds_storage_limits")?;
        }
        if total > git_transport::DOWNLOAD_LIMIT || index >= 10_000 || path.split('/').count() > 100
        {
            return Err("message.git_download_exceeds_storage_limits".into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn ssh_clone_uses_the_same_checked_destination_and_checkout_with_a_compiled_double() {
        let fixture = tempfile::tempdir().unwrap();
        let parent = std::fs::canonicalize(fixture.path()).unwrap();
        let source = parent.join("source");
        std::fs::create_dir(&source).unwrap();
        let fixture_source =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/clone-ssh.rs");
        let executable = parent.join(if cfg!(windows) {
            "ssh-fixture.exe"
        } else {
            "ssh-fixture"
        });
        let mut compiler = tokio::process::Command::new("rustc");
        compiler
            .arg("--edition=2024")
            .arg(fixture_source)
            .arg("-o")
            .arg(&executable);
        let compiled = super::super::process_tree::capture(
            &mut compiler,
            &[],
            Duration::from_secs(60),
            1024 * 1024,
        )
        .await
        .unwrap();
        assert!(
            compiled.status.success(),
            "{}",
            String::from_utf8_lossy(&compiled.stderr)
        );
        for args in [
            vec!["init", "-b", "main"],
            vec!["add", "."],
            vec!["commit", "-qm", "Synthetic SSH"],
        ] {
            if args[0] == "add" {
                std::fs::write(source.join("README.md"), "synthetic SSH content").unwrap();
            }
            let opened = repository_access::absolute(Path::new(&path_string(&source))).unwrap();
            let mut git = command(&parent, &opened, &source).await.unwrap();
            git.args([
                "-c",
                "user.name=Synthetic",
                "-c",
                "user.email=synthetic@example.invalid",
            ])
            .args(args);
            assert!(
                super::super::process_tree::capture(
                    &mut git,
                    &[],
                    Duration::from_secs(15),
                    1024 * 1024
                )
                .await
                .unwrap()
                .status
                .success()
            );
        }
        std::fs::write(source.join(".clone-fixture"), "synthetic-only").unwrap();
        let input = CloneRequest {
            url: "git@clone.example.test:team/repo.git".into(),
            parent: path_string(&parent),
            directory: "cloned".into(),
            connection: None,
            operation: "synthetic-ssh".into(),
            confirmed: true,
        };
        let mut destination = Destination::reserve(&input).unwrap();
        destination.ssh_fixture = Some((executable, source));
        destination
            .download(&input, None, std::future::pending())
            .await
            .unwrap();
        let path = destination.publish().unwrap();
        assert_eq!(
            std::fs::read_to_string(Path::new(&path).join("README.md")).unwrap(),
            "synthetic SSH content"
        );
        assert!(
            std::fs::read_to_string(Path::new(&path).join(".git/config"))
                .unwrap()
                .contains(&input.url)
        );
    }
    #[test]
    fn clone_urls_never_accept_credentials_helpers_or_local_transports() {
        remote("https://forge.example.test/team/repo.git").unwrap();
        for url in [
            "git@forge.example.test:team/repo.git",
            "ssh://git@forge.example.test:2222/team/repo.git",
        ] {
            assert_eq!(remote(url).unwrap().scheme(), "ssh");
        }
        for value in [
            "file:///tmp/repo",
            "http://forge.example.test/repo",
            "ext::program",
            "-repo",
            "https://token@forge.example.test/repo",
            "https://forge.example.test/repo?token=secret",
            "https://forge.example.test/repo#token",
            "https://forge.example.test/\nrepo",
            "https://forge.example.test\\repo",
            "ssh://-option@forge.example.test/repo",
            "ssh://git@-option/repo",
            "ssh://git:secret@forge.example.test/repo",
            "ssh://git@forge.example.test/repo?command=bad",
            "git@forge.example.test:",
        ] {
            assert!(remote(value).is_err(), "{value}");
        }
    }
    #[test]
    fn ssh_uses_only_the_agent_and_explicit_known_hosts_without_personal_programs() {
        let home = tempfile::tempdir().unwrap();
        let mut command = tokio::process::Command::new("git");
        command.env_clear();
        configure_ssh(
            &mut command,
            home.path(),
            Path::new("/synthetic/user space"),
        )
        .unwrap();
        let config = std::fs::read_to_string(home.path().join("ssh-config")).unwrap();
        for option in [
            "BatchMode yes",
            "StrictHostKeyChecking yes",
            "IdentityFile none",
            "CertificateFile none",
            "ProxyCommand none",
            "ProxyJump none",
            "PermitLocalCommand no",
            "ForwardAgent no",
            "UpdateHostKeys no",
            "UserKnownHostsFile \"/synthetic/user space/.ssh/known_hosts\"",
        ] {
            assert!(config.contains(option), "{option}");
        }
        let environment = command
            .as_std()
            .get_envs()
            .collect::<std::collections::BTreeMap<_, _>>();
        let ssh = environment[std::ffi::OsStr::new("GIT_SSH_COMMAND")]
            .unwrap()
            .to_string_lossy();
        assert!(ssh.starts_with("ssh -F '"));
        assert!(!ssh.contains("forge.example.test"));
    }
    #[test]
    fn checkout_limits_count_each_path_even_when_objects_are_shared() {
        validate_tree(b"100644 blob abc 20\tREADME.md\0").unwrap();
        assert!(validate_tree(b"100644 blob abc 536870913\tlarge\0").is_err());
        assert!(validate_tree(b"100644 blob abc 10\t../outside\0").is_err());
    }
}
