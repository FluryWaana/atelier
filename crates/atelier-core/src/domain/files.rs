pub const PREVIEW_LIMIT: usize = 512 * 1024;

pub fn validate_path(path: &str, windows: bool) -> Result<(), &'static str> {
    if path.is_empty() || path.len() > 4096 || path.contains(['\\', '\0', ':']) {
        return Err("Chemin de fichier invalide.");
    }
    for part in path.split('/') {
        if part.is_empty() || part == "." || part == ".." || part.eq_ignore_ascii_case(".git") {
            return Err("Chemin de fichier invalide.");
        }
        if windows {
            let stem = part
                .split('.')
                .next()
                .unwrap_or_default()
                .to_ascii_uppercase();
            if part.ends_with(['.', ' '])
                || matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
                || ["COM", "LPT"].iter().any(|prefix| {
                    stem.strip_prefix(prefix).is_some_and(|value| {
                        matches!(
                            value,
                            "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                        )
                    })
                })
            {
                return Err("Chemin de fichier invalide.");
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_paths_reject_traversal_metadata_streams_and_windows_devices() {
        for path in [
            "",
            "/etc/passwd",
            "../secret",
            "a/../secret",
            "a//b",
            "a/./b",
            ".git/config",
            "a/.GIT/config",
            "C:/secret",
            "a\\b",
            "a:stream",
            "a\0b",
            ":(glob)*",
        ] {
            assert!(validate_path(path, false).is_err(), "{path:?}");
        }
        for path in [
            "CON",
            "aux.txt",
            "sub/lpt1.log",
            "COM¹",
            "trailing.",
            "trailing ",
        ] {
            assert!(validate_path(path, true).is_err(), "{path}");
        }
        for path in ["src/main.rs", "new file é.txt", "-option.txt", "日本語.txt"] {
            assert!(validate_path(path, true).is_ok(), "{path}");
        }
        assert!(validate_path(&"a".repeat(4097), false).is_err());
    }
}
