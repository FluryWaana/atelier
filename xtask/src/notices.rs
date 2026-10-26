use atelier_app::{
    Result,
    infrastructure::{package, process_tree},
};
use serde_json::Value;
use std::{collections::BTreeSet, fs, path::Path, time::Duration};
pub(super) const LIMIT: usize = 32 * 1024 * 1024;
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
        for entry in fs::read_dir(root)? {
            let entry = entry?;
            let file = entry.file_name();
            let upper = file.to_string_lossy().to_ascii_uppercase();
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
                && entry.file_type()?.is_file()
            {
                files.insert(file.into());
            }
        }
        if files.is_empty() {
            return Err(format!("Texte de licence manquant pour {name} {version}; compléter les avis avant le paquet.").into());
        }
        output.push_str(&format!(
            "===== {name} {version} =====\nLicense: {}\nSource: {source}\n",
            dependency["license"]
                .as_str()
                .unwrap_or("See bundled license text")
        ));
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
        assert!(text.contains("Synthetic license text"));
        assert!(!text.contains(&root.path().display().to_string()));
        metadata["packages"][0]["license_file"] = json!("../outside");
        assert!(render(&metadata).is_err());
        metadata["packages"][0]["license_file"] = Value::Null;
        metadata["packages"][0]["source"] = json!("git+https://synthetic.example.test/private");
        assert!(render(&metadata).is_err());
    }
}
