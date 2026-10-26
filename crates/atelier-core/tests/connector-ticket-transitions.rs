use atelier_core::application::extensions::{ticket_transition_payload, ticket_transitions};
use serde_json::json;
#[test]
fn transition_choices_are_typed_bounded_and_never_default_missing_field_requirements() {
    let valid = json!({"ticket":{"instance":"https://synthetic.example.test/tenant","id":"synthetic-ticket"},"status":"Done","transitions":[{"id":"close","name":"Close synthetic ticket","status":"Closed","requiresFields":false}]});
    assert_eq!(ticket_transitions(&valid).unwrap().transitions.len(), 1);
    let mut duplicate = valid.clone();
    duplicate["transitions"]
        .as_array_mut()
        .unwrap()
        .push(valid["transitions"][0].clone());
    assert!(ticket_transitions(&duplicate).is_err());
    let mut missing = valid.clone();
    missing["transitions"][0]
        .as_object_mut()
        .unwrap()
        .remove("requiresFields");
    assert!(ticket_transitions(&missing).is_err());
    let mut extra = valid.clone();
    extra["transitions"][0]["script"] = json!("never executed");
    assert!(ticket_transitions(&extra).is_err());
    let mut excessive = valid.clone();
    excessive["transitions"]=json!((0..101).map(|index|json!({"id":format!("choice-{index}"),"name":"Synthetic","status":"Done","requiresFields":false})).collect::<Vec<_>>());
    assert!(ticket_transitions(&excessive).is_err());
    let mut payload = json!({"cardId":"synthetic-card","cardTitle":"Human title","cardRevision":"a".repeat(64),"ticket":valid["ticket"],"ticketKey":"SYN-1","fromStatus":"Done","transition":valid["transitions"][0]});
    assert!(ticket_transition_payload(&payload).is_ok());
    payload["transition"]["requiresFields"] = json!(true);
    assert!(ticket_transition_payload(&payload).is_err());
    payload["transition"]["requiresFields"] = json!(false);
    payload["cardRevision"] = json!("changed");
    assert!(ticket_transition_payload(&payload).is_err());
}
