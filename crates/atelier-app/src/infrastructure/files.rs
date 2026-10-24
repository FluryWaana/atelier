use super::{
    checked_file::{CheckedFile, require_plain},
    launch_detection,
    task_creation::path_string,
};
use crate::Result;
use atelier_core::{
    contracts::{ChangeListing, FileChange, FileDiff, FileListing, FilePreview},
    document::JsonString,
    domain::files::{PREVIEW_LIMIT, validate_path},
};
use cap_fs_ext::DirExt;
use cap_std::fs::Dir;
use std::path::PathBuf;

pub struct Repository {
    path: PathBuf,
    directory: Dir,
}

impl Repository {
    pub fn synthetic(root: &Dir, roots: &[PathBuf], repo: &JsonString) -> Result<Self> {
        let path = launch_detection::synthetic_repository(
            root,
            roots,
            &serde_json::value::to_raw_value(repo)?,
        )?;
        let path = PathBuf::from(path.utf8()?);
        let base =
            PathBuf::from(path_string(roots.first().ok_or("Racine synthétique absente.")?).utf8()?);
        let directory = root.open_dir_nofollow(path.strip_prefix(base)?)?;
        let metadata = directory
            .symlink_metadata(".git")
            .map_err(|_| "Ce dossier n’est pas une racine Git accessible.")?;
        require_plain(&metadata)?;
        if !metadata.is_dir() {
            return Err(
                "L’inspection des worktrees nécessite leur association vérifiée à une tâche."
                    .into(),
            );
        }
        Ok(Self { path, directory })
    }

    async fn verify_root(&self) -> Result<()> {
        let top = self.git(&["rev-parse", "--show-toplevel"]).await?;
        let top = String::from_utf8(top)?;
        if !atelier_core::domain::projects::same_repository(
            top.trim_end_matches(['\r', '\n']),
            &path_string(&self.path).utf8()?,
            cfg!(windows),
        ) {
            return Err("Le dossier de travail ne correspond plus à la racine du dépôt.".into());
        }
        Ok(())
    }

    pub async fn list(&self) -> Result<FileListing> {
        self.verify_root().await?;
        let output = self
            .git(&[
                "ls-files",
                "--cached",
                "--others",
                "--exclude-standard",
                "--deduplicate",
                "-z",
            ])
            .await?;
        let mut files = String::from_utf8(output)
            .map_err(|_| "Certains noms de fichiers ne sont pas en UTF-8.")?
            .split('\0')
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
            .collect::<Vec<_>>();
        files.sort();
        files.dedup();
        if files.len() > 50_000 {
            return Err("Le dépôt dépasse la limite de 50 000 fichiers de l’explorateur.".into());
        }
        Ok(FileListing { files })
    }

    pub async fn changes(&self, base_commit: Option<&str>) -> Result<ChangeListing> {
        self.verify_root().await?;
        let base = match base_commit {
            Some(commit) => {
                validate_commit(commit)?;
                String::from_utf8(self.git(&["merge-base", commit, "HEAD"]).await?)?
            }
            None => String::from_utf8(
                self.git(&["rev-parse", "--verify", "HEAD^{commit}"])
                    .await?,
            )?,
        };
        let base = base.trim().to_owned();
        validate_commit(&base)?;
        let tracked = self.diff(&["--name-status", "-z", &base, "--"]).await?;
        let tracked = String::from_utf8(tracked)
            .map_err(|_| "Certains noms de fichiers ne sont pas en UTF-8.")?;
        let parts = tracked
            .strip_suffix('\0')
            .unwrap_or(&tracked)
            .split('\0')
            .collect::<Vec<_>>();
        let mut files = Vec::new();
        if !tracked.is_empty() {
            if !parts.len().is_multiple_of(2) {
                return Err("Réponse Git invalide pour les changements.".into());
            }
            for pair in parts.chunks_exact(2) {
                files.push(FileChange {
                    path: pair[1].into(),
                    status: pair[0].into(),
                    untracked: false,
                });
            }
        }
        let untracked = self
            .git(&["ls-files", "--others", "--exclude-standard", "-z"])
            .await?;
        for path in String::from_utf8(untracked)
            .map_err(|_| "Certains noms de fichiers ne sont pas en UTF-8.")?
            .split('\0')
            .filter(|path| !path.is_empty())
        {
            files.push(FileChange {
                path: path.into(),
                status: "A".into(),
                untracked: true,
            });
        }
        if files.len() > 50_000 {
            return Err("Le dépôt dépasse la limite de 50 000 fichiers de l’explorateur.".into());
        }
        files.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(ChangeListing { base, files })
    }

