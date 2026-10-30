use super::{
    checked_file::{CheckedFile, require_plain},
    files::Repository,
    publish, worktrees,
};
use crate::Result;
use atelier_core::{
    contracts::Worktree,
    domain::agent_context::{COUNT_LIMIT, FILE_LIMIT, TOTAL_LIMIT, allowed},
};
use cap_fs_ext::DirExt;
use cap_std::fs::Dir;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

const MANIFEST: &str = "atelier-agent-context.json";

#[derive(Clone, Deserialize, Serialize)]
struct OwnedCopy {
    current: Option<String>,
    pending: Option<String>,
}
type Manifest = BTreeMap<String, OwnedCopy>;
struct Contents {
    bytes: Vec<u8>,
    executable: bool,
}
struct Change {
    name: String,
    before: Option<String>,
    next: Option<Contents>,
}

fn digest(bytes: &[u8]) -> String {
    super::session::hexadecimal(&Sha256::digest(bytes))
}

pub(super) fn parent(root: &Dir, name: &str, create: bool) -> Result<Option<(Dir, PathBuf)>> {
    let mut directory = root.try_clone()?;
    let mut parts = name.split('/').peekable();
    while let Some(part) = parts.next() {
        if parts.peek().is_none() {
            return Ok(Some((directory, PathBuf::from(part))));
        }
        match directory.symlink_metadata(part) {
            Ok(metadata) => require_plain(&metadata)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if !create {
                    return Ok(None);
                }
                let builder = cap_std::fs::DirBuilder::new();
                #[cfg(unix)]
                let builder = {
                    use cap_std::fs::DirBuilderExt;
                    let mut builder = builder;
                    builder.mode(0o700);
                    builder
                };
                directory.create_dir_with(part, &builder)?;
            }
            Err(error) => return Err(error.into()),
        }
        directory = directory.open_dir_nofollow(part)?;
    }
    Err("message.invalid_agent_configuration_path".into())
}

fn read(root: &Dir, name: &str, limit: usize) -> Result<Option<Vec<u8>>> {
    let Some((directory, name)) = parent(root, name, false)? else {
        return Ok(None);
    };
    match directory.symlink_metadata(&name) {
        Ok(metadata) => require_plain(&metadata)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    }
    Ok(Some(
        CheckedFile::open_in(&directory, &name)?
            .single_link()?
            .read(limit)?,
    ))
}

fn manifest(metadata: &Dir) -> Result<Manifest> {
    let value: Manifest = match read(metadata, MANIFEST, TOTAL_LIMIT)? {
        Some(bytes) => serde_json::from_slice(&bytes)?,
        None => Manifest::new(),
    };
    if value.len() > COUNT_LIMIT
        || value.iter().any(|(name, copy)| {
            !allowed(name)
                || [&copy.current, &copy.pending]
                    .into_iter()
                    .flatten()
                    .any(|hash| {
                        hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit())
                    })
        })
    {
        return Err("message.invalid_agent_context_inventory".into());
    }
    Ok(value)
}

fn save(metadata: &Dir, manifest: &Manifest) -> Result<()> {
    publish::write_private(
        metadata,
        Path::new(MANIFEST),
        &serde_json::to_vec(manifest)?,
        false,
    )
}

pub(super) fn exclude(source: &Dir, names: impl Iterator<Item = String>) -> Result<()> {
    let git = source.open_dir_nofollow(".git")?;
    match git.create_dir("info") {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.into()),
    }
    let info = git.open_dir_nofollow("info")?;
    let mut bytes = read(&info, "exclude", TOTAL_LIMIT)?.unwrap_or_default();
    let mut changed = false;
    for name in names {
        let mut rule = String::from("/");
        for character in name.chars() {
            if matches!(character, '\\' | ' ' | '*' | '?' | '[' | ']' | '#' | '!') {
                rule.push('\\');
            }
            rule.push(character);
        }
        if !bytes
            .split(|byte| *byte == b'\n')
            .any(|line| line.strip_suffix(b"\r").unwrap_or(line) == rule.as_bytes())
        {
            if !bytes.ends_with(b"\n") {
                bytes.push(b'\n');
            }
            bytes.extend_from_slice(rule.as_bytes());
            bytes.push(b'\n');
            changed = true;
        }
    }
    if bytes.len() > TOTAL_LIMIT {
        return Err("message.git_exclusion_rules_are_too_large".into());
    }
    if changed {
        publish::write_private(&info, Path::new("exclude"), &bytes, false)?;
    }
    Ok(())
}

