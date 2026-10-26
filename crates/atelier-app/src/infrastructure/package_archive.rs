use super::package;
use crate::Result;
use serde_json::Value;
use std::{
    collections::BTreeSet,
    fs,
    io::{Cursor, Read, Write},
    path::Path,
};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

pub const ARCHIVE_LIMIT: usize = 260 * 1024 * 1024;
const CONTENT_LIMIT: u64 = 257 * 1024 * 1024;
const ENTRY_LIMIT: usize = 133;

pub struct StagedPackage {
    directory: tempfile::TempDir,
    manifest: Value,
}
impl StagedPackage {
    pub fn path(&self) -> &Path {
        self.directory.path()
    }
    pub fn manifest(&self) -> &Value {
        &self.manifest
    }
    #[cfg(target_os = "linux")]
    pub(super) fn keep(self) -> std::path::PathBuf {
        self.directory.keep()
    }
}

fn footer(bytes: &[u8]) -> Result<(usize, u64)> {
    let invalid = || "Archive ZIP Rust non canonique ou trop volumineuse.";
    if bytes.len() < 22 || bytes.len() > ARCHIVE_LIMIT || !bytes.starts_with(b"PK\x03\x04") {
        return Err(invalid().into());
    }
    let end = &bytes[bytes.len() - 22..];
    let word = |at| u16::from_le_bytes([end[at], end[at + 1]]);
    let count = word(10) as usize;
    let size = u32::from_le_bytes(end[12..16].try_into()?) as u64;
    let offset = u32::from_le_bytes(end[16..20].try_into()?) as u64;
    if &end[..4] != b"PK\x05\x06"
        || word(4) != 0
        || word(6) != 0
        || word(8) as usize != count
        || word(20) != 0
        || count == 0
        || count > ENTRY_LIMIT
        || size > 1024 * 1024
        || offset.checked_add(size) != Some((bytes.len() - 22) as u64)
    {
        return Err(invalid().into());
    }
    Ok((count, offset))
}

fn name_allowed(name: &str, binary: &str) -> bool {
    name == binary
        || matches!(
            name,
            "LICENSE" | "README.txt" | "atelier-package.json" | "THIRD-PARTY-NOTICES.txt"
        )
        || name
            .strip_prefix("ui/")
            .is_some_and(crate::http::wasm_asset_name)
}

pub fn unpack(bytes: &[u8], target: &str, parent: &Path) -> Result<StagedPackage> {
    let binary = package::binary_name(target)?;
    let (count, central) = footer(bytes)?;
    let mut archive = ZipArchive::new(Cursor::new(bytes))?;
    if archive.len() != count {
        return Err("Entrées ZIP manquantes ou dupliquées.".into());
    }
    let mut names = BTreeSet::new();
    let mut total = 0u64;
    let mut ranges = Vec::new();
    for index in 0..archive.len() {
        let entry = archive.by_index(index)?;
        let limit = if entry.name() == "atelier-package.json"
            || matches!(entry.name(), "LICENSE" | "README.txt")
        {
            1024 * 1024
        } else if entry.name() == "THIRD-PARTY-NOTICES.txt" {
            32 * 1024 * 1024
        } else if entry.name().starts_with("ui/") {
            64 * 1024 * 1024
        } else {
            128 * 1024 * 1024
        };
        if !name_allowed(entry.name(), binary)
            || entry.name_raw() != entry.name().as_bytes()
            || !names.insert(entry.name().to_ascii_lowercase())
            || !entry.is_file()
            || entry.is_symlink()
            || entry.encrypted()
            || entry.compression() != CompressionMethod::Stored
            || entry.size() != entry.compressed_size()
            || entry.size() > limit
            || !entry.comment().is_empty()
            || entry
                .unix_mode()
                .is_some_and(|mode| mode & 0o170000 != 0o100000 || mode & 0o7000 != 0)
        {
            return Err(
                "Entrée de paquet ZIP refusée : chemin, type, taille ou compression.".into(),
            );
        }
        total = total
            .checked_add(entry.size())
            .filter(|size| *size <= CONTENT_LIMIT)
            .ok_or("Contenu ZIP trop volumineux.")?;
        let data = entry
            .data_start()
            .ok_or("Position de fichier ZIP absente.")?;
        let end = data
            .checked_add(entry.size())
            .ok_or("Position ZIP invalide.")?;
        if data < entry.header_start() || end > central {
            return Err("Étendue ZIP hors du contenu du paquet.".into());
        }
        ranges.push((entry.header_start(), end));
    }
    ranges.sort_unstable();
    let mut expected = 0;
    for (start, end) in ranges {
        if start != expected {
            return Err("Contenu ZIP superposé ou non référencé.".into());
        }
        expected = end;
    }
    if expected != central {
        return Err("Contenu ZIP non référencé avant le manifeste central.".into());
    }
    if !names.contains("atelier-package.json") {
        return Err("Manifeste du paquet ZIP absent.".into());
    }
    if !super::checked_file::plain_metadata(parent)?.is_dir() {
        return Err("Dossier temporaire de paquet requis.".into());
    }
    let directory = tempfile::Builder::new()
        .prefix(".atelier-unpack-")
        .tempdir_in(parent)?;
    fs::create_dir(directory.path().join("ui"))?;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let path = directory.path().join(entry.name());
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        let expected = entry.size();
        let written = std::io::copy(&mut entry.by_ref().take(expected + 1), &mut output)?;
        if written != expected {
            return Err("Taille du fichier extrait incohérente.".into());
        }
        output.sync_all()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(
                &path,
                fs::Permissions::from_mode(if entry.name() == binary { 0o755 } else { 0o644 }),
            )?;
        }
    }
    package::verify(directory.path())?;
    let manifest: Value = serde_json::from_slice(&package::read(
        &directory.path().join("atelier-package.json"),
        1024 * 1024,
    )?)?;
    if manifest["target"] != target {
        return Err("Le paquet ZIP ne correspond pas à la cible demandée.".into());
    }
    Ok(StagedPackage {
        directory,
        manifest,
    })
}

