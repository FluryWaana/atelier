use super::{
    agent_context::{exclude, parent},
    checked_file::{CheckedFile, require_plain},
    directory_link,
    files::Repository,
    publish,
    task_creation::path_string,
    worktrees,
};
use crate::Result;
use atelier_core::{
    contracts::Worktree,
    document::JsonString,
    domain::{files::validate_path, projects::same_repository},
};
use cap_fs_ext::DirExt;
use cap_std::fs::Dir;
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

const MANIFEST: &str = "atelier-dependencies.json";
const LIMIT: usize = 256;

fn valid(name: &str) -> bool {
    validate_path(name, true).is_ok()
        && !name.contains(['\r', '\n'])
        && matches!(name.rsplit('/').next(), Some("node_modules" | "packages"))
}

fn manifest(metadata: &Dir) -> Result<BTreeSet<String>> {
    match metadata.symlink_metadata(MANIFEST) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeSet::new()),
        Err(error) => return Err(error.into()),
        Ok(info) => require_plain(&info)?,
    }
    let names: BTreeSet<String> = serde_json::from_slice(
        &CheckedFile::open_in(metadata, Path::new(MANIFEST))?
            .single_link()?
            .read(128 * 1024)?,
    )?;
    if names.len() > LIMIT || names.iter().any(|name| !valid(name)) {
        return Err("message.invalid_shared_dependency_inventory".into());
    }
    Ok(names)
}

fn source_directory(source: &Dir, name: &str) -> Result<Option<Dir>> {
    let Some((directory, file)) = parent(source, name, false)? else {
        return Ok(None);
    };
    match directory.symlink_metadata(&file) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
        Ok(info) => {
            if require_plain(&info).is_err() {
                return Ok(None);
            }
            if !info.is_dir() {
                return Ok(None);
            }
            Ok(Some(directory.open_dir_nofollow(file)?))
        }
    }
}

fn absent(target: &Dir, name: &str) -> Result<bool> {
    let Some((directory, file)) = parent(target, name, false)? else {
        return Ok(false);
    };
    match directory.symlink_metadata(file) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(error) => Err(error.into()),
        Ok(_) => Ok(false),
    }
}

fn verify(target: &Dir, source_path: &Path, name: &str) -> Result<bool> {
    let Some((directory, file)) = parent(target, name, false)? else {
        return Ok(false);
    };
    match directory.symlink_metadata(&file) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
        Ok(_) => {}
    }
    let destination = directory
        .read_link_contents(file)
        .map_err(|_| "message.the_dependency_link_has_changed_operation_refused")?;
    if !same_repository(
        &path_string(&destination).utf8()?,
        &path_string(&source_path.join(name)).utf8()?,
        cfg!(windows),
    ) {
        return Err("message.the_dependency_link_has_changed_operation_refused".into());
    }
    Ok(true)
}

fn nuget(directory: &Dir) -> Result<bool> {
    for (count, entry) in directory.entries()?.enumerate() {
        if count >= 2000 {
            return Err("message.too_many_nuget_package_folders".into());
        }
        let entry = entry?;
        let info = directory.symlink_metadata(entry.file_name())?;
        if require_plain(&info).is_err() || !info.is_dir() {
            continue;
        }
        let package = directory.open_dir_nofollow(entry.file_name())?;
        for (count, file) in package.entries()?.enumerate() {
            if count >= 2000 {
                return Err("message.too_many_files_in_a_nuget_package".into());
            }
            let file = file?;
            if Path::new(&file.file_name())
                .extension()
                .is_some_and(|extension| extension == "nupkg")
            {
                let info = package.symlink_metadata(file.file_name())?;
                if require_plain(&info).is_ok() && info.is_file() {
                    return Ok(true);
                }
            }
        }
    }
    Ok(false)
}

fn candidates(
    source: &Dir,
    target: &Dir,
    tracked_source: &str,
    tracked_target: &str,
) -> Result<BTreeSet<String>> {
    let mut folders = BTreeSet::from([String::new()]);
    let mut tracked = BTreeSet::new();
    for file in tracked_source
        .split('\0')
        .chain(tracked_target.split('\0'))
        .filter(|path| !path.is_empty())
    {
        let mut path = file;
        tracked.insert(path.to_owned());
        while let Some((folder, _)) = path.rsplit_once('/') {
            tracked.insert(folder.to_owned());
            path = folder;
        }
    }
    for path in tracked_source.split('\0').filter(|path| !path.is_empty()) {
        let mut path = path;
        while let Some((folder, _)) = path.rsplit_once('/') {
            folders.insert(format!("{folder}/"));
            if folders.len() > 50_000 {
                return Err("message.too_many_repository_directories".into());
            }
            path = folder;
        }
    }
    let mut names = BTreeSet::new();
    for folder in folders {
        for kind in ["node_modules", "packages"] {
            let name = format!("{folder}{kind}");
            if !valid(&name) || tracked.contains(&name) || !absent(target, &name)? {
                continue;
            }
            let Some(directory) = source_directory(source, &name)? else {
                continue;
            };
            if kind == "packages" && !nuget(&directory)? {
                continue;
            }
            names.insert(name);
            if names.len() > LIMIT {
                return Err("message.too_many_shared_dependencies_maximum_256".into());
            }
        }
    }
    Ok(names)
}

