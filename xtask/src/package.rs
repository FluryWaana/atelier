pub(super) use atelier_app::infrastructure::package::verify;
use atelier_app::{
    Result,
    infrastructure::package::{assets, binary_name, executable, manifest, read, write},
};
#[cfg(test)]
use atelier_app::{http::wasm_asset_name, infrastructure::package::TARGETS};
use std::{fs, path::Path};
pub(super) fn stage(
    target: &str,
    binary: &Path,
    ui: &Path,
    notices: &Path,
    output: &Path,
) -> Result<()> {
    let name = binary_name(target)?;
    if binary.file_name().and_then(|name| name.to_str()) != Some(name) {
        return Err("Le staging exige le binaire applicatif nommé atelier, jamais un exécutable de faisabilité.".into());
    }
    let binary = read(binary, 128 * 1024 * 1024)?;
    let notices = read(notices, super::notices::LIMIT)?;
    if !notices.starts_with(b"Atelier Rust third-party notices\n") {
        return Err("Avis de licences Rust générés requis avant le paquet.".into());
    }
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
    write(&temporary.path().join("THIRD-PARTY-NOTICES.txt"), &notices)?;
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
    write(&temporary.path().join("README.txt"),b"Atelier Kanban Rust - preproduction payload\nNative binary and Leptos WASM assets. No Node runtime.\nNative launcher candidate; no launcher, installer or updater qualification is claimed.\nIsolated qualification only: atelier launch --data-dir EMPTY_SYNTHETIC_DIRECTORY --assets ui --no-browser\nStop with: atelier stop --data-dir EMPTY_SYNTHETIC_DIRECTORY\nUse atelier serve --data-dir EMPTY_SYNTHETIC_DIRECTORY --assets ui for foreground diagnostics.\n")?;
    let value = manifest(temporary.path(), target, env!("CARGO_PKG_VERSION"))?;
    write(
        &temporary.path().join("atelier-package.json"),
        &serde_json::to_vec_pretty(&value)?,
    )?;
    verify(temporary.path())?;
    if output.try_exists()? {
        return Err("Le dossier de paquet existe déjà. Aucun fichier remplacé.".into());
    }
    atelier_app::infrastructure::package::publish_directory(temporary.path(), output)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn publication_never_replaces_a_destination_created_after_staging() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("staged");
        let target = root.path().join("delivery");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("artifact"), "synthetic artifact").unwrap();
        fs::create_dir(&target).unwrap();
        fs::write(target.join("human-note"), "preserve").unwrap();
        assert!(atelier_app::infrastructure::package::publish_directory(&source, &target).is_err());
        assert_eq!(
            fs::read_to_string(target.join("human-note")).unwrap(),
            "preserve"
        );
        assert!(source.join("artifact").exists());
    }
    #[test]
    fn archives_round_trip_all_five_target_payloads_without_overwrite() {
        use atelier_app::infrastructure::package_archive;
        let (root, _, ui) = fixture();
        for (index, target) in TARGETS.iter().enumerate() {
            let source = root.path().join(format!("source-{index}"));
            fs::create_dir(&source).unwrap();
            let binary = source.join(binary_name(target).unwrap());
            let mut image = vec![0u8; 128];
            if target.contains("windows") {
                image[..2].copy_from_slice(b"MZ");
                image[60] = 64;
                image[64..70].copy_from_slice(b"PE\0\0\x64\x86");
            } else if target.contains("linux") {
                image[..6].copy_from_slice(b"\x7fELF\x02\x01");
                image[18] = if target.starts_with("aarch64") {
                    183
                } else {
                    62
                };
            } else {
                image[..4].copy_from_slice(b"\xcf\xfa\xed\xfe");
                image[4..8].copy_from_slice(if target.starts_with("aarch64") {
                    &[12, 0, 0, 1]
                } else {
                    &[7, 0, 0, 1]
                });
            }
            fs::write(&binary, &image).unwrap();
            let payload = root.path().join(format!("payload-{index}"));
            stage(
                target,
                &binary,
                &ui,
                &root.path().join("notices.txt"),
                &payload,
            )
            .unwrap();
            let archive = root.path().join(format!("payload-{index}.zip"));
            package_archive::create(&payload, &archive).unwrap();
            package_archive::verify(&archive, target).unwrap();
            assert!(package_archive::create(&payload, &archive).is_err());
            let bytes = fs::read(&archive).unwrap();
            let staged = package_archive::unpack(&bytes, target, root.path()).unwrap();
            assert_eq!(
                fs::read(staged.path().join(binary_name(target).unwrap())).unwrap(),
                image
            );
            assert_eq!(staged.manifest()["target"], *target);
            assert_eq!(
                fs::read(staged.path().join("THIRD-PARTY-NOTICES.txt")).unwrap(),
                fs::read(root.path().join("notices.txt")).unwrap()
            );
            assert!(
                package_archive::unpack(&bytes, TARGETS[(index + 1) % TARGETS.len()], root.path())
                    .is_err()
            );
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                assert_eq!(
                    fs::metadata(staged.path().join(binary_name(target).unwrap()))
                        .unwrap()
                        .permissions()
                        .mode()
                        & 0o777,
                    0o755
                );
            }
        }
    }
    fn fixture() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
        let root = tempfile::tempdir().unwrap();
        fs::write(
            root.path().join("notices.txt"),
            "Atelier Rust third-party notices\nSynthetic notice fixture only.\n",
        )
        .unwrap();
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
        assert!(
            stage(
                TARGETS[1],
                &binary,
                &ui,
                &root.path().join("notices.txt"),
                &output
            )
            .is_err()
        );
        assert!(!output.exists());
        fs::remove_file(ui.join("node.exe")).unwrap();
        assert!(
            stage(
                TARGETS[2],
                &binary,
                &ui,
                &root.path().join("notices.txt"),
                &output
            )
            .is_err()
        );
        stage(
            TARGETS[1],
            &binary,
            &ui,
            &root.path().join("notices.txt"),
            &output,
        )
        .unwrap();
        verify(&output).unwrap();
        assert!(
            stage(
                TARGETS[1],
                &binary,
                &ui,
                &root.path().join("notices.txt"),
                &output
            )
            .is_err()
        );
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
