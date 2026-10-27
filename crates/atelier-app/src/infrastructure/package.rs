use crate::{
    Result,
    http::wasm_asset_name,
    infrastructure::checked_file::{CheckedFile, Identity, plain_metadata},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, fs, io::Write, path::Path};

pub const TARGETS: &[&str] = &[
    "x86_64-pc-windows-msvc",
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
    "x86_64-apple-darwin",
    "aarch64-apple-darwin",
];
pub fn binary_name(target: &str) -> Result<&'static str> {
    if !TARGETS.contains(&target) {
        return Err("Cible de paquet Rust non prise en charge.".into());
    }
    Ok(if target.contains("windows") {
        "atelier.exe"
    } else {
        "atelier"
    })
}
pub fn read(path: &Path, limit: usize) -> Result<Vec<u8>> {
    if !plain_metadata(path)?.is_file() {
        return Err("Fichier de paquet régulier requis.".into());
    }
    CheckedFile::open(path, &Identity::inspect(path)?)?.read(limit)
}
pub fn executable(target: &str, bytes: &[u8]) -> Result<()> {
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
pub fn write(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}
pub fn publish_directory(source: &Path, destination: &Path) -> Result<()> {
    let parent = |path: &Path| {
        fs::canonicalize(
            path.parent()
                .filter(|path| !path.as_os_str().is_empty())
                .unwrap_or(Path::new(".")),
        )
    };
    let directory = parent(source)?;
    if directory != parent(destination)? || !plain_metadata(source)?.is_dir() {
        return Err("La publication exige un dossier de staging voisin de la destination.".into());
    }
    let directory = cap_std::fs::Dir::open_ambient_dir(directory, cap_std::ambient_authority())?;
    super::publish::rename_new(
        &directory,
        Path::new(source.file_name().ok_or("Nom de staging absent.")?),
        Path::new(destination.file_name().ok_or("Nom de livraison absent.")?),
    )
}
pub fn assets(directory: &Path) -> Result<Vec<String>> {
    if !plain_metadata(directory)?.is_dir() {
        return Err("Dossier d’assets ordinaire requis.".into());
    }
    let directory = cap_std::fs::Dir::open_ambient_dir(directory, cap_std::ambient_authority())?;
    let names = asset_names(&directory)?;
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

/// Shared by HTTP capture and packaging. Trunk's live lock is never served,
/// packaged, removed or acquired here; only its exact empty plain-file form is accepted.
pub(crate) fn asset_names(directory: &cap_std::fs::Dir) -> Result<Vec<String>> {
    let mut names = Vec::new();
    for (index, entry) in directory.entries()?.enumerate() {
        if index >= 129 {
            return Err("Trop d’assets Rust à charger.".into());
        }
        let name = entry?
            .file_name()
            .into_string()
            .map_err(|_| "Nom d’asset Rust invalide.")?;
        if name == ".trunk-lock" {
            CheckedFile::open_in(directory, Path::new(&name))?
                .single_link()?
                .read(0)?;
            continue;
        }
        if !wasm_asset_name(&name) {
            return Err("Asset inattendu : les paquets Rust refusent les fichiers legacy, données et runtime Node.".into());
        }
        CheckedFile::open_in(directory, Path::new(&name))?.single_link()?;
        if names.len() >= 128 {
            return Err("Trop d’assets Rust à charger.".into());
        }
        names.push(name);
    }
    names.sort();
    Ok(names)
}
pub fn version_parts(value: &str) -> Result<[u64; 3]> {
    if value.len() > 64 {
        return Err("Version de paquet trop longue.".into());
    }
    let parts = value
        .split('.')
        .map(|part| {
            if part.is_empty()
                || part.len() > 1 && part.starts_with('0')
                || !part.bytes().all(|byte| byte.is_ascii_digit())
            {
                return Err("Version de paquet invalide.".into());
            }
            part.parse::<u64>().map_err(Into::into)
        })
        .collect::<Result<Vec<_>>>()?;
    parts
        .try_into()
        .map_err(|_| "Version de paquet à trois nombres requise.".into())
}

pub fn native_target() -> Result<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => Ok(TARGETS[0]),
        ("linux", "x86_64") => Ok(TARGETS[1]),
        ("linux", "aarch64") => Ok(TARGETS[2]),
        ("macos", "x86_64") => Ok(TARGETS[3]),
        ("macos", "aarch64") => Ok(TARGETS[4]),
        _ => Err("Plateforme native de paquet non prise en charge.".into()),
    }
}

pub fn manifest(directory: &Path, target: &str, version: &str) -> Result<Value> {
    version_parts(version)?;
    let binary = binary_name(target)?;
    let expected = BTreeSet::from([
        binary,
        "ui",
        "LICENSE",
        "README.txt",
        "THIRD-PARTY-NOTICES.txt",
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
    match fs::symlink_metadata(directory.join("THIRD-PARTY-NOTICES.txt")) {
        Ok(_) => files.push("THIRD-PARTY-NOTICES.txt".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    files.extend(
        assets(&directory.join("ui"))?
            .into_iter()
            .map(|name| format!("ui/{name}")),
    );
    files.sort();
    let mut total = 0usize;
    let mut inventory = Vec::new();
    for path in files {
        let limit = if path == binary {
            128 * 1024 * 1024
        } else if path == "THIRD-PARTY-NOTICES.txt" {
            32 * 1024 * 1024
        } else if path.starts_with("ui/") {
            64 * 1024 * 1024
        } else {
            1024 * 1024
        };
        let bytes = read(&directory.join(&path), limit)?;
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
            json!({"path":path,"size":bytes.len(),"sha256":crate::infrastructure::session::hexadecimal(&Sha256::digest(&bytes))}),
        );
    }
    Ok(
        json!({"schemaVersion":1,"application":"atelier-kanban-rust","version":version,"target":target,"files":inventory}),
    )
}
pub fn verify(directory: &Path) -> Result<()> {
    if !plain_metadata(directory)?.is_dir() {
        return Err("Dossier de paquet ordinaire requis.".into());
    }
    let recorded: Value =
        serde_json::from_slice(&read(&directory.join("atelier-package.json"), 1024 * 1024)?)?;
    let target = recorded["target"]
        .as_str()
        .ok_or("Cible de paquet absente.")?;
    let version = recorded["version"]
        .as_str()
        .ok_or("Version de paquet absente.")?;
    if manifest(directory, target, version)? != recorded {
        return Err("Le contenu du paquet ne correspond pas à son manifeste.".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packaging_excludes_only_the_empty_plain_trunk_marker_and_still_requires_a_complete_bundle() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("index.html"), "Synthetic Rust").unwrap();
        fs::write(root.path().join(".trunk-lock"), []).unwrap();
        assert!(assets(root.path()).is_err());
        for name in [
            "atelier-loader.js",
            "atelier-ui-synthetic.js",
            "atelier-ui-synthetic.wasm",
            "style-synthetic.css",
        ] {
            fs::write(root.path().join(name), []).unwrap();
        }
        let names = assets(root.path()).unwrap();
        assert_eq!(names.len(), 5);
        assert!(!names.iter().any(|name| name == ".trunk-lock"));
        assert_eq!(
            fs::read(root.path().join(".trunk-lock")).unwrap(),
            Vec::<u8>::new()
        );
        fs::write(root.path().join(".trunk-lock"), b"unexpected").unwrap();
        assert!(assets(root.path()).is_err());
        fs::write(root.path().join(".trunk-lock"), []).unwrap();
        fs::write(root.path().join("unknown.txt"), []).unwrap();
        assert!(assets(root.path()).is_err());
    }
}
