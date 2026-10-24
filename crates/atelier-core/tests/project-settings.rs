use atelier_core::{document::Document, domain::projects::project_settings};
#[path = "support/json.rs"]
mod json;

#[test]
fn historical_project_settings_preserve_utf16_paths_and_error_precedence() {
    let cases: Vec<Document> = serde_json::from_str(include_str!(
        "../../../test/fixtures/rust-project-settings-reference.json"
    ))
    .unwrap();
    assert!(cases.len() >= 40);
    for (index, case) in cases.into_iter().enumerate() {
        let body: Document = case.get("body").unwrap().unwrap();
        let settings: Document = case.get("settings").unwrap().unwrap();
        let mut actual = Document::default();
        match project_settings(&body, &settings) {
            Ok(value) => actual.set("value", &value).unwrap(),
            Err(error) => actual.set("error", &error).unwrap(),
        }
        let actual = serde_json::value::to_raw_value(&actual).unwrap();
        let expected = case.raw("expected").unwrap();
        assert!(
            json::equivalent(&actual, expected),
            "case {index}\nactual: {actual}\nexpected: {expected}"
        );
    }
}