    pub async fn file_diff(
        self,
        path: String,
        base_commit: Option<&str>,
        untracked: Option<bool>,
    ) -> Result<FileDiff> {
        validate_path(&path, cfg!(windows))?;
        let changes = self.changes(base_commit).await?;
        let entry = changes
            .files
            .iter()
            .find(|entry| {
                entry.path == path && untracked.is_none_or(|expected| expected == entry.untracked)
            })
            .ok_or("Ce fichier ne fait plus partie des changements. Actualise la liste.")?;
        let mut result = FileDiff {
            path: path.clone(),
            patch: None,
            unavailable: None,
        };
        if entry.untracked {
            let preview = self.preview(path).await?;
            result.unavailable = preview.unavailable;
            if let Some(content) = preview.content {
                let lines = content.split_inclusive('\n').collect::<Vec<_>>();
                let mut patch = format!("@@ -0,0 +1,{} @@\n", lines.len());
                for line in lines {
                    patch.push('+');
                    patch.push_str(line);
                }
                if !content.is_empty() && !content.ends_with('\n') {
                    patch.push_str("\n\\ No newline at end of file\n");
                }
                if patch.len() > PREVIEW_LIMIT {
                    result.unavailable =
                        Some("Différence trop volumineuse pour l’aperçu (512 Kio maximum).".into());
                } else {
                    result.patch = Some(if content.is_empty() {
                        String::new()
                    } else {
                        patch
                    });
                }
            }
        } else {
            let patch = self
                .diff(&["--no-color", "--unified=3", &changes.base, "--", &path])
                .await?;
            if patch.len() > PREVIEW_LIMIT {
                result.unavailable =
                    Some("Différence trop volumineuse pour l’aperçu (512 Kio maximum).".into());
            } else {
                match String::from_utf8(patch) {
                    Ok(patch) => result.patch = Some(patch),
                    Err(_) => {
                        result.unavailable =
                            Some("Encodage non UTF-8 : aperçu textuel indisponible.".into())
                    }
                }
            }
        }
        Ok(result)
    }

    async fn diff(&self, arguments: &[&str]) -> Result<Vec<u8>> {
        let paths = self.list().await?.files;
        let directory = self.directory.try_clone()?;
        tokio::task::spawn_blocking(move || check_diff_paths(directory, paths)).await??;
        // Git diff can invoke clean/process filters even when external diff/textconv are disabled.
        let config = self
            .git(&["config", "--includes", "--null", "--name-only", "--list"])
            .await?;
        let config = String::from_utf8(config)?;
        let mut options = Vec::new();
        for key in config
            .split('\0')
            .collect::<std::collections::BTreeSet<_>>()
        {
            let lower = key.to_ascii_lowercase();
            if lower.starts_with("filter.")
                && [".clean", ".smudge", ".process", ".required"]
                    .iter()
                    .any(|suffix| lower.ends_with(suffix))
            {
                if key.contains('=') {
                    return Err(
                        "Configuration de filtre Git incompatible avec l’aperçu sécurisé.".into(),
                    );
                }
                options.push("-c".to_owned());
                options.push(format!(
                    "{key}={}",
                    if lower.ends_with(".required") {
                        "false"
                    } else {
                        ""
                    }
                ));
            }
        }
        let mut args = options.iter().map(String::as_str).collect::<Vec<_>>();
        args.extend([
            "-c",
            "diff.submodule=short",
            "diff",
            "--no-ext-diff",
            "--no-textconv",
            "--no-renames",
            "--ignore-submodules=dirty",
        ]);
        args.extend_from_slice(arguments);
        self.git(&args).await
    }

