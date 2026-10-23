use atelier_app::Result;
use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

mod keyring;
mod storage;
mod ui;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace parent")
        .to_owned()
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

fn build_ui() -> Result<()> {
    pinned("trunk", "trunk 0.22.0")?;
    pinned("wasm-bindgen", "wasm-bindgen 0.2.129")?;
    run(command("trunk").args([
        "build",
        "--config",
        "crates/atelier-ui/Trunk.toml",
        "--locked",
        "--offline",
        "--skip-version-check",
    ]))?;
    let directory = root().join(".artifacts/rust-ui");
    let (html, loader) =
        externalize_loader(&std::fs::read_to_string(directory.join("index.html"))?)?;
    // Trunk's generated loader is externalized so CSP never needs unsafe-inline.
    std::fs::write(directory.join("atelier-loader.js"), loader)?;
    std::fs::write(directory.join("index.html"), html)?;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["build-ui"] => build_ui(),
        ["test-storage-parity"] => storage::test().await,
        ["test-keyring"] => keyring::test().await,
        ["test-tray"] => run(command("cargo").args(["run", "--locked", "-p", "xtask", "--bin", "native-fixture", "--", "tray"])),
        ["test-ui"] => { build_ui()?; ui::test().await },
        _ => Err("Usage : cargo xtask build-ui | test-ui | test-storage-parity | test-keyring | test-tray. Les commandes de livraison seront portées en M09–M10.".into()),
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
