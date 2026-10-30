use atelier_core::{
    application::jira::{creation_text, project_key, validate_creation},
    contracts::{Card, JiraCreationMetadata},
};
use serde_json::json;

fn card() -> Card {
    serde_json::from_str(&json!({"id":"synthetic","title":"Résumé émoji 🦀","status":"todo","description":"Description\nDeuxième ligne","checklist":[{"text":"Review humaine","done":true},{"text":"Autre critère","done":false}],"instructions":"Instruction synthétique"}).to_string()).unwrap()
}
fn metadata() -> JiraCreationMetadata {
    serde_json::from_str(&json!({"revision":1,"projectKey":"SYN","issueType":{"id":"1","name":"Task","subtask":false},"fields":[{"id":"summary","name":"Summary","required":true,"hasDefault":false,"canSet":true},{"id":"description","name":"Description","required":false,"hasDefault":false,"canSet":true}]}).to_string()).unwrap()
}
#[test]
fn creation_preserves_recorded_content_and_refuses_unrepresentable_required_fields() {
    let card = card();
    let text = creation_text(&card).unwrap();
    assert!(text.starts_with("Description\nDeuxième ligne"));
    assert!(text.contains("- [x] Review humaine\n- [ ] Autre critère"));
    assert!(text.ends_with("Instructions pour l’agent :\nInstruction synthétique"));
    let mut metadata = metadata();
    assert!(validate_creation(&metadata, &card).is_ok());
    let mut required = metadata.fields[0].clone();
    required.id = "customfield_10000".into();
    metadata.fields.push(required);
    assert!(validate_creation(&metadata, &card).is_err());
    metadata.fields[2].has_default = true;
    assert!(validate_creation(&metadata, &card).is_ok());
    metadata.fields[1].can_set = false;
    assert!(validate_creation(&metadata, &card).is_err());
    metadata.fields[1].can_set = true;
    metadata.issue_type.subtask = true;
    assert!(validate_creation(&metadata, &card).is_err());
}
#[test]
fn creation_rejects_invalid_unicode_controls_and_oversized_content_without_truncation() {
    let metadata = metadata();
    for title in [" ".to_owned(), "line\nbreak".into(), "🦀".repeat(151)] {
        let mut card = card();
        card.title = title.as_str().into();
        assert!(validate_creation(&metadata, &card).is_err());
    }
    let mut card = card();
    assert!(serde_json::from_str::<Card>(r#"{"id":"synthetic","title":"\ud800"}"#).is_err());
    card.fields.set("description", &"x".repeat(60_001)).unwrap();
    assert!(creation_text(&card).is_err());
    card.fields.set("description", &"hidden\0content").unwrap();
    assert!(creation_text(&card).is_err());
    for key in ["", "syn", "SYN/other", "1SYN", "SYN-1"] {
        assert!(!project_key(key));
    }
    assert!(project_key("SYN_01"));
}
