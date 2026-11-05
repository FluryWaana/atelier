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

pub(super) fn safari_destination(
    captured: &str,
    data: &std::path::Path,
    origin: &str,
) -> Result<String> {
    use atelier_app::infrastructure::checked_file::{CheckedFile, Identity};
    let path = reqwest::Url::parse(captured)?
        .to_file_path()
        .map_err(|_| "Expected a private native browser bootstrap")?;
    if path
        .file_name()
        .is_none_or(|name| name != "local-browser.html")
        || path
            .parent()
            .ok_or("Browser bootstrap directory missing")?
            .canonicalize()?
            != data.canonicalize()?
    {
        return Err("Browser bootstrap is outside the synthetic data directory".into());
    }
    let html = String::from_utf8(
        CheckedFile::open(&path, &Identity::inspect(&path)?)?
            .private()?
            .read(16 * 1024)?,
    )?;
    let destination = html
        .strip_prefix(
            "<!doctype html><meta charset=\"utf-8\"><meta http-equiv=\"refresh\" content=\"0;url=",
        )
        .and_then(|value| value.strip_suffix("\">"))
        .ok_or("Unexpected native browser bootstrap")?;
    let url = reqwest::Url::parse(destination)?;
    if url.origin().ascii_serialization() != origin
        || url.path() != "/"
        || url.query().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || !url
            .fragment()
            .and_then(|fragment| fragment.strip_prefix("session="))
            .is_some_and(|token| {
                token.len() == 64
                    && token
                        .bytes()
                        .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
            })
    {
        return Err("Browser bootstrap does not target the expected local session".into());
    }
    Ok(destination.to_owned())
}

#[cfg(test)]
mod tests {
    #[test]
    fn safari_follows_only_the_private_bootstrap_in_its_synthetic_data_directory() {
        let data = tempfile::tempdir().unwrap();
        let path = data.path().join("local-browser.html");
        let captured = reqwest::Url::from_file_path(&path).unwrap().to_string();
        let origin = "http://127.0.0.1:4317";
        let destination = format!("{origin}/#session={}", "a".repeat(64));
        let write = |target: &str| {
            std::fs::write(&path, format!("<!doctype html><meta charset=\"utf-8\"><meta http-equiv=\"refresh\" content=\"0;url={target}\">" )).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
            }
        };
        write(&destination);
        assert_eq!(
            super::safari_destination(&captured, data.path(), origin).unwrap(),
            destination
        );
        let other = tempfile::tempdir().unwrap();
        assert!(super::safari_destination(&captured, other.path(), origin).is_err());
        for target in [
            destination.replace(origin, "https://example.invalid"),
            destination.replace(origin, "http://127.0.0.1:4318"),
            destination.replace("/#session=", "/tasks/#session="),
            format!("{origin}/#session=invalid"),
        ] {
            write(&target);
            assert!(super::safari_destination(&captured, data.path(), origin).is_err());
        }
        std::fs::write(&path, vec![b'x'; 16 * 1024 + 1]).unwrap();
        assert!(super::safari_destination(&captured, data.path(), origin).is_err());
    }
}
