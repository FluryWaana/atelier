use atelier_core::{application::agent_usage::summarize, document::Document};
use serde_json::json;
fn run(value: serde_json::Value) -> Document {
    serde_json::from_str(&value.to_string()).unwrap()
}
#[test]
fn usage_groups_provider_profile_and_model_and_reports_coverage_without_inventing_zeroes() {
    let summary=summarize(&[
        run(json!({"id":"one","agentType":"codex","agent":"profile","model":"synthetic-model","activeMilliseconds":1500,"usage":{"inputTokens":10,"cachedInputTokens":4,"outputTokens":3}})),
        run(json!({"id":"two","agentType":"codex","agent":"profile","model":"synthetic-model","usage":{"inputTokens":5,"cachedInputTokens":null,"cacheCreationInputTokens":null,"outputTokens":2}})),
        run(json!({"id":"three","agentType":"codex","agent":"profile","model":"synthetic-model"})),
        run(json!({"id":"four","agentType":"claude","agent":"profile","model":"synthetic-model","usage":{"inputTokens":8,"cachedInputTokens":2,"cacheCreationInputTokens":7,"outputTokens":4}})),
        run(json!({"id":"verification","purpose":"verification","agentType":"verification","status":"completed"})),
    ]).unwrap();
    assert_eq!(summary.len(), 2);
    let codex = summary
        .iter()
        .find(|summary| summary.agent_type == "codex")
        .unwrap();
    assert_eq!(codex.runs, 3);
    assert_eq!(codex.input.total, 15);
    assert_eq!(codex.input.samples, 2);
    assert_eq!(codex.cache_read.total, 4);
    assert_eq!(codex.cache_read.samples, 1);
    assert_eq!(codex.cache_creation.samples, 0);
    assert_eq!(codex.active_milliseconds.total, 1500);
    assert_eq!(codex.active_milliseconds.samples, 1);
    let claude = summary
        .iter()
        .find(|summary| summary.agent_type == "claude")
        .unwrap();
    assert_eq!(claude.input.total, 8);
    assert_eq!(claude.cache_creation.total, 7);
}
#[test]
fn invalid_or_overflowing_metrics_and_duplicate_sessions_fail_instead_of_undercounting() {
    for bad in [
        json!({"id":"bad","usage":{"inputTokens":-1}}),
        json!({"id":"bad","usage":{"inputTokens":"10"}}),
        json!({"id":"bad","activeMilliseconds":1.5}),
    ] {
        assert!(summarize(&[run(bad)]).is_err());
    }
    let first = run(json!({"id":"one","usage":{"inputTokens":u64::MAX}}));
    let next = run(json!({"id":"two","usage":{"inputTokens":1}}));
    assert!(summarize(&[first.clone(), next]).is_err());
    assert!(summarize(&[first.clone(), first]).is_err());
}
