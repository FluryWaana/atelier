use atelier_app::Result;
use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

mod installers;
mod inventory;
mod keyring;
mod notices;
mod package;
mod package_delivery;
mod ui;
mod update_flow;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace parent")
        .to_owned()
}

fn target() -> PathBuf {
    root().join(std::env::var_os("CARGO_TARGET_DIR").unwrap_or_else(|| "target".into()))
}

fn command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut command = Command::new(program);
    command.current_dir(root());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command
}

fn run(command: &mut Command) -> Result<()> {
    if !command.status()?.success() {
        return Err("La commande de vérification a échoué.".into());
    }
    Ok(())
}

fn pinned(program: &str, expected: &str) -> Result<()> {
    let output = command(program)
        .arg("--version")
        .stderr(Stdio::null())
        .output()?;
    if !output.status.success() || String::from_utf8(output.stdout)?.trim() != expected {
        return Err(format!("Version requise : {expected}").into());
    }
    Ok(())
}

fn externalize_loader(html: &str) -> Result<(String, String)> {
    let opening = "<script type=\"module\">";
    let (before, remainder) = html
        .split_once(opening)
        .ok_or("Chargeur Trunk absent ou format modifié.")?;
    let (script, after) = remainder
        .split_once("</script>")
        .ok_or("Chargeur Trunk incomplet.")?;
    if after.contains("<script") || before.contains("<script") || script.trim().is_empty() {
        return Err("Scripts Trunk inattendus ; vérifier la politique CSP.".into());
    }
    Ok((
        format!("{before}<script type=\"module\" src=\"/atelier-loader.js\"></script>{after}"),
        script.to_owned(),
    ))
}

fn build_ui(dev: bool) -> Result<()> {
    pinned("trunk", "trunk 0.22.1")?;
    pinned("wasm-bindgen", "wasm-bindgen 0.2.130")?;
    let directory = root().join(if dev {
        ".artifacts/rust-ui-dev"
    } else {
        ".artifacts/rust-ui"
    });
    let mut trunk = command("trunk");
    trunk.arg("build");
    if dev {
        trunk
            .arg("--dist")
            .arg(&directory)
            .env("CARGO_PROFILE_DEV_DEBUG", "0")
            .env("CARGO_PROFILE_DEV_INCREMENTAL", "true");
    } else {
        trunk.arg("--release");
    }
    run(trunk.args([
        "--config",
        "crates/atelier-ui/Trunk.toml",
        "--locked",
        "--offline",
        "--skip-version-check",
    ]))?;
    let (html, loader) =
        externalize_loader(&std::fs::read_to_string(directory.join("index.html"))?)?;
    // Trunk's generated loader is externalized so CSP never needs unsafe-inline.
    std::fs::write(directory.join("atelier-loader.js"), loader)?;
    std::fs::write(directory.join("index.html"), html)?;
    validate_ui_assets(&directory)
}

