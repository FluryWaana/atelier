use atelier_core::{
    application::agent_cli::{self, Phase},
    document::Document,
};
use serde_json::json;

fn settings(phase: &str) -> Document {
    serde_json::from_str(
        &json!({"agentCliUpdate": {
            "id":"cli-synthetic", "provider":"synthetic", "phase":phase,
            "info":{"package":"@openai/codex","availability":"available",
                "latestVersion":"2.0.0","installedVersion":"1.0.0",
                "managedRoot":null,"image":null,"observedAt":"2026-10-26T00:00:00Z"},
            "startedAt":"before","updatedAt":"before","actualVersion":if phase=="observed"{json!("2.0.0")}else{json!(null)},
            "targetReached":if phase=="observed"{json!(true)}else{json!(null)},"message":"synthetic"
        }})
        .to_string(),
    )
    .unwrap()
}

#[test]
fn interruption_never_replays_or_claims_an_update_succeeded() {
    for (before, after) in [("preparing", Phase::Failed), ("running", Phase::Uncertain)] {
        let mut settings = settings(before);
        assert!(agent_cli::busy(&settings).unwrap());
        assert!(agent_cli::recover(&mut settings, "after").unwrap());
        let operation = agent_cli::operation(&settings).unwrap().unwrap();
        assert_eq!(operation.phase, after);
        assert_eq!(operation.updated_at, "after");
        assert!(operation.actual_version.is_none());
        assert!(operation.target_reached.is_none());
        assert!(!agent_cli::busy(&settings).unwrap());
        assert!(!agent_cli::recover(&mut settings, "later").unwrap());
    }
}

#[test]
fn terminal_results_are_preserved_and_corruption_blocks_mutation() {
    for phase in ["observed", "failed", "uncertain"] {
        let mut settings = settings(phase);
        let original = settings.clone();
        assert!(!agent_cli::recover(&mut settings, "after").unwrap());
        assert_eq!(
            serde_json::to_string(&settings).unwrap(),
            serde_json::to_string(&original).unwrap()
        );
    }
    let mut settings = settings("running");
    settings
        .set("agentCliUpdate", &json!({"phase":"running"}))
        .unwrap();
    assert!(agent_cli::busy(&settings).is_err());
    settings.set("agentCliUpdate", &"x".repeat(65_537)).unwrap();
    assert!(agent_cli::recover(&mut settings, "after").is_err());
}

#[test]
fn an_observation_cannot_exist_without_its_actual_version_or_leak_into_a_running_state() {
    for phase in ["observed", "running"] {
        let mut settings = settings(phase);
        let mut operation = agent_cli::operation(&settings).unwrap().unwrap();
        if phase == "observed" {
            operation.actual_version = None;
        } else {
            operation.actual_version = Some("2.0.0".into());
            operation.target_reached = Some(true);
        }
        settings.set("agentCliUpdate", &operation).unwrap();
        assert!(agent_cli::operation(&settings).is_err());
        assert!(agent_cli::busy(&settings).is_err());
        assert!(agent_cli::recover(&mut settings, "after").is_err());
    }
}
