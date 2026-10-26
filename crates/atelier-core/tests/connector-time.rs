use atelier_core::application::extensions::time_proposal;
use serde_json::json;

#[test]
fn time_proposals_require_a_complete_bounded_human_entry_and_destination() {
    let valid = json!({"source":{"id":"synthetic-entry","cardId":"synthetic-card","cardTitle":"Synthetic task","ticket":null,"seconds":125,"date":"2026-10-26","startTime":"10:15:00","description":"Human work"},"instance":"https://synthetic.example.test/time","account":"synthetic-account","reference":"SYN-1"});
    assert_eq!(time_proposal(&valid).unwrap().source.seconds, 125);
    for (pointer, value) in [
        ("/source/seconds", json!(0)),
        ("/source/seconds", json!(86401)),
        ("/source/seconds", json!(1.5)),
        ("/source/date", json!("2026-02-30")),
        ("/account", json!("")),
        ("/reference", json!("x".repeat(2001))),
    ] {
        let mut invalid = valid.clone();
        *invalid.pointer_mut(pointer).unwrap() = value;
        assert!(time_proposal(&invalid).is_err(), "{pointer}");
    }
    let mut unknown = valid;
    unknown["source"]["unknown"] = json!("not accepted");
    assert!(time_proposal(&unknown).is_err());
}
