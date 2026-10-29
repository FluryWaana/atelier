use atelier_core::contracts::AgentDiagnostic;
use serde_json::json;

#[test]
fn latest_check_wins_and_profile_changes_invalidate_the_result() {
    use atelier_core::{application::agent_status::Checks, document::Document};
    let profile: Document = serde_json::from_str(r#"{"id":"claude","revision":1}"#).unwrap();
    let status: AgentDiagnostic = serde_json::from_value(json!({"executable":"synthetic", "version":"1.0.0", "versionError":null,"authentication":"present","authenticationError":null,"account":null,"authMethod":"cli"})).unwrap();
    let mut checks = Checks::default();
    let old = checks.begin("claude", &profile);
    let fresh = checks.begin("claude", &profile);
    checks.finish("claude", fresh, Some(status.clone()));
    checks.finish("claude", old, None);
    assert!(
        checks
            .diagnostic("claude", &profile)
            .unwrap()
            .cli_connected()
    );
    let public: Document = serde_json::from_str(
        r#"{"id":"claude","revision":1,"apiKeySet":false,"updating":false,"updatePhase":""}"#,
    )
    .unwrap();
    assert!(
        checks
            .diagnostic("claude", &public)
            .unwrap()
            .cli_connected()
    );
    let changed: Document = serde_json::from_str(r#"{"id":"claude","revision":2}"#).unwrap();
    assert!(checks.diagnostic("claude", &changed).is_none());
    assert!(checks.diagnostic("another-profile", &profile).is_none());
    let retry = checks.begin("claude", &changed);
    assert!(checks.diagnostic("claude", &changed).is_none());
    checks.finish("claude", retry, None);
    checks.finish("claude", fresh, Some(status));
    assert!(checks.diagnostic("claude", &changed).is_none());
}

#[test]
fn connected_cli_is_not_marked_offline_by_optional_quota_or_version_checks() {
    for quota in ["available", "unavailable", "disconnected", "rateLimited"] {
        let mut value = json!({
            "executable":"synthetic-claude", "version":null,
            "versionError":"synthetic version unavailable",
            "authentication":"present", "authenticationError":null,
            "account":null, "authMethod":"cli",
            "accountReport":{"state":quota,"account":null,"subscription":null,"windows":[],"observedAt":""}
        });
        let status: AgentDiagnostic = serde_json::from_value(value.clone()).unwrap();
        assert!(status.cli_connected(), "quota: {quota}");
        for auth in ["absent", "unavailable", "configured", "missingKey"] {
            value["authentication"] = json!(auth);
            let status: AgentDiagnostic = serde_json::from_value(value.clone()).unwrap();
            assert!(!status.cli_connected(), "authentication: {auth}");
        }
        value["authentication"] = json!("present");
        value["authenticationError"] = json!("synthetic inconsistent response");
        let status: AgentDiagnostic = serde_json::from_value(value).unwrap();
        assert!(!status.cli_connected());
    }
}