fn synchronize(
    source: Dir,
    target: Dir,
    metadata: Dir,
    names: Vec<String>,
    tracked: BTreeSet<String>,
) -> Result<()> {
    let previous = manifest(&metadata)?;
    let mut files = BTreeMap::new();
    let mut total = 0usize;
    for name in names {
        if tracked.contains(&name) {
            continue;
        }
        let Some(bytes) = read(&source, &name, FILE_LIMIT)? else {
            continue;
        };
        total += bytes.len();
        if total > TOTAL_LIMIT {
            return Err(
                "message.agent_configuration_is_too_large_1_mib_per_file_8_mib_total".into(),
            );
        }
        #[cfg(unix)]
        let executable = {
            use cap_std::fs::PermissionsExt;
            let (directory, file) =
                parent(&source, &name, false)?.ok_or("message.agent_configuration_is_missing")?;
            directory.symlink_metadata(file)?.permissions().mode() & 0o100 != 0
        };
        #[cfg(not(unix))]
        let executable = false;
        files.insert(name, Contents { bytes, executable });
    }
    let names = files
        .keys()
        .chain(previous.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    if names.len() > COUNT_LIMIT {
        return Err("message.agent_configuration_is_too_large_2_000_files_maximum".into());
    }
    let mut planned = Manifest::new();
    let mut finished = Manifest::new();
    let mut changes = Vec::new();
    for name in names {
        if tracked.contains(&name) {
            continue;
        }
        let current = read(&target, &name, FILE_LIMIT)?.map(|bytes| digest(&bytes));
        if let Some(hash) = &current
            && !previous.get(&name).is_some_and(|copy| {
                copy.current.as_ref() == Some(hash) || copy.pending.as_ref() == Some(hash)
            })
        {
            continue; // Task edits relinquish ownership; later syncs must leave them alone too.
        }
        let next = files.remove(&name);
        let hash = next.as_ref().map(|file| digest(&file.bytes));
        planned.insert(
            name.clone(),
            OwnedCopy {
                current: current.clone(),
                pending: hash.clone(),
            },
        );
        if hash.is_some() {
            finished.insert(
                name.clone(),
                OwnedCopy {
                    current: hash.clone(),
                    pending: None,
                },
            );
        }
        if current != hash {
            changes.push(Change {
                name,
                before: current,
                next,
            });
        }
    }
    exclude(&source, finished.keys().cloned())?;
    // Record both valid contents before changing files, so interrupted copies can be resumed safely.
    save(&metadata, &planned)?;
    for change in changes {
        if read(&target, &change.name, FILE_LIMIT)?.map(|bytes| digest(&bytes)) != change.before {
            return Err(
                "message.agent_context_changed_during_synchronization_verify_it_before_tr".into(),
            );
        }
        let (directory, name) = parent(&target, &change.name, change.next.is_some())?
            .ok_or("message.agent_configuration_is_missing")?;
        if let Some(file) = change.next {
            // Private temporary bytes stay in Git metadata, never as publishable untracked files.
            publish::write_private_from(
                &metadata,
                &directory,
                &name,
                &file.bytes,
                file.executable,
            )?;
        } else {
            directory.remove_file(name)?;
        }
    }
    save(&metadata, &finished)
}

pub(super) async fn sync(root: &Dir, roots: &[PathBuf], worktree: &Worktree) -> Result<()> {
    let source = Repository::synthetic(root, roots, worktree.source.as_str())?;
    let target = worktrees::inspect(root, roots, worktree).await?;
    let metadata = worktrees::metadata(&source, &target)?;
    for name in [
        ".claude",
        ".claude/skills",
        ".claude/agents",
        ".claude/commands",
        ".claude/rules",
        ".claude/hooks",
        ".claude/plugins",
    ] {
        if let Some((directory, name)) = parent(&source.directory, name, false)? {
            match directory.symlink_metadata(name) {
                Ok(metadata) => require_plain(&metadata)?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
    }
    let names = String::from_utf8(
        source
            .git(&[
                "ls-files",
                "--cached",
                "--others",
                "-z",
                "--",
                "CLAUDE.md",
                "CLAUDE.local.md",
                "AGENTS.md",
                "AGENTS.override.md",
                ".mcp.json",
                ".claude",
            ])
            .await?,
    )?;
    let names = names
        .split('\0')
        .filter(|name| allowed(name))
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    if names.len() > COUNT_LIMIT {
        return Err("message.agent_configuration_is_too_large_2_000_files_maximum".into());
    }
    let tracked = String::from_utf8(target.git(&["ls-files", "--cached", "-z"]).await?)?
        .split('\0')
        .map(str::to_owned)
        .collect();
    tokio::task::spawn_blocking(move || {
        synchronize(
            source.directory,
            target.directory,
            metadata,
            names.into_iter().collect(),
            tracked,
        )
    })
    .await?
}

pub(super) fn has_unowned_ignored(metadata: Dir, target: Dir, names: Vec<String>) -> Result<bool> {
    let owned = manifest(&metadata)?;
    for name in names {
        let Some(copy) = owned.get(&name) else {
            return Ok(true);
        };
        let current = read(&target, &name, FILE_LIMIT)?.map(|bytes| digest(&bytes));
        if current.is_none() || (current != copy.current && current != copy.pending) {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn interrupted_context_publications_accept_only_the_recorded_old_or_new_contents() {
        let fixture = tempfile::tempdir().unwrap();
        let root = Dir::open_ambient_dir(fixture.path(), cap_std::ambient_authority()).unwrap();
        for path in ["source", "target", "metadata"] {
            root.create_dir(path).unwrap();
        }
        let source = root.open_dir("source").unwrap();
        source.create_dir(".git").unwrap();
        let target = root.open_dir("target").unwrap();
        let metadata = root.open_dir("metadata").unwrap();
        for current in ["before", "pending", "task edit"] {
            source.write("AGENTS.md", b"latest source").unwrap();
            target.write("AGENTS.md", current.as_bytes()).unwrap();
            let mut planned = Manifest::new();
            planned.insert(
                "AGENTS.md".into(),
                OwnedCopy {
                    current: Some(digest(b"before")),
                    pending: Some(digest(b"pending")),
                },
            );
            save(&metadata, &planned).unwrap();
            synchronize(
                source.try_clone().unwrap(),
                target.try_clone().unwrap(),
                metadata.try_clone().unwrap(),
                vec!["AGENTS.md".into()],
                BTreeSet::new(),
            )
            .unwrap();
            assert_eq!(
                target.read_to_string("AGENTS.md").unwrap(),
                if current == "task edit" {
                    "task edit"
                } else {
                    "latest source"
                }
            );
            assert_eq!(
                manifest(&metadata).unwrap().contains_key("AGENTS.md"),
                current != "task edit"
            );
        }
    }
}