pub(super) async fn sync(root: &Dir, roots: &[PathBuf], worktree: &Worktree) -> Result<()> {
    let source = Repository::synthetic(root, roots, &JsonString::from(worktree.source.as_str()))?;
    let target = worktrees::inspect(root, roots, worktree).await?;
    let metadata = worktrees::metadata(&source, &target)?;
    let tracked_source = String::from_utf8(source.git(&["ls-files", "--cached", "-z"]).await?)?;
    let tracked_target = String::from_utf8(target.git(&["ls-files", "--cached", "-z"]).await?)?;
    for name in manifest(&metadata)? {
        let prefix = format!("{name}/");
        if tracked_target
            .split('\0')
            .any(|file| file == name || file.starts_with(&prefix))
        {
            return Err(
                "message.a_shared_dependency_is_now_tracked_by_git_check_the_worktree".into(),
            );
        }
    }
    let source_dir = source.directory.try_clone()?;
    let target_dir = target.directory.try_clone()?;
    let names = tokio::task::spawn_blocking(move || {
        candidates(&source_dir, &target_dir, &tracked_source, &tracked_target)
    })
    .await??;
    let mut eligible = BTreeSet::new();
    for name in names {
        if source
            .git_optional(&["check-ignore", "-q", "--", &format!("{name}/")], true)
            .await?
            .is_some()
        {
            eligible.insert(name);
        }
    }
    tokio::task::spawn_blocking(move || {
        let mut owned = manifest(&metadata)?;
        for name in &owned {
            verify(&target.directory, &source.path, name)?;
        }
        owned.extend(eligible);
        if owned.len() > LIMIT {
            return Err("message.too_many_shared_dependencies_maximum_256".into());
        }
        exclude(&source.directory, owned.iter().cloned())?;
        // Intent precedes links: retries can recognize only paths already recorded for this source.
        publish::write_private(
            &metadata,
            Path::new(MANIFEST),
            &serde_json::to_vec(&owned)?,
            false,
        )?;
        restore(&source.directory, &source.path, &target.directory, &owned)
    })
    .await?
}

pub(super) fn restore(
    source: &Dir,
    source_path: &Path,
    target: &Dir,
    names: &BTreeSet<String>,
) -> Result<()> {
    for name in names {
        if verify(target, source_path, name)? {
            continue;
        }
        let _source = source_directory(source, name)?
            .ok_or("message.source_dependencies_are_missing_the_link_cannot_be_restored")?;
        let (directory, file) =
            parent(target, name, false)?.ok_or("message.the_dependency_folder_is_missing")?;
        directory_link::create(&directory, &file, &source_path.join(name))?;
    }
    Ok(())
}

pub(super) fn detach(
    source: &Dir,
    source_path: &Path,
    target: &Dir,
    metadata: &Dir,
) -> Result<BTreeSet<String>> {
    let mut present = BTreeSet::new();
    for name in manifest(metadata)? {
        if verify(target, source_path, &name)? {
            present.insert(name);
        }
    }
    let result = (|| -> Result<()> {
        for name in &present {
            if !verify(target, source_path, name)? {
                continue;
            }
            let (directory, file) =
                parent(target, name, false)?.ok_or("message.the_dependency_folder_is_missing")?;
            directory_link::remove(&directory, &file)?;
        }
        Ok(())
    })();
    if let Err(error) = result {
        restore(source, source_path, target, &present)
            .map_err(|restore| format!("{error} Restauration des liens impossible : {restore}"))?;
        return Err(error);
    }
    Ok(present)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inventory_refuses_paths_outside_dependency_folders() {
        for name in ["node_modules", "front/node_modules", "back/packages"] {
            assert!(valid(name));
        }
        for name in [
            "../node_modules",
            ".git/node_modules",
            "front\\node_modules",
            "front/secret",
            "C:/node_modules",
            "front\n/node_modules",
        ] {
            assert!(!valid(name));
        }
    }

    #[cfg(windows)]
    #[test]
    fn changed_link_target_refuses_detach_before_touching_any_link() {
        let fixture = tempfile::tempdir().unwrap();
        let root = Dir::open_ambient_dir(fixture.path(), cap_std::ambient_authority()).unwrap();
        for name in ["source", "target", "metadata", "unrelated"] {
            root.create_dir(name).unwrap();
        }
        let source = root.open_dir("source").unwrap();
        source.create_dir("node_modules").unwrap();
        let target = root.open_dir("target").unwrap();
        let metadata = root.open_dir("metadata").unwrap();
        let source_path = std::fs::canonicalize(fixture.path().join("source")).unwrap();
        let unrelated = std::fs::canonicalize(fixture.path().join("unrelated")).unwrap();
        root.write("unrelated/keep", b"preserve unrelated content")
            .unwrap();
        directory_link::create(&target, Path::new("node_modules"), &unrelated).unwrap();
        publish::write_private(
            &metadata,
            Path::new(MANIFEST),
            br#"["node_modules"]"#,
            false,
        )
        .unwrap();
        assert!(detach(&source, &source_path, &target, &metadata).is_err());
        assert!(target.read_link_contents("node_modules").is_ok());
        assert_eq!(
            root.read("unrelated/keep").unwrap(),
            b"preserve unrelated content"
        );
        directory_link::remove(&target, Path::new("node_modules")).unwrap();
    }
}
