use super::{
    checked_file::{CheckedFile, require_plain},
    files::Repository,
    task_creation::path_string,
};
use crate::Result;
use atelier_core::{
    application::worktrees::validate_reference as reference,
    contracts::{Worktree, WorktreeState},
    domain::projects::same_repository,
};
use cap_fs_ext::DirExt;
use cap_std::fs::Dir;
use std::path::{Path, PathBuf};

struct Entry {
    path: String,
    branch: String,
    locked: bool,
}

async fn entries(source: &Repository) -> Result<Vec<Entry>> {
    let output = String::from_utf8(
        source
            .git(&["worktree", "list", "--porcelain", "-z"])
            .await?,
    )?;
    let mut entries = Vec::new();
    for block in output.split("\0\0").filter(|block| !block.is_empty()) {
        let mut entry = Entry {
            path: String::new(),
            branch: String::new(),
            locked: false,
        };
        for field in block.split('\0') {
            if let Some(path) = field.strip_prefix("worktree ") {
                entry.path = path.into();
            }
            if let Some(branch) = field.strip_prefix("branch refs/heads/") {
                entry.branch = branch.into();
            }
            if field == "locked" || field.starts_with("locked ") {
                entry.locked = true;
            }
        }
        if entry.path.is_empty() {
            return Err("message.invalid_git_worktree_list".into());
        }
        entries.push(entry);
    }
    Ok(entries)
}

fn same(a: &str, b: &str) -> bool {
    same_repository(a, b, cfg!(windows))
}

fn managed_path(roots: &[PathBuf], worktree: &Worktree) -> Result<PathBuf> {
    let parent = roots
        .first()
        .ok_or("message.synthetic_root_missing")?
        .join("repositories/atelier-worktrees");
    let target = PathBuf::from(&worktree.path);
    let name = target
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("message.invalid_worktree_folder")?;
    let id = uuid::Uuid::parse_str(name).map_err(|_| "message.invalid_worktree_folder")?;
    if name != id.to_string() || !same(&worktree.path, &path_string(&parent.join(name))) {
        return Err("message.the_worktree_is_outside_its_managed_folder".into());
    }
    Ok(PathBuf::from(path_string(&parent.join(name))))
}

fn managed_directory(root: &Dir) -> Result<Dir> {
    let repositories = root.open_dir_nofollow("repositories")?;
    match repositories.create_dir("atelier-worktrees") {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.into()),
    }
    require_plain(&repositories.symlink_metadata("atelier-worktrees")?)?;
    Ok(repositories.open_dir_nofollow("atelier-worktrees")?)
}

pub async fn prepare(
    root: &Dir,
    roots: &[PathBuf],
    repo: &str,
    branch: String,
    base: String,
) -> Result<Worktree> {
    reference(&branch)?;
    reference(&base)?;
    let source = Repository::synthetic(root, roots, repo)?;
    source.verify_root().await?;
    source
        .git(&["check-ref-format", "--branch", &branch])
        .await?;
    if entries(&source)
        .await?
        .iter()
        .any(|entry| entry.branch == branch)
    {
        return Err("message.this_branch_is_already_used_by_a_worktree".into());
    }
    let base_commit = String::from_utf8(
        source
            .git(&[
                "rev-parse",
                "--verify",
                "--end-of-options",
                &format!("{base}^{{commit}}"),
            ])
            .await?,
    )?
    .trim()
    .to_owned();
    if !matches!(base_commit.len(), 40 | 64)
        || !base_commit.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("message.invalid_base_commit".into());
    }
    let _parent = managed_directory(root)?;
    let path = roots[0]
        .join("repositories/atelier-worktrees")
        .join(uuid::Uuid::new_v4().to_string());
    Ok(Worktree {
        path: path_string(&path),
        source: path_string(&source.path),
        branch,
        base,
        base_commit,
        state: WorktreeState::Creating,
    })
}

async fn source(root: &Dir, roots: &[PathBuf], worktree: &Worktree) -> Result<Repository> {
    managed_path(roots, worktree)?;
    let source = Repository::synthetic(root, roots, worktree.source.as_str())?;
    source.verify_root().await?;
    Ok(source)
}

