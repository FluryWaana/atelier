use std::collections::BTreeMap;

#[test]
fn translation_catalogs_cover_explicit_keys_and_placeholders() {
    let catalog: BTreeMap<String, String> =
        serde_json::from_str(include_str!("../../crates/atelier-ui/locales/en-US.json")).unwrap();
    let french: BTreeMap<String, String> =
        serde_json::from_str(include_str!("../../crates/atelier-ui/locales/fr-FR.json")).unwrap();
    assert_eq!(
        catalog.keys().collect::<Vec<_>>(),
        french.keys().collect::<Vec<_>>()
    );
    assert!(french.values().all(|message| !message.trim().is_empty()));
    assert!(catalog.values().all(|message| !message.trim().is_empty()));
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../crates");
    let mut directories = vec![
        root.join("atelier-ui/src"),
        root.join("atelier-app/src"),
        root.join("atelier-core/src"),
    ];
    while let Some(directory) = directories.pop() {
        for entry in std::fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                directories.push(path);
                continue;
            }
            if path.extension().is_none_or(|extension| extension != "rs") {
                continue;
            }
            let source = std::fs::read_to_string(&path).unwrap();
            for key in source
                .split("\"message.")
                .skip(1)
                .filter_map(|part| part.split('"').next())
                .filter(|key| !key.is_empty())
            {
                let key = format!("message.{key}");
                assert!(
                    catalog.contains_key(&key),
                    "Missing message in {}: {key}",
                    path.display()
                );
            }
            for call in source.split("i18n.text(\"").skip(1) {
                let key = call.split('"').next().unwrap();
                assert!(
                    catalog.contains_key(key),
                    "Missing English message in {}: {key}",
                    path.display()
                );
            }
        }
    }
    for (key, message) in &catalog {
        assert!(
            key.starts_with("message.")
                && key
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '.')
        );
        for position in 0..10 {
            let placeholder = format!("{{{position}}}");
            assert_eq!(
                french[key].matches(&placeholder).count(),
                message.matches(&placeholder).count(),
                "Placeholder mismatch: {key}"
            );
        }
    }
}
