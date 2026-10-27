use atelier_app::{
    Result,
    infrastructure::{package, process_tree},
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, fs, path::Path, time::Duration};
pub(super) const LIMIT: usize = 32 * 1024 * 1024;
fn supplement(dependency: &Value, root: &Path) -> Result<Option<String>> {
    let manifests = [
        include_str!("../licenses/core-supplements.json"),
        include_str!("../licenses/leptos-supplements.json"),
        include_str!("../licenses/connector-supplements.json"),
        include_str!("../licenses/other-supplements.json"),
    ];
    let mut matched = None;
    for manifest in manifests {
        for entry in serde_json::from_str::<Vec<Value>>(manifest)? {
            if entry["name"] != dependency["name"] || entry["version"] != dependency["version"] {
                continue;
            }
            if matched.is_some()
                || entry["license"] != dependency["license"]
                || entry["repository"] != dependency["repository"]
            {
                return Err("Identité du complément de licence incompatible.".into());
            }
            let vcs: Value = serde_json::from_slice(&package::read(
                &root.join(".cargo_vcs_info.json"),
                1024 * 1024,
            )?)?;
            if vcs["git"]["sha1"] != entry["commit"] {
                return Err("Révision du complément de licence incompatible.".into());
            }
            let lock = include_str!("../../Cargo.lock");
            let name = format!("name = {}", serde_json::to_string(&entry["name"])?);
            let version = format!("version = {}", serde_json::to_string(&entry["version"])?);
            let checksum = format!("checksum = {}", serde_json::to_string(&entry["checksum"])?);
            if !lock.split("[[package]]").any(|block| {
                let lines = block.lines().collect::<BTreeSet<_>>();
                lines.contains(name.as_str())
                    && lines.contains(version.as_str())
                    && lines.contains(checksum.as_str())
            }) {
                return Err("Empreinte Cargo du complément de licence incompatible.".into());
            }
            let mut text = format!(
                "\nSelected license: {}\nUpstream revision: {}\n",
                entry["selectedLicense"]
                    .as_str()
                    .ok_or("Licence sélectionnée absente.")?,
                entry["commit"].as_str().ok_or("Révision absente.")?
            );
            let texts = entry["texts"]
                .as_array()
                .filter(|texts| !texts.is_empty())
                .ok_or("Textes complémentaires absents.")?;
            for item in texts {
                let relative = Path::new(item["file"].as_str().ok_or("Chemin de licence absent.")?);
                if relative
                    .components()
                    .any(|part| !matches!(part, std::path::Component::Normal(_)))
                {
                    return Err("Chemin de complément de licence invalide.".into());
                }
                let bytes = package::read(
                    &super::root().join("xtask/licenses").join(relative),
                    1024 * 1024,
                )?;
                if atelier_app::infrastructure::session::hexadecimal(&Sha256::digest(&bytes))
                    != item["sha256"].as_str().unwrap_or_default()
                {
                    return Err("Texte complémentaire de licence modifié.".into());
                }
                text.push_str(&format!(
                    "\n--- {} ---\nUpstream: {}\n{}\n",
                    relative.display(),
                    item["url"].as_str().ok_or("Provenance absente.")?,
                    std::str::from_utf8(&bytes)?
                ));
            }
            matched = Some(text);
        }
    }
    Ok(matched)
}
fn collect(
    root: &Path,
    relative: &Path,
    depth: usize,
    entries: &mut usize,
    files: &mut BTreeSet<std::path::PathBuf>,
) -> Result<()> {
    if depth > 32 {
        return Err("Arbre de licences trop profond.".into());
    }
    for entry in fs::read_dir(root.join(relative))? {
        let entry = entry?;
        *entries = entries
            .checked_add(1)
            .ok_or("Inventaire des licences excessif.")?;
        if *entries > 1_000_000 {
            return Err("Inventaire des licences excessif.".into());
        }
        let path = relative.join(entry.file_name());
        let metadata =
            atelier_app::infrastructure::checked_file::plain_metadata(&root.join(&path))?;
        if metadata.is_dir() {
            collect(root, &path, depth + 1, entries, files)?;
        } else if metadata.is_file() {
            let upper = entry.file_name().to_string_lossy().to_ascii_uppercase();
            if [
                "LICENSE",
                "LICENCE",
                "COPYING",
                "NOTICE",
                "UNLICENSE",
                "COPYRIGHT",
            ]
            .iter()
            .any(|prefix| upper.starts_with(prefix))
            {
                files.insert(path);
                if files.len() > 4096 {
                    return Err("Trop de textes de licences dans une dépendance.".into());
                }
            }
        } else {
            return Err("Fichier spécial interdit dans l’inventaire des licences.".into());
        }
    }
    Ok(())
}
fn render(metadata: &Value) -> Result<String> {
    let packages = metadata["packages"]
        .as_array()
        .ok_or("Métadonnées Cargo invalides.")?;
    let members = metadata["workspace_members"]
        .as_array()
        .ok_or("Workspace Cargo absent.")?
        .iter()
        .filter_map(Value::as_str)
        .collect::<BTreeSet<_>>();
    let mut packages = packages
        .iter()
        .filter(|value| !members.contains(value["id"].as_str().unwrap_or_default()))
        .collect::<Vec<_>>();
    packages.sort_by(|left, right| {
        left["name"]
            .as_str()
            .cmp(&right["name"].as_str())
            .then_with(|| left["version"].as_str().cmp(&right["version"].as_str()))
            .then_with(|| left["id"].as_str().cmp(&right["id"].as_str()))
    });
    if packages.is_empty() {
        return Err("Inventaire des dépendances vide.".into());
    }
    let mut output = String::from(
        "Atelier Rust third-party notices\nCargo.lock dependency inventory; includes build tools and all target platforms.\nLicense expressions are declarations from dependency metadata.\n\n",
    );
    output.push_str("===== Bootstrap 5.3.8 CSS =====\nLicense: MIT\nSource: Atelier historical stylesheet, Git 7f48996, public/style.css\n");
    output.push_str(include_str!("../licenses/bootstrap-5.3.8.txt"));
    output.push('\n');
    let mut entries = 0;
    for dependency in packages {
        let name = dependency["name"]
            .as_str()
            .ok_or("Nom de dépendance absent.")?;
        let version = dependency["version"]
            .as_str()
            .ok_or("Version de dépendance absente.")?;
        let source = dependency["source"]
            .as_str()
            .ok_or("Source de dépendance absente.")?;
        if source != "registry+https://github.com/rust-lang/crates.io-index"
            && source != "registry+https://index.crates.io/"
        {
            return Err(format!("Source non recensée pour {name} {version}.").into());
        }
        let root = Path::new(
            dependency["manifest_path"]
                .as_str()
                .ok_or("Manifeste de dépendance absent.")?,
        )
        .parent()
        .ok_or("Dossier de dépendance absent.")?;
        let mut files = BTreeSet::new();
        if let Some(file) = dependency["license_file"].as_str() {
            let file = Path::new(file);
            let file = if file.is_absolute() {
                file.strip_prefix(root)
                    .map_err(|_| "Licence extérieure au paquet Cargo.")?
            } else {
                file
            };
            if file
                .components()
                .any(|part| !matches!(part, std::path::Component::Normal(_)))
            {
                return Err("Chemin de licence invalide.".into());
            }
            files.insert(file.to_path_buf());
        }
        collect(root, Path::new(""), 0, &mut entries, &mut files)?;
        let supplemental = supplement(dependency, root)?;
        if files.is_empty() && supplemental.is_none() {
            return Err(format!("Texte de licence manquant pour {name} {version}; compléter les avis avant le paquet.").into());
        }
        output.push_str(&format!(
            "===== {name} {version} =====\nLicense: {}\nSource: {source}\nSource archive: https://static.crates.io/crates/{name}/{name}-{version}.crate\n",
            dependency["license"]
                .as_str()
                .unwrap_or("See bundled license text")
        ));
        if let Some(text) = supplemental {
            output.push_str(&text);
            if output.len() > LIMIT {
                return Err("Avis de licences trop volumineux.".into());
            }
        }
        for file in files {
            let bytes = package::read(&root.join(&file), 1024 * 1024)?;
            let text = std::str::from_utf8(&bytes).map_err(|_| "Texte de licence non UTF-8.")?;
            output.push_str(&format!("\n--- {} ---\n{text}\n", file.display()));
            if output.len() > LIMIT {
                return Err("Avis de licences trop volumineux.".into());
            }
        }
    }
    Ok(output)
}
pub(super) async fn generate(output: &Path) -> Result<()> {
    if output.try_exists()? {
        return Err("Les avis existent déjà. Aucun fichier remplacé.".into());
    }
    let mut command = tokio::process::Command::new("cargo");
    command.current_dir(super::root()).args([
        "metadata",
        "--locked",
        "--offline",
        "--format-version",
        "1",
    ]);
    let result = process_tree::capture(
        &mut command,
        &[],
        Duration::from_secs(120),
        64 * 1024 * 1024,
    )
    .await?;
    if !result.status.success() {
        return Err("Inventaire Cargo hors ligne impossible ; charger les dépendances verrouillées avant la campagne.".into());
    }
    let metadata: Value = serde_json::from_slice(&result.stdout)?;
    package::write(output, render(&metadata)?.as_bytes())
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn supplemental_notices_require_exact_locked_identity_and_revision() {
        let root = tempfile::tempdir().unwrap();
        let entries: Vec<Value> =
            serde_json::from_str(include_str!("../licenses/core-supplements.json")).unwrap();
        let entry = &entries[0];
        fs::write(
            root.path().join(".cargo_vcs_info.json"),
            serde_json::to_vec(&json!({"git":{"sha1":entry["commit"]}})).unwrap(),
        )
        .unwrap();
        let mut dependency = json!({"name":entry["name"],"version":entry["version"],"license":entry["license"],"repository":entry["repository"]});
        let text = supplement(&dependency, root.path()).unwrap().unwrap();
        assert!(text.contains("Upstream revision:"));
        assert!(text.contains("MIT License"));
        dependency["id"] = json!("synthetic-wrapper");
        dependency["source"] = json!("registry+https://github.com/rust-lang/crates.io-index");
        dependency["manifest_path"] = json!(root.path().join("Cargo.toml"));
        fs::write(root.path().join("LICENSE"), "Synthetic wrapper license.").unwrap();
        let rendered =
            render(&json!({"workspace_members":[],"packages":[dependency.clone()]})).unwrap();
        assert!(rendered.contains("Synthetic wrapper license."));
        assert!(rendered.contains("Upstream revision:"));
        dependency["license"] = json!("Unrecognised license");
        assert!(supplement(&dependency, root.path()).is_err());
        dependency["license"] = entry["license"].clone();
        fs::write(
            root.path().join(".cargo_vcs_info.json"),
            br#"{"git":{"sha1":"different-revision"}}"#,
        )
        .unwrap();
        assert!(supplement(&dependency, root.path()).is_err());
        dependency["version"] = json!("999.0.0");
        assert!(supplement(&dependency, root.path()).unwrap().is_none());
    }
    #[test]
    fn missing_license_text_and_foreign_paths_are_not_silently_omitted() {
        let root = tempfile::tempdir().unwrap();
        let mut metadata = json!({"workspace_members":[],"packages":[{"id":"synthetic","name":"synthetic-dependency","version":"1.0.0","source":"registry+https://github.com/rust-lang/crates.io-index","manifest_path":root.path().join("Cargo.toml"),"license":"MIT","license_file":null}]});
        assert!(render(&metadata).is_err());
        fs::write(
            root.path().join("LICENSE-MIT"),
            "Synthetic license text; not an actual dependency.",
        )
        .unwrap();
        let text = render(&metadata).unwrap();
        assert!(text.contains("synthetic-dependency 1.0.0"));
        assert!(text.contains("Bootstrap 5.3.8 CSS"));
        assert!(text.contains("Copyright (c) 2011-2025 The Bootstrap Authors"));
        assert!(text.contains("Synthetic license text"));
        assert!(!text.contains(&root.path().display().to_string()));
        let embedded = root.path().join("src/polyfill/synthetic-component");
        fs::create_dir_all(&embedded).unwrap();
        fs::write(
            embedded.join("LICENSE-APACHE"),
            "Synthetic embedded component notice.",
        )
        .unwrap();
        let text = render(&metadata).unwrap();
        assert!(text.contains("Synthetic embedded component notice."));
        assert_eq!(
            text.matches("Synthetic embedded component notice.").count(),
            1
        );
        metadata["packages"][0]["license_file"] = json!("../outside");
        assert!(render(&metadata).is_err());
        metadata["packages"][0]["license_file"] = Value::Null;
        metadata["packages"][0]["source"] = json!("git+https://synthetic.example.test/private");
        assert!(render(&metadata).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn embedded_license_discovery_never_follows_an_external_directory_link() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(
            outside.path().join("LICENSE"),
            "Synthetic outside content; preserve.",
        )
        .unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path().join("vendor")).unwrap();
        let mut files = BTreeSet::new();
        assert!(collect(root.path(), Path::new(""), 0, &mut 0, &mut files).is_err());
        assert!(files.is_empty());
        assert_eq!(
            fs::read_to_string(outside.path().join("LICENSE")).unwrap(),
            "Synthetic outside content; preserve."
        );
    }
}