pub async fn prepare_from_origin(
    root: &Dir,
    roots: &[PathBuf],
    repo: &str,
    branch: String,
    base: String,
) -> Result<Worktree> {
    prepare_from_origin_with_auth(root, roots, repo, branch, base, None).await
}
pub async fn prepare_from_forge_origin(
    root: &Dir,
    roots: &[PathBuf],
    repo: &str,
    branch: String,
    base: String,
    authentication: &super::git_transport::HttpsAuthentication,
) -> Result<Worktree> {
    prepare_from_origin_with_auth(root, roots, repo, branch, base, Some(authentication)).await
}
async fn prepare_from_origin_with_auth(
    root: &Dir,
    roots: &[PathBuf],
    repo: &str,
    branch: String,
    base: String,
    authentication: Option<&super::git_transport::HttpsAuthentication>,
) -> Result<Worktree> {
    reference(&base)?;
    // Validate the destination before downloading; existing task branches are never moved.
    let mut plan = prepare(root, roots, repo, branch, "HEAD".into()).await?;
    let source = source(root, roots, &plan).await?;
    plan.base_commit = fetch_origin_commit(root, roots, &source, &base, authentication).await?;
    plan.base = base;
    Ok(plan)
}

pub(super) async fn fetch_origin_commit(
    root: &Dir,
    roots: &[PathBuf],
    source: &Repository,
    base: &str,
    authentication: Option<&super::git_transport::HttpsAuthentication>,
) -> Result<String> {
    reference(base)?;
    let remote_base = base
        .strip_prefix("refs/remotes/origin/")
        .or_else(|| base.strip_prefix("origin/"))
        .or_else(|| base.strip_prefix("refs/heads/"))
        .unwrap_or(base);
    let remote_ref = if remote_base == "HEAD" {
        "HEAD".to_owned()
    } else {
        if remote_base.starts_with("refs/") {
            return Err("message.choose_an_origin_branch_not_another_git_reference".into());
        }
        let name = format!("refs/heads/{remote_base}");
        source.git(&["check-ref-format", &name]).await?;
        name
    };
    let config = source
        .git(&[
            "config",
            "--local",
            "--no-includes",
            "--null",
            "--name-only",
            "--list",
        ])
        .await?;
    let config = String::from_utf8(config)?;
    if authentication.is_some() {
        super::git_transport::HttpsAuthentication::validate_configuration(&config)?;
    }
    for key in config.split('\0') {
        let key = key.to_ascii_lowercase();
        if key.starts_with("url.")
            || key.starts_with("include.")
            || key.starts_with("includeif.")
            || key == "extensions.worktreeconfig"
        {
            return Err(
                "message.synthetic_fetching_refuses_git_url_rewrites_and_configuration_in".into(),
            );
        }
    }
    let origin = String::from_utf8(
        source
            .git(&[
                "config",
                "--local",
                "--no-includes",
                "--get",
                "remote.origin.url",
            ])
            .await?,
    )?;
    let origin = origin.trim_end_matches(['\r', '\n']);
    let remote_path = if let Some(authentication) = authentication {
        authentication.origin(origin)?
    } else {
        let remote = Repository::synthetic(root, roots, origin).map_err(
            |_| "message.origin_must_identify_an_authorized_local_repository_or_an_explic",
        )?;
        remote.verify_root().await?;
        path_string(&remote.path)
    };
    let private_ref = format!("refs/atelier-fetch/{}", uuid::Uuid::new_v4());
    let fetched = async {
        if let Some(authentication) = authentication {
            authentication
                .fetch(source, &remote_path, &remote_ref, &private_ref)
                .await?;
        } else {
            source
                .without_programs(&[
                    "-c",
                    "protocol.allow=never",
                    "-c",
                    "protocol.file.allow=always",
                    "fetch",
                    "--no-tags",
                    "--no-write-fetch-head",
                    "--no-recurse-submodules",
                    "--no-auto-maintenance",
                    "--no-prune",
                    "--",
                    &remote_path,
                    &format!("{remote_ref}:{private_ref}"),
                ])
                .await?;
        }
        let commit = String::from_utf8(
            source
                .git(&[
                    "rev-parse",
                    "--verify",
                    "--end-of-options",
                    &format!("{private_ref}^{{commit}}"),
                ])
                .await?,
        )?
        .trim()
        .to_owned();
        if !matches!(commit.len(), 40 | 64) || !commit.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err("message.invalid_fetched_commit".into());
        }
        Ok::<_, crate::Error>(commit)
    }
    .await;
    let cleanup = source
        .without_programs(&["update-ref", "-d", &private_ref])
        .await;
    if cleanup.is_err() {
        return Err(
            "message.could_not_clean_up_the_temporary_reference_the_source_repository".into(),
        );
    }
    fetched
}