pub fn create(directory: &Path, output: &Path) -> Result<()> {
    package::verify(directory)?;
    let original = package::read(&directory.join("atelier-package.json"), 1024 * 1024)?;
    let manifest: Value = serde_json::from_slice(&original)?;
    let target = manifest["target"]
        .as_str()
        .ok_or("Cible du paquet absente.")?;
    let binary = package::binary_name(target)?;
    if output.try_exists()? {
        return Err("L’archive existe déjà. Aucun fichier remplacé.".into());
    }
    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    if !super::checked_file::plain_metadata(parent)?.is_dir() {
        return Err("Dossier de sortie de paquet ordinaire requis.".into());
    }
    let mut temporary = tempfile::Builder::new()
        .prefix(".atelier-archive-")
        .tempfile_in(parent)?;
    {
        let mut archive = ZipWriter::new(temporary.as_file_mut());
        let files = manifest["files"]
            .as_array()
            .ok_or("Inventaire de paquet absent.")?;
        for record in files {
            let name = record["path"].as_str().ok_or("Chemin de paquet absent.")?;
            if !name_allowed(name, binary) {
                return Err("Chemin de paquet ZIP invalide.".into());
            }
            let bytes = package::read(&directory.join(name), 128 * 1024 * 1024)?;
            archive.start_file(
                name,
                SimpleFileOptions::default()
                    .compression_method(CompressionMethod::Stored)
                    .unix_permissions(if name == binary { 0o755 } else { 0o644 }),
            )?;
            archive.write_all(&bytes)?;
        }
        archive.start_file(
            "atelier-package.json",
            SimpleFileOptions::default()
                .compression_method(CompressionMethod::Stored)
                .unix_permissions(0o644),
        )?;
        archive.write_all(&original)?;
        archive.finish()?.sync_all()?;
    }
    let bytes = package::read(temporary.path(), ARCHIVE_LIMIT)?;
    let scratch = tempfile::tempdir()?;
    let staged = unpack(&bytes, target, scratch.path())?;
    if staged.manifest() != &manifest {
        return Err("Le contenu de l’archive a changé pendant sa création.".into());
    }
    package::verify(directory)?;
    if package::read(&directory.join("atelier-package.json"), 1024 * 1024)? != original {
        return Err("Le manifeste source a changé pendant l’archivage.".into());
    }
    temporary.persist_noclobber(output)?;
    Ok(())
}

pub fn verify(archive: &Path, target: &str) -> Result<()> {
    let bytes = package::read(archive, ARCHIVE_LIMIT)?;
    let scratch = tempfile::tempdir()?;
    unpack(&bytes, target, scratch.path())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn archive(names: &[&str]) -> Vec<u8> {
        let mut archive = ZipWriter::new(Cursor::new(Vec::new()));
        for name in names {
            archive
                .start_file(
                    *name,
                    SimpleFileOptions::default()
                        .compression_method(CompressionMethod::Stored)
                        .unix_permissions(0o644),
                )
                .unwrap();
            archive.write_all(b"synthetic").unwrap();
        }
        archive.finish().unwrap().into_inner()
    }
    #[test]
    fn extraction_rejects_traversal_links_case_collisions_corruption_and_forged_counts() {
        let root = tempfile::tempdir().unwrap();
        for name in [
            "../outside",
            "/outside",
            "C:/outside",
            "ui/../outside",
            "ui/node.exe",
            "data/kanban.sqlite",
            "runtime/node",
            "atelier.exe",
        ] {
            let bytes = archive(&[name]);
            assert!(
                unpack(&bytes, package::TARGETS[1], root.path()).is_err(),
                "{name}"
            );
            assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
        }
        let bytes = archive(&["ui/atelier-ui-test.js", "ui/atelier-ui-TEST.js"]);
        assert!(unpack(&bytes, package::TARGETS[1], root.path()).is_err());
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        writer
            .add_symlink(
                "ui/index.html",
                "../../outside",
                SimpleFileOptions::default().compression_method(CompressionMethod::Stored),
            )
            .unwrap();
        let bytes = writer.finish().unwrap().into_inner();
        assert!(unpack(&bytes, package::TARGETS[1], root.path()).is_err());
        let valid = archive(&["atelier-package.json"]);
        assert_eq!(footer(&valid).unwrap().0, 1);
        let mut too_many = valid.clone();
        let end = too_many.len() - 22;
        too_many[end + 8..end + 12].copy_from_slice(&[255, 255, 255, 255]);
        assert!(footer(&too_many).is_err());
        let mut comment = valid.clone();
        comment.push(0);
        assert!(footer(&comment).is_err());
        let mut truncated = valid.clone();
        truncated.pop();
        assert!(footer(&truncated).is_err());
        let mut checksum = valid;
        checksum[30 + "atelier-package.json".len()] = b'X';
        assert!(unpack(&checksum, package::TARGETS[1], root.path()).is_err());
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
    }
    #[test]
    fn numeric_versions_are_bounded_and_compared_without_lexical_shortcuts() {
        assert!(
            package::version_parts("0.1.10").unwrap() > package::version_parts("0.1.9").unwrap()
        );
        for version in [
            "01.2.3",
            "1.2",
            "1.2.3.4",
            "1.2.3-beta",
            "1/2/3",
            "1.2.18446744073709551616",
            "",
        ] {
            assert!(package::version_parts(version).is_err(), "{version}");
        }
    }
}
