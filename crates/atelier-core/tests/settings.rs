use atelier_core::{
    application::settings::{config, prepare_update, public_config, remove_token},
    document::{Document, JsonString},
    domain::text_units,
};
#[path = "support/json.rs"]
mod json;

#[test]
fn global_jira_mappings_require_exact_mapping_and_connection_preconditions() {
    let stored:Document=serde_json::from_str(r#"{"jiraConnection":{"mode":"cloud","url":"https://synthetic.example.test","email":"synthetic@example.test","query":"","revision":7}}"#).unwrap();
    let body:Document=serde_json::from_str(r#"{"jiraAutoTransition":true,"jiraStatusMap":{"doing":"31"},"expected":{"jiraAutoTransition":false,"jiraStatusMap":{}},"expectedJiraRevision":7}"#).unwrap();
    let prepare = |stored: &Document, body: &Document| {
        prepare_update(
            stored,
            &Document::default(),
            body,
            |_| Err("No repository access expected".into()),
            |value| Ok(value.clone()),
        )
    };
    let saved = prepare(&stored, &body).unwrap();
    assert_eq!(
        saved.settings.get::<bool>("jiraAutoTransition").unwrap(),
        Some(true)
    );
    assert!(prepare(&saved.settings, &body).is_err());
    let mut stale = body.clone();
    stale.set("expectedJiraRevision", &6).unwrap();
    assert!(prepare(&stored, &stale).is_err());
    let mut invalid = body.clone();
    invalid
        .set(
            "expected",
            &serde_json::json!({"jiraAutoTransition":"false","jiraStatusMap":{}}),
        )
        .unwrap();
    assert!(prepare(&stored, &invalid).is_err());
}

#[test]
fn historical_settings_preserve_credentials_utf16_fields_and_validation_order() {
    let reference: Document =
        serde_json::from_str(include_str!("fixtures/rust-settings-reference.json")).unwrap();
    let mut defaults: Document = reference.get("defaults").unwrap().unwrap();
    // Generic integration defaults extend the current contract, not the historical fixture.
    defaults.set("ticketProvider", &"local").unwrap();
    defaults.set("timeProvider", &"local").unwrap();
    let cases: Vec<Document> = reference.get("cases").unwrap().unwrap();
    assert!(cases.len() >= 40);
    for case in cases {
        let name: String = case.get("name").unwrap().unwrap();
        let stored: Document = case.get("stored").unwrap().unwrap_or_default();
        let tokens: Document = case.get("tokens").unwrap().unwrap_or_default();
        let body: Document = case.get("body").unwrap().unwrap();
        let result = prepare_update(
            &stored,
            &tokens,
            &body,
            |raw| {
                let value = serde_json::from_str::<JsonString>(raw.get())
                    .map_err(|_| "message.invalid_or_overly_long_text")?;
                let value = text_units(&value, 2000)?;
                if value == JsonString::from("missing") {
                    return Err("Dépôt synthétique absent.".into());
                }
                let mut resolved = JsonString::from("resolved:");
                resolved.0.extend(value.0);
                Ok(resolved)
            },
            |value| {
                if value == &JsonString::from("invalid") {
                    return Err("Adresse synthétique invalide.".into());
                }
                let mut value = value.clone();
                if value.0.last() == Some(&u16::from(b'/')) {
                    value.0.pop();
                }
                Ok(value)
            },
        );
        if let Some(error) = case.get::<String>("error").unwrap() {
            assert_eq!(result.err().expect(&name).to_string(), error, "{name}");
        } else {
            let result = result.unwrap_or_else(|error| panic!("{name}: {error}"));
            let mut expected = defaults.clone();
            expected.overlay(&stored);
            expected.overlay(
                &case
                    .get::<Document>("settings")
                    .unwrap()
                    .unwrap_or_default(),
            );
            for key in case.get::<Vec<String>>("omit").unwrap().unwrap_or_default() {
                expected.remove(&key);
            }
            let actual = serde_json::value::to_raw_value(&result.settings).unwrap();
            let expected = serde_json::value::to_raw_value(&expected).unwrap();
            assert!(
                json::equivalent(&actual, &expected),
                "{name}\nactual: {actual}\nexpected: {expected}"
            );
            let credentials = case
                .get::<Document>("credentials")
                .unwrap()
                .unwrap_or(tokens);
            assert!(
                json::equivalent(
                    &serde_json::value::to_raw_value(&result.tokens).unwrap(),
                    &serde_json::value::to_raw_value(&credentials).unwrap()
                ),
                "{name}: credentials"
            );
        }
    }
}

#[test]
fn public_flags_and_removal_never_copy_vault_values_into_settings() {
    let stored: Document = serde_json::from_str(r#"{"future":{"\ud800":"\udc00"}}"#).unwrap();
    let tokens: Document = serde_json::from_str(
        r#"{"jira":"synthetic-secret","azure":[],"tempo":"","future":"keep"}"#,
    )
    .unwrap();
    let public = public_config(&stored, &tokens, "fixture").unwrap();
    assert_eq!(public.get::<bool>("tokenSet").unwrap(), Some(true));
    assert_eq!(public.get::<bool>("azureTokenSet").unwrap(), Some(true));
    assert_eq!(public.get::<bool>("tempoTokenSet").unwrap(), Some(false));
    assert!(
        !serde_json::to_string(&public)
            .unwrap()
            .contains("synthetic-secret")
    );
    let removed = remove_token(&tokens, "jira").unwrap();
    assert!(removed.string_equals("jira", "").unwrap());
    assert!(removed.string_equals("future", "keep").unwrap());
    assert!(tokens.string_equals("jira", "synthetic-secret").unwrap());
    assert_eq!(
        remove_token(&tokens, "other").unwrap_err().to_string(),
        "message.unknown_service"
    );
    assert!(config(&stored).unwrap().raw("future").is_some());
}
