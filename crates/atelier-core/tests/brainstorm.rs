use atelier_core::application::brainstorm::{Exchange, parse, prompt};
use serde_json::json;
fn answer() -> serde_json::Value {
    json!({"message":"Explorons l’idée 🦀","questions":[{"question":"Quel périmètre ?","options":["Local","Équipe"]}],"tickets":[{"title":"Proposition","description":"Résultat observable","criteria":["Critère humain"],"questions":["Question conservée"]}]})
}
#[test]
fn brainstorm_validates_the_complete_response_without_extracting_partial_tickets() {
    let valid = answer();
    assert!(parse(&valid.to_string()).is_ok());
    assert!(parse(&format!("```json\n{valid}\n``` ")).is_ok());
    let mut conversation = valid.clone();
    conversation["tickets"] = json!([]);
    assert!(parse(&conversation.to_string()).is_ok());
    for bad in [
        json!(null),
        json!({"message":"No tickets property","questions":[]}),
        {
            let mut bad = valid.clone();
            bad["tickets"][0]["criteria"] = json!([]);
            bad
        },
        {
            let mut bad = valid.clone();
            bad["questions"][0]["options"] = json!(["same", " same "]);
            bad
        },
        {
            let mut bad = valid.clone();
            bad["tickets"][0]["description"] = json!("x".repeat(12_001));
            bad
        },
        {
            let mut bad = valid.clone();
            bad["message"] = json!("hidden\0message");
            bad
        },
    ] {
        assert!(parse(&bad.to_string()).is_err());
    }
    assert!(parse(&format!("{valid} trailing data")).is_err());
    assert!(parse(&"x".repeat(64 * 1024 + 1)).is_err());
}
#[test]
fn conversation_keeps_prior_decisions_and_bounds_history_without_silent_truncation() {
    let answer = parse(&answer().to_string()).unwrap();
    let history = vec![Exchange {
        user: "Première décision".into(),
        assistant: answer,
    }];
    let instructions = prompt("Idée", "Contexte", &history, "Découpe en tâches").unwrap();
    assert!(instructions.contains("Première décision"));
    assert!(instructions.contains("Découpe en tâches"));
    assert!(instructions.contains("lecture seule"));
    assert!(prompt("Idée", "", &history, &"x".repeat(12_001)).is_err());
    assert!(prompt("Idée", "", &vec![history[0].clone(); 65], "Suite").is_err());
    assert!(prompt("Idée", "", &history, &" ".repeat(4)).is_err());
}
