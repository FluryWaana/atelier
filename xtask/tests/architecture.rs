use std::{fs, path::Path};

fn rust_sources(directory: &Path) -> Vec<std::path::PathBuf> {
    let mut sources = Vec::new();
    for entry in fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            sources.extend(rust_sources(&path));
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            sources.push(path);
        }
    }
    sources
}

#[test]
fn domain_and_use_cases_stay_independent_of_io_and_platform_adapters() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let manifest = fs::read_to_string(root.join("crates/atelier-core/Cargo.toml")).unwrap();
    let mut dependencies = false;
    for line in manifest.lines().map(str::trim) {
        if line.starts_with('[') {
            dependencies = line.ends_with("dependencies]");
        } else if dependencies && let Some((name, _)) = line.split_once('=') {
            assert!(
                matches!(name.trim().split('.').next(), Some("serde" | "serde_json")),
                "Native dependency in core: {line}"
            );
        }
    }
    for path in rust_sources(&root.join("crates/atelier-core/src")) {
        let source = fs::read_to_string(&path).unwrap();
        for forbidden in [
            "std::fs",
            "std::process",
            "std::net",
            "tokio::",
            "rusqlite::",
            "reqwest::",
            "atelier_app::",
        ] {
            assert!(
                !source.contains(forbidden),
                "I/O dependency {forbidden} in {}",
                path.display()
            );
        }
        for retired in ["JsonString", ".truthy(", "Document::spread", "WTF-8"] {
            assert!(
                !source.contains(retired),
                "Retired compatibility contract {retired} in {}",
                path.display()
            );
        }
    }
}
