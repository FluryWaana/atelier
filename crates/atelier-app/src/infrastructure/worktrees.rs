use super::{
    checked_file::{CheckedFile, require_plain},
    files::Repository,
    task_creation::path_string,
};
use crate::Result;
use atelier_core::{
    application::worktrees::validate_reference as reference,
    contracts::{Worktree, WorktreeState},
    document::JsonString,
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
            return Err("Liste des worktrees Git invalide.".into());
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
        .ok_or("Racine synthétique absente.")?
        .join("repositories/atelier-worktrees");
    let target = PathBuf::from(&worktree.path);
    let name = target
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("Dossier de worktree invalide.")?;
    let id = uuid::Uuid::parse_str(name).map_err(|_| "Dossier de worktree invalide.")?;
    if name != id.to_string() || !same(&worktree.path, &path_string(&parent.join(name)).utf8()?) {
        return Err("Le worktree sort de son dossier géré.".into());
    }
    Ok(PathBuf::from(path_string(&parent.join(name)).utf8()?))
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
    let source = Repository::synthetic(root, roots, &JsonString::from(repo))?;
    source.verify_root().await?;
    source
        .git(&["check-ref-format", "--branch", &branch])
        .await?;
    if entries(&source)
        .await?
        .iter()
        .any(|entry| entry.branch == branch)
    {
        return Err("Cette branche est déjà utilisée par un worktree.".into());
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
        return Err("Commit de départ invalide.".into());
    }
    let _parent = managed_directory(root)?;
    let path = roots[0]
        .join("repositories/atelier-worktrees")
        .join(uuid::Uuid::new_v4().to_string());
    Ok(Worktree {
        path: path_string(&path).utf8()?,
        source: path_string(&source.path).utf8()?,
        branch,
        base,
        base_commit,
        state: WorktreeState::Creating,
    })
}

async fn source(root: &Dir, roots: &[PathBuf], worktree: &Worktree) -> Result<Repository> {
    managed_path(roots, worktree)?;
    let source = Repository::synthetic(root, roots, &JsonString::from(worktree.source.as_str()))?;
    source.verify_root().await?;
    Ok(source)
}

pub async fn inspect(root: &Dir, roots: &[PathBuf], worktree: &Worktree) -> Result<Repository> {
    let source = source(root, roots, worktree).await?;
    let entries = entries(&source).await?;
    if !entries
        .iter()
        .any(|entry| same(&entry.path, &worktree.path) && entry.branch == worktree.branch)
    {
        return Err("Le worktree associé a disparu ou sa branche a changé.".into());
    }
    let target =
        Repository::open_synthetic(root, roots, &JsonString::from(worktree.path.as_str()))?;
    let link =
        String::from_utf8(CheckedFile::open_in(&target.directory, Path::new(".git"))?.read(8192)?)?;
    let gitdir = link
        .trim_end_matches(['\r', '\n'])
        .strip_prefix("gitdir: ")
        .ok_or("Association Git du worktree invalide.")?;
    let gitdir_path = PathBuf::from(gitdir);
    let name = gitdir_path
        .file_name()
        .ok_or("Association Git du worktree invalide.")?;
    if !same(
        gitdir,
        &path_string(&source.path.join(".git/worktrees").join(name)).utf8()?,
    ) {
        return Err("Le dépôt source ne correspond plus au worktree.".into());
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
            &path_string(&target.path.join(".git")).utf8()?,
        )
    {
        return Err("Association Git du worktree invalide.".into());
    }
    target.verify_root().await?;
    let branch = String::from_utf8(target.git(&["symbolic-ref", "HEAD"]).await?)?;
    if branch.trim() != format!("refs/heads/{}", worktree.branch) {
        return Err("Le worktree associé a disparu ou sa branche a changé.".into());
    }
    Ok(target)
}

pub async fn create(root: &Dir, roots: &[PathBuf], worktree: &Worktree) -> Result<()> {
    super::files::validate_commit(&worktree.base_commit)?;
    let source = source(root, roots, worktree).await?;
    let registered = entries(&source).await?;
    if registered
        .iter()
        .any(|entry| same(&entry.path, &worktree.path))
    {
        inspect(root, roots, worktree).await?;
        return Ok(());
    }
    if registered
        .iter()
        .any(|entry| entry.branch == worktree.branch)
    {
        return Err("Cette branche est déjà utilisée par un worktree.".into());
    }
    let parent = managed_directory(root)?;
    let target = managed_path(roots, worktree)?;
    match parent.symlink_metadata(target.file_name().ok_or("Dossier de worktree invalide.")?) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Ok(_) => {
            return Err("Le dossier cible existe déjà. Aucun fichier n’a été remplacé.".into());
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
    inspect(root, roots, worktree).await?;
    Ok(())
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
    let missing =
        match parent.symlink_metadata(target.file_name().ok_or("Dossier de worktree invalide.")?) {
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
            "Ce dossier n’est plus enregistré comme worktree. Aucun fichier n’a été supprimé."
                .into(),
        );
    };
    if found.branch != worktree.branch || found.locked {
        return Err(
            "Le worktree est verrouillé ou sa branche a changé. Aucun fichier n’a été supprimé."
                .into(),
        );
    }
    if !missing {
        let target = inspect(root, roots, worktree).await?;
        if !discard
            && !target
                .without_programs(&[
                    "status",
                    "--porcelain=v1",
                    "--untracked-files=all",
                    "--ignored=matching",
                    "-z",
                ])
                .await?
                .is_empty()
        {
            return Err("Ce worktree contient des fichiers modifiés, nouveaux ou ignorés. Confirme leur abandon pour le retirer.".into());
        }
    }
    let mut args = vec!["worktree", "remove"];
    if discard || missing {
        args.push("--force");
    }
    args.extend(["--", &worktree.path]);
    source.without_programs(&args).await?;
    Ok(())
}
