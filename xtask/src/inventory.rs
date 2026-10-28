use crate::Result;
use std::{fs, io::Write, path::Path};

const ROOTS: &[&str] = &["crates", "xtask", "scripts", ".github", ".cargo"];
const FILES: &[&str] = &[
    "Cargo.toml",
    "Cargo.lock",
    "rust-toolchain.toml",
    "README.md",
    "Atelier.cmd",
    "AGENTS.md",
    "LICENSE",
    ".gitignore",
    ".gitattributes",
];
const EXCLUDED: &[&str] = &["target", "data", ".artifacts", ".git", "runtime"];
const GROUPS: &[&str] = &[
    "crates/atelier-core",
    "crates/atelier-app",
    "crates/atelier-ui",
    "xtask",
    "scripts",
    ".github",
    ".cargo",
    "racine",
];
const HEADER: &str = "# Inventaire des sources Rust\n\nGénéré par `cargo xtask inventory`. Vérification sans écriture : `cargo xtask inventory --check`.\n\nCartographie lexicale des fichiers actuels, sans analyse des appels ni preuve de compilation, de couverture ou de réussite. Les références fonctionnelles de l’ancien projet se consultent dans Git au commit `7f48996` ; elles ne font pas partie du projet actif.\n\nPérimètre : `crates/`, `xtask/`, `scripts/`, `.github/`, `.cargo/` et les fichiers racine Cargo, toolchain, README, AGENTS, LICENSE et règles Git. Les segments `target`, `data`, `.artifacts`, `.git` et `runtime` sont exclus ; les liens et points de réanalyse sont refusés. Aucune donnée utilisateur ni contenu de source n’est lu.\n\nLe plan et les capacités restent décrits dans [rust-migration-plan.md](rust-migration-plan.md) et [rust-rebuild-features.md](rust-rebuild-features.md).\n\n## Répartition\n\n| Périmètre | Fichiers |\n| --- | ---: |\n";

fn ordinary(path: &Path) -> Result<fs::Metadata> {
    let metadata = fs::symlink_metadata(path)?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err("Point de réanalyse refusé dans l’inventaire.".into());
        }
    }
    if metadata.file_type().is_symlink() || (!metadata.is_file() && !metadata.is_dir()) {
        return Err("Lien ou fichier spécial refusé dans l’inventaire.".into());
    }
    Ok(metadata)
}

fn collect(
    root: &Path,
    relative: &str,
    depth: usize,
    visited: &mut usize,
    files: &mut Vec<String>,
) -> Result<()> {
    if depth > 24 || *visited >= 30_000 || files.len() >= 20_000 {
        return Err("Inventaire trop profond ou trop volumineux.".into());
    }
    *visited += 1;
    let metadata = ordinary(&root.join(relative))?;
    if metadata.is_file() {
        files.push(relative.to_owned());
    } else {
        for entry in fs::read_dir(root.join(relative))? {
            let entry = entry?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| "Nom d’inventaire non UTF-8.")?;
            if EXCLUDED.contains(&name.as_str()) {
                continue;
            }
            if name.is_empty()
                || !name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
            {
                return Err("Nom non pris en charge par les liens d’inventaire.".into());
            }
            collect(
                root,
                &format!("{relative}/{name}"),
                depth + 1,
                visited,
                files,
            )?;
        }
    }
    Ok(())
}

fn group(path: &str) -> &'static str {
    GROUPS
        .iter()
        .copied()
        .find(|group| {
            path.strip_prefix(*group)
                .is_some_and(|tail| tail.starts_with('/'))
        })
        .unwrap_or("racine")
}
fn kind(path: &str) -> &'static str {
    if path.contains("/tests/") || path.contains("/examples/") || path.starts_with("xtask/tests/") {
        "Test / support"
    } else if path.ends_with(".rs") {
        "Source Rust"
    } else if path.starts_with("scripts/") || path.starts_with("xtask/packaging/") {
        "Outil natif / fabrication"
    } else if path.ends_with(".toml")
        || path.ends_with(".lock")
        || path.starts_with(".cargo/")
        || path.starts_with(".github/")
        || path.starts_with(".git")
    {
        "Configuration"
    } else {
        "Ressource / documentation"
    }
}

fn render(root: &Path) -> Result<String> {
    if !ordinary(root)?.is_dir() {
        return Err("Racine d’inventaire ordinaire requise.".into());
    }
    let mut files = vec![];
    let mut visited = 0;
    for entry in ROOTS.iter().chain(FILES) {
        collect(root, entry, 0, &mut visited, &mut files)?;
    }
    files.sort();
    let mut output = HEADER.to_owned();
    for name in GROUPS {
        let count = files.iter().filter(|file| group(file) == *name).count();
        output.push_str(&format!("| {name} | {count} |\n"));
    }
    output.push_str(&format!(
        "\nTotal : {} fichiers.\n\n## Fichiers\n\n| Fichier | Nature |\n| --- | --- |\n",
        files.len()
    ));
    for file in files {
        output.push_str(&format!("| [{file}](../{file}) | {} |\n", kind(&file)));
    }
    Ok(output)
}

pub(super) fn command(check: bool) -> Result<()> {
    let root = crate::root();
    let content = render(&root)?;
    let docs = root.join("docs");
    if !ordinary(&docs)?.is_dir() {
        return Err("Dossier de documentation ordinaire requis.".into());
    }
    let destination = docs.join("rust-migration-inventory.md");
    let metadata = ordinary(&destination)?;
    if !metadata.is_file() || metadata.len() > 8 * 1024 * 1024 {
        return Err("Inventaire existant invalide ou trop volumineux.".into());
    }
    if check {
        if fs::read_to_string(destination)?.replace("\r\n", "\n") != content {
            return Err("Inventaire périmé : régénère avec cargo xtask inventory.".into());
        }
    } else {
        let temporary = docs.join(format!(
            ".rust-migration-inventory-{}.tmp",
            std::process::id()
        ));
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)?;
        let result: Result<()> = (|| {
            file.write_all(content.as_bytes())?;
            file.sync_all()?;
            drop(file);
            fs::rename(&temporary, &destination)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(root: &Path) {
        for directory in ROOTS {
            fs::create_dir(root.join(directory)).unwrap();
        }
        for file in FILES {
            fs::write(root.join(file), "synthetic").unwrap();
        }
    }
    #[test]
    fn inventory_is_sorted_deterministic_and_does_not_enter_excluded_data() {
        let temporary = tempfile::tempdir().unwrap();
        fixture(temporary.path());
        fs::write(temporary.path().join("xtask/z.rs"), "synthetic").unwrap();
        fs::write(temporary.path().join("xtask/a.rs"), "synthetic").unwrap();
        fs::create_dir(temporary.path().join("xtask/data")).unwrap();
        fs::write(
            temporary.path().join("xtask/data/never-read.sqlite"),
            "synthetic",
        )
        .unwrap();
        let first = render(temporary.path()).unwrap();
        assert_eq!(first, render(temporary.path()).unwrap());
        assert!(first.find("[xtask/a.rs]").unwrap() < first.find("[xtask/z.rs]").unwrap());
        assert!(!first.contains("never-read.sqlite"));
    }
    #[cfg(unix)]
    #[test]
    fn source_links_are_refused_instead_of_followed() {
        let temporary = tempfile::tempdir().unwrap();
        fixture(temporary.path());
        std::os::unix::fs::symlink(
            temporary.path().join("README.md"),
            temporary.path().join("xtask/linked.rs"),
        )
        .unwrap();
        assert!(render(temporary.path()).is_err());
    }
}
