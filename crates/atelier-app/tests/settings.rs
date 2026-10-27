use atelier_app::infrastructure::settings::jira_base;
use atelier_core::document::{Document, JsonString};

#[test]
fn historical_jira_urls_preserve_normalization_and_refusals() {
    let cases: Vec<Document> =
        serde_json::from_str(include_str!("fixtures/rust-jira-url-reference.json")).unwrap();
    assert!(cases.len() >= 20);
    for case in cases {
        let input: JsonString = case.get("input").unwrap().unwrap();
        let actual = jira_base(&input);
        if let Some(error) = case.get::<String>("error").unwrap() {
            assert_eq!(actual.unwrap_err().to_string(), error, "{input:?}");
        } else {
            assert_eq!(
                actual.unwrap(),
                case.get::<JsonString>("value").unwrap().unwrap(),
                "{input:?}"
            );
        }
    }
}