    pub async fn preview(self, path: String) -> Result<FilePreview> {
        validate_path(&path, cfg!(windows))?;
        if !self.list().await?.files.contains(&path) {
            return Err("Fichier absent du projet ou ignoré par Git.".into());
        }
        tokio::task::spawn_blocking(move || self.read(path)).await?
    }

    fn read(self, path: String) -> Result<FilePreview> {
        let mut directory = self.directory;
        let parts = path.split('/').collect::<Vec<_>>();
        for part in &parts[..parts.len() - 1] {
            require_plain(&directory.symlink_metadata(part)?)?;
            directory = directory.open_dir_nofollow(part)?;
        }
        let name = PathBuf::from(*parts.last().ok_or("Chemin de fichier invalide.")?);
        let metadata = directory.symlink_metadata(&name)?;
        require_plain(&metadata)?;
        let mut preview = FilePreview {
            path,
            content: None,
            unavailable: None,
        };
        if !metadata.is_file() {
            return Err("Ce chemin ne désigne pas un fichier ordinaire.".into());
        }
        if metadata.len() > PREVIEW_LIMIT as u64 {
            preview.unavailable =
                Some("Fichier trop volumineux pour l’aperçu (512 Kio maximum).".into());
        } else {
            let bytes = CheckedFile::open_in(&directory, &name)?.read(PREVIEW_LIMIT)?;
            if bytes.contains(&0) {
                preview.unavailable = Some("Fichier binaire : aperçu textuel indisponible.".into());
            } else {
                match String::from_utf8(bytes) {
                    Ok(content) => preview.content = Some(content),
                    Err(_) => {
                        preview.unavailable =
                            Some("Encodage non UTF-8 : aperçu textuel indisponible.".into())
                    }
                }
            }
        }
        Ok(preview)
    }

    #[cfg(windows)]
    async fn git(&self, arguments: &[&str]) -> Result<Vec<u8>> {
        let mut command = tokio::process::Command::new("git");
        for (key, _) in std::env::vars_os() {
            if key
                .to_string_lossy()
                .to_ascii_uppercase()
                .starts_with("GIT_")
            {
                command.env_remove(key);
            }
        }
        // Synthetic inspection never loads personal Git configuration or fetches missing objects.
        command
            .current_dir(&self.path)
            .env("GIT_CONFIG_SYSTEM", "NUL")
            .env("GIT_CONFIG_GLOBAL", "NUL")
            .env("GIT_TERMINAL_PROMPT", "0")
            .args([
                "--no-pager",
                "--no-optional-locks",
                "--literal-pathspecs",
                "--no-lazy-fetch",
                "--no-replace-objects",
                "-c",
                "core.fsmonitor=false",
                "-c",
                "core.excludesFile=NUL",
            ])
            .args(arguments);
        let output = super::process_tree::capture(
            &mut command,
            &[],
            std::time::Duration::from_secs(15),
            4 * 1024 * 1024,
        )
        .await?;
        if !output.status.success() {
            return Err(
                "Lecture Git impossible. Vérifie le dépôt et l’installation de Git.".into(),
            );
        }
        Ok(output.stdout)
    }

    #[cfg(not(windows))]
    async fn git(&self, _arguments: &[&str]) -> Result<Vec<u8>> {
        Err("L’explorateur Git natif attend sa qualification sur ce système.".into())
    }
}

fn validate_commit(commit: &str) -> Result<()> {
    if !matches!(commit.len(), 40 | 64) || !commit.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("Commit de départ invalide.".into());
    }
    Ok(())
}

fn check_diff_paths(root: Dir, paths: Vec<String>) -> Result<()> {
    for path in paths {
        validate_path(&path, cfg!(windows))?;
        let mut directory = root.try_clone()?;
        let mut parts = path.split('/').peekable();
        while let Some(part) = parts.next() {
            let metadata = match directory.symlink_metadata(part) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
                Err(error) => return Err(error.into()),
            };
            require_plain(&metadata)?;
            if parts.peek().is_some() {
                directory = directory.open_dir_nofollow(part)?;
            }
        }
    }
    Ok(())
}
