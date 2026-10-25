use atelier_core::application::preparation::{criteria, parse};
use serde_json::json;
#[test]
fn preparation_never_validates_criteria_and_rejects_incomplete_or_excessive_reports() {
    let value = json!({"criteria":["Observable result 🦀","Second result"],"questions":["Clarify the limit"]});
    let parsed = parse(&value.to_string()).unwrap();
    assert!(
        criteria(&parsed)
            .unwrap()
            .iter()
            .all(|criterion| !criterion.done)
    );
    assert!(parse(&format!("```json\n{value}\n```")).is_ok());
    for value in [
        json!({"criteria":[],"questions":[]}),
        json!({"criteria":["x"],"questions":[" "]}),
        json!({"criteria":["x".repeat(501)],"questions":[]}),
        json!({"criteria":["x"],"questions":vec!["q";11]}),
        json!({"criteria":vec!["x";21],"questions":[]}),
        json!({"criteria":["x"],"questions":[],"validated":true}),
    ] {
        assert!(parse(&value.to_string()).is_err());
    }
    assert!(parse(&"x".repeat(64 * 1024 + 1)).is_err());
}