fn validate_ui_assets(directory: &Path) -> Result<()> {
    let mut total = 0u64;
    for asset in atelier_app::infrastructure::package::assets(directory)? {
        let size =
            atelier_app::infrastructure::checked_file::plain_metadata(&directory.join(&asset))?
                .len();
        if size > 64 * 1024 * 1024 {
            return Err(format!("Interface non utilisable : {asset} contient {size} octets, limite 64 Mio par asset. Le profil de compilation doit produire un fichier plus petit.").into());
        }
        total = total
            .checked_add(size)
            .ok_or("Taille totale de l’interface excessive.")?;
        if total > 256 * 1024 * 1024 {
            return Err("Interface non utilisable : plus de 256 Mio d’assets produits. Aucun relèvement de limite.".into());
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["inventory"] => inventory::command(false),
        ["inventory", "--check"] => inventory::command(true),
        ["build-ui"] => build_ui(false),
        ["build-ui", "--dev"] => build_ui(true),
        ["qualify-packages", output] => package_delivery::qualify(Path::new(output)).await,
        ["third-party-notices", output] => notices::generate(Path::new(output)).await,
        ["build-installers",archive,signature,trust,output]=>installers::build(Path::new(archive),Path::new(signature),Path::new(trust),Path::new(output)).await,
        ["stage-package", target, binary, assets, notices, output] => package::stage(target,Path::new(binary),Path::new(assets),Path::new(notices),Path::new(output)),
        ["verify-package", directory] => package::verify(Path::new(directory)),
        ["archive-package", directory, output] => atelier_app::infrastructure::package_archive::create(Path::new(directory),Path::new(output)),
        ["verify-archive", archive, target] => atelier_app::infrastructure::package_archive::verify(Path::new(archive),target),
        ["synthetic-update-key", directory] => atelier_app::infrastructure::update_package::generate_synthetic(Path::new(directory)),
        ["sign-package", archive, target, keys, output] => atelier_app::infrastructure::update_package::sign(Path::new(archive),target,Path::new(keys),Path::new(output)),
        ["verify-signed-package", archive, metadata, trust, target, current] => {
            let trust=atelier_app::infrastructure::update_package::Trust::load(Path::new(trust))?;
            let scratch=tempfile::tempdir()?;
            atelier_app::infrastructure::update_package::prepare(Path::new(archive),Path::new(metadata),&trust,target,current,scratch.path()).map(|_|())
        },
        ["test-keyring"] => keyring::test(false).await,
        ["test-keyring-lock"] => keyring::test(true).await,
        ["test-keyring-account", phase, directory] => keyring::accounts::probe(Path::new(directory), phase).await,
        ["test-tray"] => run(command("cargo").args(["run", "--locked", "-p", "xtask", "--bin", "native-fixture", "--", "tray"])),
        ["test-ui"] => ui::test(false).await,
        ["test-ui", "--prepared-assets"] => ui::test(true).await,
        ["test-updates"] => update_flow::test().await,
        ["test-updates", "--prepared-delivery", delivery] => update_flow::test_delivery(Path::new(delivery)).await,
        ["qualify-installed-update", installation, data, port, delivery] => update_flow::test_installed(Path::new(installation), Path::new(data), port.parse()?, Path::new(delivery)).await,
        _ => Err("Usage : cargo xtask inventory [--check] | build-ui [--dev] | build-installers ARCHIVE SIGNATURE TRUST OUTPUT | third-party-notices OUTPUT.txt | stage-package TARGET BINARY ASSETS NOTICES.txt OUTPUT | verify-package DIRECTORY | archive-package DIRECTORY OUTPUT.zip | verify-archive ARCHIVE.zip TARGET | synthetic-update-key DIRECTORY | sign-package ARCHIVE TARGET KEYS OUTPUT | verify-signed-package ARCHIVE METADATA TRUST TARGET CURRENT | test-ui [--prepared-assets] | test-keyring | test-keyring-lock | test-keyring-account PHASE DIRECTORY | test-tray. Les commandes de livraison, installateurs, tray et mises à jour sont des candidats non qualifiés ; aucune réussite sans campagne finale exécutée.".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn workspace_keeps_native_adapters_out_of_core_and_ui() {
        let output = command("cargo")
            .args([
                "metadata",
                "--format-version",
                "1",
                "--no-deps",
                "--locked",
                "--offline",
            ])
            .output()
            .unwrap();
        assert!(output.status.success());
        let metadata: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(metadata["workspace_members"].as_array().unwrap().len(), 4);
        for package in metadata["packages"].as_array().unwrap() {
            for dependency in package["dependencies"].as_array().unwrap() {
                let name = dependency["name"].as_str().unwrap();
                match package["name"].as_str().unwrap() {
                    "atelier-core" => assert!(
                        ["serde", "serde_json"].contains(&name),
                        "Native dependency in core: {name}"
                    ),
                    "atelier-ui" => assert!(
                        !["atelier-app", "xtask", "tokio", "rusqlite", "windows-sys"]
                            .contains(&name),
                        "Native dependency in UI: {name}"
                    ),
                    "atelier-app" => assert!(!["atelier-ui", "xtask"].contains(&name)),
                    "xtask" => {}
                    name => panic!("Unexpected workspace crate: {name}"),
                }
            }
        }
    }

    #[test]
    fn generated_loader_is_external_and_unexpected_scripts_fail_closed() {
        let (html, loader) = externalize_loader(
            "<head></head><script type=\"module\">import init from './app.js'; init();</script>",
        )
        .unwrap();
        assert_eq!(loader, "import init from './app.js'; init();");
        assert!(html.contains("src=\"/atelier-loader.js\""));
        assert!(!html.contains("import init"));
        for html in [
            "",
            "<script type=\"module\"></script>",
            "<script type=\"module\">a()</script><script>b()</script>",
        ] {
            assert!(externalize_loader(html).is_err());
        }
    }
}
