#![cfg(windows)]
use atelier_app::infrastructure::agent_diagnostics::diagnose;
use atelier_core::{application::agent_account::State, document::Document};
use serde_json::json;
#[tokio::test]
async fn compiled_synthetic_accounts_only_report_explicit_scoped_windows_and_never_raw_credentials()
{
    for (marker, state) in [
        ("atelier-r03-synthetic-only", State::Available),
        ("atelier-r03-synthetic-malformed", State::Unavailable),
        ("atelier-r03-synthetic-failure", State::Disconnected),
    ] {
        let root = tempfile::tempdir().unwrap();
        let binary = root.path().join("synthetic-codex.exe");
        std::fs::copy(env!("CARGO_BIN_EXE_native-fixture"), &binary).unwrap();
        std::fs::write(root.path().join(".agent-run-fixture"), marker).unwrap();
        let config = root.path().join("agents-ui-config");
        std::fs::create_dir(&config).unwrap();
        let profile:Document=serde_json::from_str(&json!({"id":"synthetic-account","type":"codex","auth":"cli","command":binary,"configDir":config}).to_string()).unwrap();
        let diagnostic = diagnose(profile).await.unwrap();
        let report = diagnostic.account_report.as_ref().unwrap();
        assert_eq!(report.state, state);
        assert!(!report.observed_at.is_empty());
        if state == State::Available {
            assert_eq!(report.windows[0].remaining_tenths, 750);
            assert_eq!(report.account.as_deref(), Some("synthetic@example.invalid"));
        }
        let public = serde_json::to_string(&diagnostic).unwrap();
        for secret in [
            "private-account-output",
            "private-quota-output",
            "private-output-must-not-be-returned",
        ] {
            assert!(!public.contains(secret));
        }
    }
}
