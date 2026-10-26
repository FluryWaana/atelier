//! Private-index snapshots. The durable HTTP workflow must reserve before calling publish.
use super::{
    checked_file::require_plain,
    files::{Repository, validate_commit},
    process_tree,
};
use crate::Result;
use atelier_core::domain::files::validate_path;
use cap_fs_ext::DirExt;
use std::{path::Path, time::Duration};

pub use atelier_core::contracts::LocalCommitSnapshot as Snapshot;

pub struct Preview {
    pub snapshot: Snapshot,
    pub patch: String,
}

fn paths(paths: &[String]) -> Result<()> {
    if paths.is_empty()
        || paths.len() > 100
        || paths.iter().map(String::len).sum::<usize>() > 16_384
    {
        return Err("Choisis entre 1 et 100 fichiers, dans la limite de 16 Kio de chemins.".into());
    }
    let mut seen = std::collections::BTreeSet::new();
    for path in paths {
        validate_path(path, cfg!(windows))?;
        if !seen.insert(path) || path.chars().any(char::is_control) {
            return Err("Liste de fichiers invalide ou répétée.".into());
        }
    }
    Ok(())
}

pub fn identity(name: &str, email: &str, message: &str) -> Result<()> {
    if name.trim().is_empty()
        || name.trim() != name
        || name.len() > 255
        || name.contains(['<', '>'])
        || name.chars().any(char::is_control)
        || email.len() > 254
        || email.split('@').count() != 2
        || email.starts_with('@')
        || email.ends_with('@')
        || email
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || matches!(c, '<' | '>'))
        || message.trim().is_empty()
        || message.encode_utf16().count() > 2000
        || message
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
    {
        return Err(
            "Renseigne un auteur, une adresse e-mail et un message de commit valides.".into(),
        );
    }
    Ok(())
}

async fn command(repository: &Repository) -> Result<tokio::process::Command> {
    let mut command = super::git_transport::git_command().await?;
    let null = if cfg!(windows) { "NUL" } else { "/dev/null" };
    #[cfg(unix)]
    {
        use std::os::{fd::AsRawFd, unix::process::CommandExt};
        let descriptor = repository.directory.as_raw_fd();
        // The caller retains repository through capture; fchdir is async-signal-safe after fork.
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
    command
        .current_dir(&repository.path)
        .env("GIT_CONFIG_GLOBAL", null)
        .env("GIT_CONFIG_SYSTEM", null)
        .env("GIT_TERMINAL_PROMPT", "0")
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
        "core.excludesFile=NUL",
        "commit.gpgSign=false",
        "tag.gpgSign=false",
        "protocol.allow=never",
        "gc.auto=0",
        "maintenance.auto=false",
        "index.threads=1",
        "core.splitIndex=false",
        "core.sparseCheckout=false",
        "index.sparse=false",
        "pack.threads=1",
    ] {
        command.args(["-c", &option.replace("NUL", null)]);
    }
    command.args(repository.program_options().await?);
    Ok(command)
}

async fn run(
    repository: &Repository,
    index: &Path,
    args: &[&str],
    input: &[u8],
) -> Result<Vec<u8>> {
    let mut command = command(repository).await?;
    command.env("GIT_INDEX_FILE", index).args(args);
    let output = process_tree::capture(
        &mut command,
        input,
        Duration::from_secs(30),
        4 * 1024 * 1024,
    )
    .await?;
    if !output.status.success() {
        return Err(
            "Opération de commit Git refusée. Les fichiers et l’index restent conservés.".into(),
        );
    }
    Ok(output.stdout)
}

fn oid(bytes: Vec<u8>) -> Result<String> {
    let value = String::from_utf8(bytes)?.trim().to_owned();
    validate_commit(&value)?;
    Ok(value)
}

pub async fn current(repository: &Repository, branch: &str) -> Result<String> {
    repository.verify_root().await?;
    repository
        .git(&["check-ref-format", &format!("refs/heads/{branch}")])
        .await?;
    oid(repository
        .git(&[
            "rev-parse",
            "--verify",
            &format!("refs/heads/{branch}^{{commit}}"),
        ])
        .await?)
}

