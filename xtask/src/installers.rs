use atelier_app::{
    Result,
    infrastructure::{installation::InstallationMode, installed, package, process_tree},
};
use sha2::{Digest, Sha256};
use std::{fs, path::Path, time::Duration};
const PRE: &str = include_str!("../packaging/linux-preinstall.sh");
const REMOVE: &str = include_str!("../packaging/linux-preremove.sh");
const POST: &str = include_str!("../packaging/linux-postremove.sh");
const INSTALL: &str = include_str!("../packaging/linux-postinstall.sh");
#[cfg(unix)]
fn system_modes(root: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut pending = vec![root.to_owned()];
    let mut count = 0;
    while let Some(path) = pending.pop() {
        count += 1;
        if count > 512 {
            return Err("Installation de fabrication trop volumineuse.".into());
        }
        let metadata = atelier_app::infrastructure::checked_file::plain_metadata(&path)?;
        let mode = if metadata.is_dir() {
            for entry in fs::read_dir(&path)? {
                pending.push(entry?.path());
            }
            0o755
        } else if metadata.is_file() {
            if metadata.permissions().mode() & 0o111 != 0 {
                0o755
            } else {
                0o644
            }
        } else {
            return Err("Fichier spécial refusé dans l’installeur.".into());
        };
        fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
    }
    Ok(())
}
fn artifact(name: &str, bytes: &[u8]) -> Result<()> {
    let valid = if name.ends_with(".exe") {
        bytes.starts_with(b"MZ")
    } else if name.ends_with(".deb") {
        bytes.starts_with(b"!<arch>\n")
    } else if name.ends_with(".rpm") {
        bytes.starts_with(&[0xed, 0xab, 0xee, 0xdb])
    } else {
        false
    };
    if !valid {
        return Err("Format d’installeur produit invalide.".into());
    }
    Ok(())
}
fn put(root: &Path, name: &str, bytes: &[u8], executable: bool) -> Result<()> {
    let path = root.join(name);
    fs::create_dir_all(path.parent().ok_or("Dossier de paquet absent.")?)?;
    package::write(&path, bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            &path,
            fs::Permissions::from_mode(if executable { 0o755 } else { 0o644 }),
        )?;
    }
    #[cfg(not(unix))]
    let _ = executable;
    Ok(())
}
async fn run(program: impl AsRef<std::ffi::OsStr>, args: &[std::ffi::OsString]) -> Result<()> {
    let mut command = tokio::process::Command::new(program);
    command.args(args).env_clear();
    for key in [
        "PATH",
        "SystemRoot",
        "WINDIR",
        "TEMP",
        "TMP",
        "HOME",
        "LANG",
        "LC_ALL",
        "APPDATA",
        "LOCALAPPDATA",
    ] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    let result =
        process_tree::capture(&mut command, &[], Duration::from_secs(600), 2 * 1024 * 1024).await?;
    if !result.status.success() {
        eprintln!(
            "Installer compiler exit: {:?}\n{}\n{}",
            result.status.code(),
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        return Err("Fabrication de l’installeur refusée ; aucun artefact final publié.".into());
    }
    Ok(())
}
pub(super) async fn build(
    archive: &Path,
    signature: &Path,
    trust: &Path,
    output: &Path,
) -> Result<()> {
    if std::env::var("CI").as_deref() != Ok("true")
        || std::env::var("ATELIER_RUST_FINAL_CAMPAIGN").as_deref() != Ok("1")
    {
        return Err("Les installeurs sont réservés à la campagne finale CI synthétique.".into());
    }
    let target = package::native_target()?;
    if target.contains("apple") {
        return Err("macOS utilise le paquet portable Rust ; aucun installeur système supplémentaire prévu.".into());
    }
    if output.try_exists()? {
        return Err("Le dossier de livraison existe déjà. Aucun artefact remplacé.".into());
    }
    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let scratch = tempfile::Builder::new()
        .prefix("atelier-installers-")
        .tempdir_in(parent)?;
    let scratch_path = fs::canonicalize(scratch.path())?;
    if scratch_path.to_str().is_none_or(|path| {
        path.chars()
            .any(|c| c.is_control() || matches!(c, '"' | '\'' | '%' | '$' | '`'))
    }) {
        return Err(
            "Chemin de fabrication non pris en charge par les compilateurs de paquets.".into(),
        );
    }
    let delivery = tempfile::Builder::new()
        .prefix("atelier-delivery-")
        .tempdir_in(parent)?;
    let result = fs::canonicalize(delivery.path())?;
    let app = scratch_path.join("app");
    fs::create_dir_all(app.parent().ok_or("Dossier de staging absent.")?)?;
    installed::install(
        archive,
        signature,
        trust,
        &app,
        if cfg!(windows) {
            InstallationMode::User
        } else {
            InstallationMode::System
        },
        target,
    )?;
    installed::verify_all(&app, target)?;
    let installation = installed::open(&app, target)?.ok_or("Installation de staging absente.")?;
    let version = installation.current().active.version.clone();
    let slot = installation.slot_path(&installation.current().active)?;
    drop(installation);
    if cfg!(windows) {
        let compiler = std::env::var_os("ATELIER_ISCC")
            .ok_or("Chemin absolu ATELIER_ISCC requis (Inno Setup 6).")?;
        if !Path::new(&compiler).is_absolute() {
            return Err("Compilateur Inno absolu requis.".into());
        }
        run(
            compiler,
            &[
                format!("/DSourceDir={}", app.display()).into(),
                format!("/DOutputDir={}", result.display()).into(),
                format!("/DReleaseVersion={version}").into(),
                super::root()
                    .join("xtask/packaging/windows.iss")
                    .into_os_string(),
            ],
        )
        .await?;
    } else {
        let deb = scratch_path.join("deb");
        for (name, source, limit, executable) in [
            (
                "usr/lib/atelier-kanban-rust/package-tool",
                app.join("atelier"),
                128 * 1024 * 1024,
                true,
            ),
            (
                "usr/lib/atelier-kanban-rust/payload.zip",
                archive.to_owned(),
                atelier_app::infrastructure::package_archive::ARCHIVE_LIMIT,
                false,
            ),
            (
                "usr/lib/atelier-kanban-rust/signed-package.json",
                signature.to_owned(),
                16 * 1024,
                false,
            ),
            (
                "usr/lib/atelier-kanban-rust/trusted-update-key.json",
                trust.to_owned(),
                4096,
                false,
            ),
            (
                "usr/share/doc/atelier-kanban-rust/LICENSE",
                slot.join("LICENSE"),
                1024 * 1024,
                false,
            ),
            (
                "usr/share/doc/atelier-kanban-rust/THIRD-PARTY-NOTICES.txt",
                slot.join("THIRD-PARTY-NOTICES.txt"),
                32 * 1024 * 1024,
                false,
            ),
        ] {
            put(&deb, name, &package::read(&source, limit)?, executable)?;
        }
        put(
            &deb,
            "usr/bin/atelier-kanban-rust",
            include_bytes!("../packaging/atelier-kanban-rust"),
            true,
        )?;
        put(
            &deb,
            "usr/share/applications/atelier-kanban-rust.desktop",
            include_bytes!("../packaging/atelier-kanban-rust.desktop"),
            false,
        )?;
        let architecture = if target.starts_with("aarch64") {
            "arm64"
        } else {
            "amd64"
        };
        let control = format!(
            "Package: atelier-kanban-rust\nVersion: {version}\nArchitecture: {architecture}\nMaintainer: Atelier Kanban <noreply@github.com>\nSection: devel\nPriority: optional\nDepends: libc6, libgcc-s1, libsecret-tools, xdg-utils, zenity, util-linux\nDescription: Local Kanban workspace with a Rust runtime\n Signed native installation. User data is retained on removal.\n"
        );
        put(&deb, "DEBIAN/control", control.as_bytes(), false)?;
        put(&deb, "DEBIAN/preinst", PRE.as_bytes(), true)?;
        put(&deb, "DEBIAN/postinst", INSTALL.as_bytes(), true)?;
        put(&deb, "DEBIAN/prerm", REMOVE.as_bytes(), true)?;
        put(&deb, "DEBIAN/postrm", POST.as_bytes(), true)?;
        #[cfg(unix)]
        system_modes(&deb)?;
        run(
            "desktop-file-validate",
            &[deb
                .join("usr/share/applications/atelier-kanban-rust.desktop")
                .into_os_string()],
        )
        .await?;
        run(
            "dpkg-deb",
            &[
                "--root-owner-group".into(),
                "--build".into(),
                deb.clone().into_os_string(),
                result
                    .join(format!("atelier-kanban-{version}-linux-{architecture}.deb"))
                    .into_os_string(),
            ],
        )
        .await?;
        let rpm_arch = if target.starts_with("aarch64") {
            "aarch64"
        } else {
            "x86_64"
        };
        let rpm = scratch_path.join("rpmbuild");
        fs::create_dir(&rpm)?;
        for name in ["BUILD", "BUILDROOT", "RPMS", "SOURCES", "SPECS", "SRPMS"] {
            fs::create_dir(rpm.join(name))?;
        }
        let spec = format!(
            "Name: atelier-kanban-rust\nVersion: {version}\nRelease: 1\nSummary: Local Kanban workspace\nLicense: AGPL-3.0-or-later\nRequires: libsecret, xdg-utils, zenity, util-linux\n%global __os_install_post %{{nil}}\n%description\nNative Rust application; signed runtime state and user data are retained safely.\n%install\nmkdir -p \"%{{buildroot}}\"\ncp -a \"{}/usr\" \"%{{buildroot}}/\"\n%pre\n{PRE}\n%post\n{INSTALL}\n%preun\n{REMOVE}\n%postun\n{POST}\n%files\n%defattr(-,root,root,-)\n/usr/lib/atelier-kanban-rust\n/usr/bin/atelier-kanban-rust\n/usr/share/applications/atelier-kanban-rust.desktop\n/usr/share/doc/atelier-kanban-rust\n",
            deb.display()
        );
        let spec_path = scratch_path.join("atelier.spec");
        package::write(&spec_path, spec.as_bytes())?;
        run(
            "rpmbuild",
            &[
                "-bb".into(),
                "--target".into(),
                rpm_arch.into(),
                "--define".into(),
                format!("_topdir {}", rpm.display()).into(),
                "--define".into(),
                format!("_rpmdir {}", result.display()).into(),
                "--define".into(),
                format!("_rpmfilename atelier-kanban-{version}-linux-{rpm_arch}.rpm").into(),
                spec_path.into_os_string(),
            ],
        )
        .await?;
    }
    let artifacts = fs::read_dir(&result)?.collect::<std::io::Result<Vec<_>>>()?;
    if artifacts.len() != if cfg!(windows) { 1 } else { 2 } {
        return Err("Nombre d’artefacts d’installation inattendu.".into());
    }
    for artifact in artifacts {
        let bytes = package::read(&artifact.path(), 512 * 1024 * 1024)?;
        let name = artifact
            .file_name()
            .into_string()
            .map_err(|_| "Nom d’artefact invalide.")?;
        self::artifact(&name, &bytes)?;
        put(
            &result,
            &format!("{name}.sha256"),
            format!("{:x}  {name}\n", Sha256::digest(bytes)).as_bytes(),
            false,
        )?;
    }
    if output.try_exists()? {
        return Err("Destination créée pendant la fabrication ; aucun fichier remplacé.".into());
    }
    package::publish_directory(&result, output)?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_or_misnamed_compiler_results_do_not_become_installers() {
        for (name, bytes) in [
            ("setup.exe", &b""[..]),
            ("package.deb", &b"MZ"[..]),
            ("package.rpm", &b"!<arch>\n"[..]),
            ("node.zip", &b"MZ"[..]),
        ] {
            assert!(artifact(name, bytes).is_err());
        }
        assert!(artifact("synthetic.deb", b"!<arch>\nsynthetic header only").is_ok());
    }
}
