use std::collections::BTreeMap;

#[test]
fn every_explicit_ui_translation_has_a_nonempty_english_message() {
    let catalog: BTreeMap<String, String> =
        serde_json::from_str(include_str!("../../crates/atelier-ui/locales/en-US.json")).unwrap();
    assert!(catalog.values().all(|message| !message.trim().is_empty()));
    let directory =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../crates/atelier-ui/src");
    for entry in std::fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|extension| extension != "rs") {
            continue;
        }
        let source = std::fs::read_to_string(&path).unwrap();
        for call in source.split("i18n.text(\"").skip(1) {
            let key = call.split('"').next().unwrap();
            assert!(
                catalog.contains_key(key),
                "Missing English message in {}: {key}",
                path.display()
            );
        }
    }
    for (key, message) in &catalog {
        for position in 0..10 {
            let placeholder = format!("{{{position}}}");
            assert_eq!(
                key.matches(&placeholder).count(),
                message.matches(&placeholder).count(),
                "Placeholder mismatch: {key}"
            );
        }
    }
}
