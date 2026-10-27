use super::{Result, Shared};
use crate::infrastructure::checked_file::CheckedFile;
use axum::{Router, body::Bytes, http::header, routing::get};
use cap_std::{ambient_authority, fs::Dir};
use std::path::Path;

pub fn wasm_asset_name(name: &str) -> bool {
    if !name
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
    {
        return false;
    }
    name == "index.html"
        || name == "atelier-loader.js"
        || (name.starts_with("atelier-ui-") && (name.ends_with(".js") || name.ends_with(".wasm")))
        || (name.starts_with("style-") && name.ends_with(".css"))
}

pub(super) fn router(path: &Path) -> Result<Router<Shared>> {
    let directory = Dir::open_ambient_dir(path, ambient_authority())?;
    let names = crate::infrastructure::package::asset_names(&directory)?;
    let mut router = Router::new();
    let mut total = 0usize;
    for name in names {
        // Capture checked bytes once; replacing the path later cannot publish another file.
        let bytes = Bytes::from(
            CheckedFile::open_in(&directory, Path::new(&name))?
                .single_link()?
                .read(64 * 1024 * 1024)
                .map_err(|error| format!("Asset {name} : {error}"))?,
        );
        total = total
            .checked_add(bytes.len())
            .filter(|total| *total <= 256 * 1024 * 1024)
            .ok_or("Les assets dépassent la limite de chargement.")?;
        let mime = match Path::new(&name)
            .extension()
            .and_then(|extension| extension.to_str())
        {
            Some("js") => "text/javascript; charset=utf-8",
            Some("css") => "text/css; charset=utf-8",
            Some("wasm") => "application/wasm",
            Some("png") => "image/png",
            _ => "text/html; charset=utf-8",
        };
        let route = if name == "index.html" {
            "/".into()
        } else {
            format!("/{name}")
        };
        router = router.route(
            &route,
            get(move || {
                let bytes = bytes.clone();
                async move { ([(header::CONTENT_TYPE, mime)], bytes) }
            }),
        );
    }
    Ok(router)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_assets_refuse_unexpected_names_and_unbounded_file_counts() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("index.html"), "Synthetic Rust").unwrap();
        assert!(router(root.path()).is_ok());
        std::fs::write(root.path().join(".trunk-lock"), []).unwrap();
        assert!(router(root.path()).is_ok());
        let directory = Dir::open_ambient_dir(root.path(), ambient_authority()).unwrap();
        assert_eq!(
            crate::infrastructure::package::asset_names(&directory).unwrap(),
            vec!["index.html"]
        );
        assert!(root.path().join(".trunk-lock").is_file());
        std::fs::write(root.path().join(".trunk-lock"), b"unexpected").unwrap();
        assert!(router(root.path()).is_err());
        std::fs::write(root.path().join(".trunk-lock"), []).unwrap();
        std::fs::write(root.path().join("node.exe"), "Never served").unwrap();
        assert!(router(root.path()).is_err());
        std::fs::remove_file(root.path().join("node.exe")).unwrap();
        for index in 0..128 {
            std::fs::write(root.path().join(format!("style-{index}.css")), []).unwrap();
        }
        assert!(router(root.path()).is_err());
    }

    #[test]
    fn a_hardlinked_trunk_marker_is_not_ignored() {
        let root = tempfile::tempdir().unwrap();
        let source = tempfile::NamedTempFile::new_in(root.path().parent().unwrap()).unwrap();
        std::fs::write(root.path().join("index.html"), "Synthetic Rust").unwrap();
        std::fs::hard_link(source.path(), root.path().join(".trunk-lock")).unwrap();
        assert!(router(root.path()).is_err());
        assert!(source.path().is_file());
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_trunk_marker_is_not_ignored() {
        let root = tempfile::tempdir().unwrap();
        let source = tempfile::NamedTempFile::new_in(root.path().parent().unwrap()).unwrap();
        std::fs::write(root.path().join("index.html"), "Synthetic Rust").unwrap();
        std::os::unix::fs::symlink(source.path(), root.path().join(".trunk-lock")).unwrap();
        assert!(router(root.path()).is_err());
        assert!(source.path().is_file());
    }
}
