use atelier_core::{document::Document, domain::agent_options::provider_settings};
#[path = "support/json.rs"]
mod json;

#[test]
fn provider_settings_preserve_utf16_and_error_priority() {
    let reference: Document = serde_json::from_str(include_str!(
        "fixtures/rust-provider-settings-reference.json"
    ))
    .unwrap();
    let input: Document = reference.get("body").unwrap().unwrap();
    let defaults: Document = reference.get("defaults").unwrap().unwrap();
    let cases: Vec<Document> = reference.get("cases").unwrap().unwrap();
    assert!(cases.len() >= 18);
    for (index, case) in cases.into_iter().enumerate() {
        let mut body = input.clone();
        body.overlay(&case.get::<Document>("body").unwrap().unwrap());
        let actual = provider_settings(&serde_json::value::to_raw_value(&body).unwrap());
        if let Some(error) = case.get::<String>("error").unwrap() {
            assert_eq!(actual.unwrap_err(), error, "case {index}");
        } else {
            let mut expected = defaults.clone();
            expected.overlay(&case.get::<Document>("settings").unwrap().unwrap());
            let actual = serde_json::value::to_raw_value(&actual.unwrap()).unwrap();
            let expected = serde_json::value::to_raw_value(&expected).unwrap();
            assert!(
                json::equivalent(&actual, &expected),
                "case {index}\nactual: {actual}\nexpected: {expected}"
            );
        }
    }
}
