pub const PREVIEW_LIMIT: usize = 512 * 1024;

pub fn diff_line_numbers(patch: &str) -> Vec<(Option<usize>, Option<usize>)> {
    let mut position = None::<(usize, usize)>;
    patch
        .lines()
        .take(10_000)
        .map(|line| {
            if line.starts_with("@@") {
                position = (|| {
                    let header = line.strip_prefix("@@ ")?.split_once(" @@")?.0;
                    let mut sides = header.split_whitespace();
                    let old = sides
                        .next()?
                        .strip_prefix('-')?
                        .split(',')
                        .next()?
                        .parse()
                        .ok()?;
                    let new = sides
                        .next()?
                        .strip_prefix('+')?
                        .split(',')
                        .next()?
                        .parse()
                        .ok()?;
                    if sides.next().is_some() {
                        return None;
                    }
                    Some((old, new))
                })();
                return (None, None);
            }
            let Some((old, new)) = position else {
                return (None, None);
            };
            let (numbers, next) = match line.as_bytes().first() {
                Some(b' ') => (
                    (Some(old), Some(new)),
                    old.checked_add(1).zip(new.checked_add(1)),
                ),
                Some(b'-') => ((Some(old), None), old.checked_add(1).map(|old| (old, new))),
                Some(b'+') => ((None, Some(new)), new.checked_add(1).map(|new| (old, new))),
                Some(b'\\') => ((None, None), position),
                _ => ((None, None), None),
            };
            position = next;
            numbers
        })
        .collect()
}

#[cfg(test)]
mod diff_preview_tests {
    #[test]
    fn diff_numbers_follow_each_hunk_and_never_number_metadata_as_source() {
        let patch = "diff --git a/a b/a\n--- a/a\n+++ b/a\n@@ -3,2 +7,3 @@\n same\n-old\n+new\n+more\n\\ No newline at end of file\n@@ -9 +14 @@\n next\ndiff --git a/b b/b\n--- a/b\n+++ b/b\n@@ -0,0 +1 @@\n+added\n@@ invalid @@\n+unknown\n";
        let numbers = super::diff_line_numbers(patch);
        assert_eq!(numbers[2], (None, None));
        assert_eq!(numbers[4], (Some(3), Some(7)));
        assert_eq!(numbers[5], (Some(4), None));
        assert_eq!(numbers[6], (None, Some(8)));
        assert_eq!(numbers[8], (None, None));
        assert_eq!(numbers[10], (Some(9), Some(14)));
        assert_eq!(numbers[13], (None, None));
        assert_eq!(numbers[15], (None, Some(1)));
        assert_eq!(numbers[17], (None, None));
    }
}

pub fn validate_path(path: &str, windows: bool) -> Result<(), &'static str> {
    if path.is_empty() || path.len() > 4096 || path.contains(['\\', '\0', ':']) {
        return Err("message.invalid_file_path");
    }
    for part in path.split('/') {
        if part.is_empty() || part == "." || part == ".." || part.eq_ignore_ascii_case(".git") {
            return Err("message.invalid_file_path");
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
                return Err("message.invalid_file_path");
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
