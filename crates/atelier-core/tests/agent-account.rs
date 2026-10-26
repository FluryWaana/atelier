use atelier_core::application::agent_account::{Probe, State, windows};
use serde_json::json;
#[test]
fn the_account_probe_only_reads_and_never_requests_login_threads_credits_or_email() {
    let mut probe = Probe::default();
    assert_eq!(probe.start()["method"], "initialize");
    let (messages, report) = probe
        .receive(json!({"id":"account-initialize","result":{}}))
        .unwrap();
    assert!(report.is_none());
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0]["method"], "initialized");
    assert_eq!(messages[1]["method"], "account/read");
    assert_eq!(messages[1]["params"]["refreshToken"], false);
    let (denied,report)=probe.receive(json!({"id":"unexpected","method":"account/chatgptAuthTokens/refresh","params":{"private":"never expose"}})).unwrap();
    assert!(report.is_none());
    assert_eq!(denied[0]["error"]["code"], -32601);
    let (messages,report)=probe.receive(json!({"id":"account-read","result":{"account":{"type":"chatgpt","email":"synthetic@example.invalid","planType":"synthetic-plan","accessToken":"private-token"}}})).unwrap();
    assert!(report.is_none());
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["method"], "account/rateLimits/read");
    let (messages,report)=probe.receive(json!({"id":"account-quotas","result":{"rateLimitsByLimitId":{"synthetic":{"limitId":"synthetic","primary":{"usedPercent":20.4,"windowDurationMins":60,"resetsAt":1800000000},"secondary":null}},"credits":{"private":"never expose"}}})).unwrap();
    assert!(messages.is_empty());
    let report = report.unwrap();
    assert_eq!(report.state, State::Available);
    assert_eq!(report.windows[0].remaining_tenths, 796);
    assert_eq!(report.windows[0].resets_at, Some(1800000000));
    let public = serde_json::to_string(&report).unwrap();
    assert!(!public.contains("private-token"));
    assert!(!public.contains("never expose"));
    assert!(
        probe
            .receive(json!({"id":"account-quotas","result":{}}))
            .is_err()
    );
}
#[test]
fn api_or_missing_accounts_stop_before_requesting_subscription_quotas() {
    for (account, state) in [
        (json!(null), State::Disconnected),
        (json!({"type":"apiKey"}), State::Api),
        (json!({"type":"other"}), State::Unsupported),
    ] {
        let mut probe = Probe::default();
        probe
            .receive(json!({"id":"account-initialize","result":{}}))
            .unwrap();
        let (messages, report) = probe
            .receive(json!({"id":"account-read","result":{"account":account}}))
            .unwrap();
        assert!(messages.is_empty());
        assert_eq!(report.unwrap().state, state);
    }
}
#[test]
fn invalid_windows_fail_the_report_instead_of_claiming_partial_coverage() {
    for used in [json!(-1), json!(101), json!("20"), json!(null)] {
        assert!(windows(&json!({"rateLimits":{"primary":{"usedPercent":used}}})).is_err());
    }
    assert!(windows(&json!({"rateLimitsByLimitId":[]})).is_err());
    assert!(
        windows(
            &json!({"rateLimitsByLimitId":{"one":{"primary":{"usedPercent":20}},"invalid":42}})
        )
        .is_err()
    );
    assert!(windows(&json!({"rateLimits":{"primary":{"usedPercent":0,"resetsAt":-1}}})).is_err());
    let zero = windows(&json!({"rateLimits":{"primary":{"usedPercent":100}}})).unwrap();
    assert_eq!(zero[0].remaining_tenths, 0);
    assert!(zero[0].resets_at.is_none());
    assert!(windows(&json!({})).unwrap().is_empty());
}