pub async fn inspect(root: &Dir, roots: &[PathBuf], worktree: &Worktree) -> Result<Repository> {
    let source = source(root, roots, worktree).await?;
    let entries = entries(&source).await?;
    if !entries
        .iter()
        .any(|entry| same(&entry.path, &worktree.path) && entry.branch == worktree.branch)
    {
        return Err("message.the_associated_worktree_is_missing_or_its_branch_has_changed".into());
    }
    let target = Repository::open_synthetic(root, roots, worktree.path.as_str())?;
    metadata(&source, &target)?;
    target.verify_root().await?;
    let branch = String::from_utf8(target.git(&["symbolic-ref", "HEAD"]).await?)?;
    if branch.trim() != format!("refs/heads/{}", worktree.branch) {
        return Err("message.the_associated_worktree_is_missing_or_its_branch_has_changed".into());
    }
    Ok(target)
}

pub async fn inspect_status(
    root: &Dir,
    roots: &[PathBuf],
    worktree: &Worktree,
) -> Result<(String, bool)> {
    let repository = inspect(root, roots, worktree).await?;
    let head = String::from_utf8(repository.git(&["rev-parse", "--verify", "HEAD"]).await?)?;
    let head = head.trim().to_owned();
    super::files::validate_commit(&head)?;
    let changes = repository
        .git(&["status", "--porcelain=v1", "-z", "--untracked-files=normal"])
        .await?;
    let after = String::from_utf8(repository.git(&["rev-parse", "--verify", "HEAD"]).await?)?;
    if head != after.trim() {
        return Err(
            "message.the_worktree_commit_changed_during_inspection_refresh_its_status".into(),
        );
    }
    inspect(root, roots, worktree).await?;
    Ok((head, changes.is_empty()))
}

pub(super) fn metadata(source: &Repository, target: &Repository) -> Result<Dir> {
    let link =
        String::from_utf8(CheckedFile::open_in(&target.directory, Path::new(".git"))?.read(8192)?)?;
    let gitdir = link
        .trim_end_matches(['\r', '\n'])
        .strip_prefix("gitdir: ")
        .ok_or("message.invalid_git_worktree_association")?;
    let gitdir_path = PathBuf::from(gitdir);
    let name = gitdir_path
        .file_name()
        .ok_or("message.invalid_git_worktree_association")?;
    if !same(
        gitdir,
        &path_string(&source.path.join(".git/worktrees").join(name)),
    ) {
        return Err("message.the_source_repository_no_longer_matches_the_worktree".into());
    }
    let metadata = source
        .directory
        .open_dir_nofollow(".git")?
        .open_dir_nofollow("worktrees")?
        .open_dir_nofollow(name)?;
    let common =
        String::from_utf8(CheckedFile::open_in(&metadata, Path::new("commondir"))?.read(128)?)?;
    let backlink =
        String::from_utf8(CheckedFile::open_in(&metadata, Path::new("gitdir"))?.read(8192)?)?;
    if common.trim() != "../.."
        || !same(
            backlink.trim_end_matches(['\r', '\n']),
            &path_string(&target.path.join(".git")),
        )
    {
        return Err("message.invalid_git_worktree_association".into());
    }
    Ok(metadata)
}

