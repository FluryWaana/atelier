use std::collections::BTreeMap;

#[test]
fn every_explicit_ui_translation_has_a_nonempty_english_message() {
    let catalog: BTreeMap<String, String> =
        serde_json::from_str(include_str!("../../crates/atelier-ui/locales/en-US.json")).unwrap();
    assert!(catalog.values().all(|message| !message.trim().is_empty()));
    for source in [
        include_str!("../../crates/atelier-ui/src/main.rs"),
        include_str!("../../crates/atelier-ui/src/task_editor.rs"),
        include_str!("../../crates/atelier-ui/src/projects.rs"),
        include_str!("../../crates/atelier-ui/src/settings.rs"),
        include_str!("../../crates/atelier-ui/src/backups.rs"),
        include_str!("../../crates/atelier-ui/src/files.rs"),
    ] {
        for call in source.split("i18n.text(\"").skip(1) {
            let key = call.split('"').next().unwrap();
            assert!(catalog.contains_key(key), "Missing English message: {key}");
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
