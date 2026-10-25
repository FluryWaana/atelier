use atelier_app::{
    Result,
    http::wasm_asset_name,
    infrastructure::checked_file::{CheckedFile, Identity, plain_metadata},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, fs, io::Write, path::Path};

const TARGETS: &[&str] = &[
    "x86_64-pc-windows-msvc",
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
    "x86_64-apple-darwin",
    "aarch64-apple-darwin",
];
fn binary_name(target: &str) -> Result<&'static str> {
    if !TARGETS.contains(&target) {
        return Err("Cible de paquet Rust non prise en charge.".into());
    }
    Ok(if target.contains("windows") {
        "atelier.exe"
    } else {
        "atelier"
    })
}
fn read(path: &Path, limit: usize) -> Result<Vec<u8>> {
    if !plain_metadata(path)?.is_file() {
        return Err("Fichier de paquet régulier requis.".into());
    }
    CheckedFile::open(path, &Identity::inspect(path)?)?.read(limit)
}
fn executable(target: &str, bytes: &[u8]) -> Result<()> {
    binary_name(target)?;
    let valid = if target.contains("windows") {
        if !bytes.starts_with(b"MZ") {
            false
        } else {
            let offset = bytes
                .get(60..64)
                .map(|value| u32::from_le_bytes(value.try_into().expect("four bytes")) as usize);
            offset
                .and_then(|offset| offset.checked_add(6).and_then(|end| bytes.get(offset..end)))
                .is_some_and(|header| header == b"PE\0\0\x64\x86")
        }
    } else if target.contains("linux") {
        bytes.starts_with(b"\x7fELF\x02\x01")
            && bytes.get(18..20)
                == Some(if target.starts_with("aarch64") {
                    &[183, 0][..]
                } else {
                    &[62, 0][..]
                })
    } else {
        bytes.starts_with(b"\xcf\xfa\xed\xfe")
            && bytes.get(4..8)
                == Some(if target.starts_with("aarch64") {
                    &[12, 0, 0, 1][..]
                } else {
                    &[7, 0, 0, 1][..]
                })
    };
    if !valid {
        return Err(
            "Le binaire ne correspond pas à la plateforme et à l’architecture du paquet.".into(),
        );
    }
    Ok(())
}
fn write(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}
fn assets(directory: &Path) -> Result<Vec<String>> {
    if !plain_metadata(directory)?.is_dir() {
        return Err("Dossier d’assets ordinaire requis.".into());
    }
    let mut names = Vec::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| "Nom d’asset invalide.")?;
        if !wasm_asset_name(&name) || !plain_metadata(&entry.path())?.is_file() {
            return Err("Asset inattendu : les paquets Rust refusent les fichiers legacy, données et runtime Node.".into());
        }
        names.push(name);
        if names.len() > 128 {
            return Err("Trop d’assets dans le paquet.".into());
        }
    }
    names.sort();
    if !["index.html", "atelier-loader.js"]
        .iter()
        .all(|name| names.iter().any(|value| value.as_str() == *name))
        || !names
            .iter()
            .any(|name| name.starts_with("atelier-ui-") && name.ends_with(".js"))
        || !names.iter().any(|name| name.ends_with(".wasm"))
        || !names.iter().any(|name| name.ends_with(".css"))
    {
        return Err("Interface WASM construite et complète requise pour le paquet.".into());
    }
    Ok(names)
}
fn manifest(directory: &Path, target: &str) -> Result<Value> {
    let binary = binary_name(target)?;
    let expected = BTreeSet::from([
        binary,
        "ui",
        "LICENSE",
        "README.txt",
        "atelier-package.json",
    ]);
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_str().ok_or("Nom de paquet invalide.")?;
        if !expected.contains(name) {
            return Err("Fichier inattendu dans le paquet Rust.".into());
        }
    }
    let mut files = vec![binary.to_owned(), "LICENSE".into(), "README.txt".into()];
    files.extend(
        assets(&directory.join("ui"))?
            .into_iter()
            .map(|name| format!("ui/{name}")),
    );
    files.sort();
    let mut total = 0usize;
    let mut inventory = Vec::new();
    for path in files {
        let bytes = read(&directory.join(&path), 128 * 1024 * 1024)?;
        total = total
            .checked_add(bytes.len())
            .ok_or("Paquet trop volumineux.")?;
        if total > 256 * 1024 * 1024 {
            return Err("Paquet trop volumineux.".into());
        }
        if path == binary {
            executable(target, &bytes)?;
        }
        if path.ends_with(".wasm") && !bytes.starts_with(b"\0asm\x01\0\0\0") {
            return Err("Module WASM invalide dans le paquet.".into());
        }
        inventory.push(
            json!({"path":path,"size":bytes.len(),"sha256":format!("{:x}",Sha256::digest(&bytes))}),
        );
    }
    Ok(
        json!({"schemaVersion":1,"application":"atelier-kanban-rust","version":env!("CARGO_PKG_VERSION"),"target":target,"files":inventory}),
    )
}
pub(super) fn verify(directory: &Path) -> Result<()> {
    if !plain_metadata(directory)?.is_dir() {
        return Err("Dossier de paquet ordinaire requis.".into());
    }
    let recorded: Value =
        serde_json::from_slice(&read(&directory.join("atelier-package.json"), 1024 * 1024)?)?;
    let target = recorded["target"]
        .as_str()
        .ok_or("Cible de paquet absente.")?;
    if manifest(directory, target)? != recorded {
        return Err("Le contenu du paquet ne correspond pas à son manifeste.".into());
    }
    Ok(())
}
pub(super) fn stage(target: &str, binary: &Path, ui: &Path, output: &Path) -> Result<()> {
    let name = binary_name(target)?;
    if binary.file_name().and_then(|name| name.to_str()) != Some(name) {
        return Err("Le staging exige le binaire applicatif nommé atelier, jamais un exécutable de faisabilité.".into());
    }
    let binary = read(binary, 128 * 1024 * 1024)?;
    executable(target, &binary)?;
    if output.try_exists()? {
        return Err("Le dossier de paquet existe déjà. Aucun fichier remplacé.".into());
    }
    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    if !plain_metadata(parent)?.is_dir() {
        return Err("Dossier de sortie ordinaire requis.".into());
    }
    let temporary = tempfile::Builder::new()
        .prefix(".atelier-package-")
        .tempdir_in(parent)?;
    write(&temporary.path().join(name), &binary)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            temporary.path().join(name),
            fs::Permissions::from_mode(0o755),
        )?;
    }
    fs::create_dir(temporary.path().join("ui"))?;
    for asset in assets(ui)? {
        write(
            &temporary.path().join("ui").join(&asset),
            &read(&ui.join(&asset), 64 * 1024 * 1024)?,
        )?;
    }
    write(
        &temporary.path().join("LICENSE"),
        &read(&super::root().join("LICENSE"), 1024 * 1024)?,
    )?;
    write(&temporary.path().join("README.txt"),b"Atelier Kanban Rust - preproduction payload\nNative binary and Leptos WASM assets. No Node runtime.\nThis candidate payload does not qualify the launcher, installer or updater.\nFor isolated qualification only: atelier serve-prototype --data-dir EMPTY_SYNTHETIC_DIRECTORY --assets ui\n")?;
    let value = manifest(temporary.path(), target)?;
    write(
        &temporary.path().join("atelier-package.json"),
        &serde_json::to_vec_pretty(&value)?,
    )?;
    verify(temporary.path())?;
    if output.try_exists()? {
        return Err("Le dossier de paquet existe déjà. Aucun fichier remplacé.".into());
    }
    fs::rename(temporary.path(), output)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
        let root = tempfile::tempdir().unwrap();
        let binary = root.path().join("atelier");
        let mut bytes = vec![0; 64];
        bytes[..6].copy_from_slice(b"\x7fELF\x02\x01");
        bytes[18] = 62;
        fs::write(&binary, bytes).unwrap();
        let ui = root.path().join("assets");
        fs::create_dir(&ui).unwrap();
        for (name, content) in [
            ("index.html", &b"synthetic index"[..]),
            ("atelier-loader.js", &b"synthetic loader"[..]),
            ("atelier-ui-test.js", &b"synthetic bindings"[..]),
            ("atelier-ui-test_bg.wasm", &b"\0asm\x01\0\0\0"[..]),
            ("style-test.css", &b"synthetic style"[..]),
        ] {
            fs::write(ui.join(name), content).unwrap();
        }
        (root, binary, ui)
    }
    #[test]
    fn staging_rejects_node_unexpected_data_tampering_wrong_architecture_and_overwrite() {
        let (root, binary, ui) = fixture();
        let output = root.path().join("package");
        fs::write(ui.join("node.exe"), "never package").unwrap();
        assert!(stage(TARGETS[1], &binary, &ui, &output).is_err());
        assert!(!output.exists());
        fs::remove_file(ui.join("node.exe")).unwrap();
        assert!(stage(TARGETS[2], &binary, &ui, &output).is_err());
        stage(TARGETS[1], &binary, &ui, &output).unwrap();
        verify(&output).unwrap();
        assert!(stage(TARGETS[1], &binary, &ui, &output).is_err());
        fs::write(output.join("ui/style-test.css"), "changed").unwrap();
        assert!(verify(&output).is_err());
        for name in [
            "app.js",
            "server.mjs",
            "node.exe",
            "../index.html",
            "runtime/node",
            ".env",
            "kanban.sqlite",
        ] {
            assert!(!wasm_asset_name(name));
        }
    }
}