pub async fn create(root: &Dir, roots: &[PathBuf], worktree: &Worktree) -> Result<()> {
    super::files::validate_commit(&worktree.base_commit)?;
    let source = source(root, roots, worktree).await?;
    let registered = entries(&source).await?;
    if registered
        .iter()
        .any(|entry| same(&entry.path, &worktree.path))
    {
        super::agent_context::sync(root, roots, worktree).await?;
        return super::dependencies::sync(root, roots, worktree).await;
    }
    if registered
        .iter()
        .any(|entry| entry.branch == worktree.branch)
    {
        return Err("message.this_branch_is_already_used_by_a_worktree".into());
    }
    let parent = managed_directory(root)?;
    let target = managed_path(roots, worktree)?;
    match parent.symlink_metadata(
        target
            .file_name()
            .ok_or("message.invalid_worktree_folder")?,
    ) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Ok(_) => {
            return Err(
                "message.the_destination_folder_already_exists_no_files_were_replaced".into(),
            );
        }
        Err(error) => return Err(error.into()),
    }
    reference(&worktree.branch)?;
    source
        .git(&["check-ref-format", "--branch", &worktree.branch])
        .await?;
    let full = format!("refs/heads/{}", worktree.branch);
    let refs = String::from_utf8(
        source
            .git(&["for-each-ref", "--format=%(refname)", &full])
            .await?,
    )?;
    let exists = refs.lines().any(|line| line == full);
    let mut args = vec!["worktree", "add"];
    if !exists {
        args.extend(["-b", &worktree.branch]);
    }
    args.extend([
        "--",
        &worktree.path,
        if exists {
            &worktree.branch
        } else {
            &worktree.base_commit
        },
    ]);
    source.without_programs(&args).await?;
    super::agent_context::sync(root, roots, worktree).await?;
    super::dependencies::sync(root, roots, worktree).await
}

pub async fn remove(
    root: &Dir,
    roots: &[PathBuf],
    worktree: &Worktree,
    discard: bool,
) -> Result<()> {
    let source = source(root, roots, worktree).await?;
    let registered = entries(&source).await?;
    let found = registered
        .iter()
        .find(|entry| same(&entry.path, &worktree.path));
    let parent = managed_directory(root)?;
    let target = managed_path(roots, worktree)?;
    let missing = match parent.symlink_metadata(
        target
            .file_name()
            .ok_or("message.invalid_worktree_folder")?,
    ) {
        Ok(metadata) => {
            require_plain(&metadata)?;
            false
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => true,
        Err(error) => return Err(error.into()),
    };
    let Some(found) = found else {
        if missing {
            return Ok(());
        }
        return Err(
            "message.this_folder_is_no_longer_registered_as_a_worktree_no_files_were".into(),
        );
    };
    if found.branch != worktree.branch || found.locked {
        return Err(
            "message.the_worktree_is_locked_or_its_branch_has_changed_no_files_were_d".into(),
        );
    }
    let detached = if !missing {
        let target = inspect(root, roots, worktree).await?;
        let metadata = metadata(&source, &target)?;
        let directory = source.directory.try_clone()?;
        let path = source.path.clone();
        tokio::task::spawn_blocking(move || {
            super::dependencies::detach(&directory, &path, &target.directory, &metadata)
        })
        .await??
    } else {
        std::collections::BTreeSet::new()
    };
    let result = async {
        if !missing {
            let target = inspect(root, roots, worktree).await?;
            if !discard {
                let changed = !target
                    .without_programs(&["status", "--porcelain=v1", "--untracked-files=all", "-z"])
                    .await?
                    .is_empty();
                let ignored = String::from_utf8(
                    target
                        .git(&[
                            "ls-files",
                            "--others",
                            "--ignored",
                            "--exclude-standard",
                            "-z",
                        ])
                        .await?,
                )?
                .split('\0')
                .filter(|path| !path.is_empty())
                .map(str::to_owned)
                .collect();
                let metadata = metadata(&source, &target)?;
                let files = target.directory.try_clone()?;
                let unowned = tokio::task::spawn_blocking(move || {
                    super::agent_context::has_unowned_ignored(metadata, files, ignored)
                })
                .await??;
                if changed || unowned {
                    return Err(
                        "message.this_worktree_contains_modified_new_or_ignored_files_confirm_dis"
                            .into(),
                    );
                }
            }
        }
        let mut args = vec!["worktree", "remove"];
        if discard || missing {
            args.push("--force");
        }
        args.extend(["--", &worktree.path]);
        source.without_programs(&args).await?;
        Ok::<_, crate::Error>(())
    }
    .await;
    if let Err(error) = result {
        if !detached.is_empty() {
            let restored = async {
                let target = inspect(root, roots, worktree).await?;
                tokio::task::spawn_blocking(move || {
                    super::dependencies::restore(
                        &source.directory,
                        &source.path,
                        &target.directory,
                        &detached,
                    )
                })
                .await?
            }
            .await;
            if let Err(restore) = restored {
                return Err(
                    format!("{error} Restauration des liens impossible : {restore}").into(),
                );
            }
        }
        return Err(error);
    }
    Ok(())
}