async fn branch(repository: &Repository) -> Result<(String, String)> {
    repository.verify_root().await?;
    let branch = String::from_utf8(repository.git(&["symbolic-ref", "--quiet", "HEAD"]).await?)?
        .trim()
        .strip_prefix("refs/heads/")
        .ok_or("Le commit exige une branche locale attachée.")?
        .to_owned();
    for name in [
        "MERGE_HEAD",
        "CHERRY_PICK_HEAD",
        "REVERT_HEAD",
        "rebase-merge",
        "rebase-apply",
        "sequencer",
    ] {
        let path = String::from_utf8(repository.git(&["rev-parse", "--git-path", name]).await?)?;
        if repository.path.join(path.trim()).try_exists()? {
            return Err("Termine l’opération Git en cours avant de créer un commit.".into());
        }
    }
    if !repository
        .git(&["ls-files", "--unmerged", "-z"])
        .await?
        .is_empty()
    {
        return Err("Résous les conflits Git avant de créer un commit.".into());
    }
    let head = current(repository, &branch).await?;
    Ok((branch, head))
}

/// Reads only regular selected files and their anchored parents, with a bounded total.
fn check_paths(repository: &Repository, paths: &[String]) -> Result<()> {
    let mut total = 0u64;
    for path in paths {
        let mut directory = repository.directory.try_clone()?;
        let parts = path.split('/').collect::<Vec<_>>();
        for (i, part) in parts.iter().enumerate() {
            let metadata = match directory.symlink_metadata(part) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
                Err(error) => return Err(error.into()),
            };
            require_plain(&metadata)?;
            if i + 1 == parts.len() {
                if !metadata.is_file() || metadata.len() > 16 * 1024 * 1024 {
                    return Err("Sélectionne des fichiers ordinaires de 16 Mio maximum.".into());
                }
                total = total
                    .checked_add(metadata.len())
                    .ok_or("Sélection trop volumineuse.")?;
                if total > 64 * 1024 * 1024 {
                    return Err("La sélection dépasse 64 Mio.".into());
                }
            } else {
                directory = directory.open_dir_nofollow(part)?;
            }
        }
    }
    Ok(())
}

pub async fn preview(repository: &Repository, selected: Vec<String>) -> Result<Preview> {
    paths(&selected)?;
    let (branch_name, head) = branch(repository).await?;
    let changes = repository.changes(None).await?;
    if selected
        .iter()
        .any(|path| !changes.files.iter().any(|entry| &entry.path == path))
    {
        return Err("La sélection ne correspond plus aux fichiers modifiés.".into());
    }
    check_paths(repository, &selected)?;
    let home = tempfile::tempdir()?;
    let index = home.path().join("index");
    run(repository, &index, &["read-tree", &head], &[]).await?;
    let mut args = vec!["add", "--all", "--"];
    args.extend(selected.iter().map(String::as_str));
    run(repository, &index, &args, &[]).await?;
    check_paths(repository, &selected)?;
    let tree = oid(run(repository, &index, &["write-tree"], &[]).await?)?;
    let patch = String::from_utf8(
        run(
            repository,
            &index,
            &[
                "diff",
                "--no-ext-diff",
                "--no-textconv",
                "--no-renames",
                "--no-color",
                "--ignore-submodules=dirty",
                &head,
                &tree,
                "--",
            ],
            &[],
        )
        .await?,
    )?;
    if patch.is_empty() {
        return Err("La sélection ne produit aucun changement à commiter.".into());
    }
    if branch(repository).await? != (branch_name.clone(), head.clone()) {
        return Err("La branche a changé pendant l’aperçu du commit.".into());
    }
    home.close()?;
    Ok(Preview {
        snapshot: Snapshot {
            branch: branch_name,
            head,
            tree,
            paths: selected,
        },
        patch,
    })
}

/// Compare the files agents can read with the reviewed commit without consuming a human index.
pub async fn disk_matches_commit(repository: &Repository, source: &str) -> Result<bool> {
    validate_commit(source)?;
    let before = repository.fingerprint().await?;
    let (branch_name, head) = branch(repository).await?;
    if head != source {
        return Ok(false);
    }
    let selected = repository
        .changes(None)
        .await?
        .files
        .into_iter()
        .map(|entry| entry.path)
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let same = if selected.is_empty() {
        true
    } else {
        paths(&selected)?;
        check_paths(repository, &selected)?;
        let home = tempfile::tempdir()?;
        let index = home.path().join("index");
        run(repository, &index, &["read-tree", source], &[]).await?;
        let mut arguments = vec!["add", "--all", "--"];
        arguments.extend(selected.iter().map(String::as_str));
        run(repository, &index, &arguments, &[]).await?;
        check_paths(repository, &selected)?;
        let actual = oid(run(repository, &index, &["write-tree"], &[]).await?)?;
        let expected = oid(repository
            .git(&["rev-parse", "--verify", &format!("{source}^{{tree}}")])
            .await?)?;
        home.close()?;
        actual == expected
    };
    if branch(repository).await? != (branch_name, head) || repository.fingerprint().await? != before
    {
        return Err(
            "Les sources ont changé pendant leur comparaison au commit. Relance la review.".into(),
        );
    }
    Ok(same)
}

