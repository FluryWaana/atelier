use crate::{Result, command, run, target};
use std::path::PathBuf;

const MARKER: &str = "atelier-synthetic-browser-only";

pub(super) fn run_isolated(args: &[String]) -> Result<bool> {
    if !args.first().is_some_and(|arg| {
        matches!(
            arg.as_str(),
            "test-ui"
                | "test-updates"
                | "qualify-packages"
                | "qualify-signed-portable"
                | "qualify-installed-update"
        )
    }) || std::env::var_os("ATELIER_BROWSER_CAPTURE").is_some()
    {
        return Ok(false);
    }
    if args[0] != "test-ui" {
        run(command("cargo").args([
            "build",
            "--locked",
            "-p",
            "xtask",
            "--bin",
            "native-fixture",
        ]))?;
    }
    let temporary = tempfile::tempdir()?;
    std::fs::write(temporary.path().join(".browser-fixture"), MARKER)?;
    run(command(std::env::current_exe()?)
        .args(args)
        .env(
            "ATELIER_BROWSER",
            target()
                .join("debug")
                .join(format!("native-fixture{}", std::env::consts::EXE_SUFFIX)),
        )
        .env(
            "ATELIER_BROWSER_CAPTURE",
            temporary.path().join("opened.json"),
        ))?;
    Ok(true)
}

pub(super) fn capture_path() -> Result<PathBuf> {
    let path = PathBuf::from(
        std::env::var_os("ATELIER_BROWSER_CAPTURE").ok_or("Synthetic browser capture missing")?,
    );
    let parent = path.parent().ok_or("Synthetic browser directory missing")?;
    if !path.is_absolute() || std::fs::read_to_string(parent.join(".browser-fixture"))? != MARKER {
        return Err("Synthetic browser scope marker missing".into());
    }
    Ok(path)
}

pub(super) fn captured_address() -> Result<Option<String>> {
    let path = capture_path()?;
    if !path.try_exists()? {
        return Ok(None);
    }
    use atelier_app::infrastructure::checked_file::{CheckedFile, Identity};
    let bytes = CheckedFile::open(&path, &Identity::inspect(&path)?)?.read(16 * 1024)?;
    match serde_json::from_slice(&bytes) {
        Ok(address) => Ok(Some(address)),
        Err(error) if error.is_eof() => Ok(None),
        Err(error) => Err(error.into()),
    }
}

pub(super) fn clear_capture() -> Result<()> {
    let path = capture_path()?;
    if path.try_exists()? {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

pub(super) async fn wait_for_open() -> Result<String> {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        if let Some(address) = captured_address()? {
            return Ok(address);
        }
        if tokio::time::Instant::now() >= deadline {
            return Err("Atelier did not request an automatic browser opening".into());
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}