/// Creates an unreachable object only. Callers persist its ID before publishing the ref.
pub async fn prepare(
    repository: &Repository,
    expected: &Snapshot,
    name: &str,
    email: &str,
    message: &str,
) -> Result<String> {
    identity(name, email, message)?;
    validate_commit(&expected.head)?;
    validate_commit(&expected.tree)?;
    if preview(repository, expected.paths.clone()).await?.snapshot != *expected {
        return Err("Les fichiers ou la branche ont changé. Prépare un nouvel aperçu.".into());
    }
    let mut command = command(repository).await?;
    command
        .env("GIT_AUTHOR_NAME", name)
        .env("GIT_COMMITTER_NAME", name)
        .env("GIT_AUTHOR_EMAIL", email)
        .env("GIT_COMMITTER_EMAIL", email)
        .args(["commit-tree", &expected.tree, "-p", &expected.head]);
    let output = process_tree::capture(
        &mut command,
        message.as_bytes(),
        Duration::from_secs(30),
        4 * 1024 * 1024,
    )
    .await?;
    if !output.status.success() {
        return Err("Création de l’objet commit refusée.".into());
    }
    oid(output.stdout)
}

/// Exactly one CAS. Any error after dispatch must remain uncertain until an explicit read.
pub async fn publish(repository: &Repository, expected: &Snapshot, commit: &str) -> Result<()> {
    validate_commit(commit)?;
    validate_commit(&expected.head)?;
    if branch(repository).await? != (expected.branch.clone(), expected.head.clone()) {
        return Err("La branche a changé avant le commit.".into());
    }
    let tree = oid(repository
        .git(&["rev-parse", "--verify", &format!("{commit}^{{tree}}")])
        .await?)?;
    let parents = String::from_utf8(
        repository
            .git(&["rev-list", "--parents", "--no-walk", commit])
            .await?,
    )?;
    if tree != expected.tree
        || parents.split_whitespace().collect::<Vec<_>>() != [commit, expected.head.as_str()]
    {
        return Err("L’objet commit ne correspond pas à l’aperçu confirmé.".into());
    }
    let mut command = command(repository).await?;
    command.args([
        "update-ref",
        "--create-reflog",
        "-m",
        "Atelier: explicit local commit",
        &format!("refs/heads/{}", expected.branch),
        commit,
        &expected.head,
    ]);
    let output =
        process_tree::capture(&mut command, &[], Duration::from_secs(30), 4 * 1024 * 1024).await?;
    if !output.status.success() {
        return Err("Le déplacement conditionnel de la branche a été refusé.".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selections_and_authorship_are_explicit_and_bounded() {
        for selected in [
            vec![],
            vec!["../secret".into()],
            vec!["a".into(), "a".into()],
            vec![".git/config".into()],
            vec!["a\nb".into()],
        ] {
            assert!(paths(&selected).is_err());
        }
        assert!(paths(&["-option é.txt".into()]).is_ok());
        assert!(paths(&(0..101).map(|n| format!("file-{n}")).collect::<Vec<_>>()).is_err());
        assert!(
            paths(
                &(0..10)
                    .map(|n| format!("{n}{}", "x".repeat(2000)))
                    .collect::<Vec<_>>()
            )
            .is_err()
        );
        assert!(
            identity(
                "Synthetic",
                "synthetic@example.invalid",
                "Commit\n\nDetails"
            )
            .is_ok()
        );
        for (name, email, message) in [
            ("", "s@example.invalid", "Message"),
            ("Synthetic <x>", "s@example.invalid", "Message"),
            ("Synthetic", "bad@", "Message"),
            ("Synthetic", "s@example.invalid", "\0"),
        ] {
            assert!(identity(name, email, message).is_err());
        }
    }
}
