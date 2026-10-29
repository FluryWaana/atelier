use super::task_creation::{Result, options, present, provider, text};
use crate::{
    document::{Document, JsonString},
    domain::tasks::STATUSES,
};
use serde_json::value::RawValue;

#[derive(Debug)]
pub enum SettingsFailure {
    UnknownService,
    Invalid(&'static str),
    Conflict,
}
impl std::fmt::Display for SettingsFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::UnknownService => "message.unknown_service",
            Self::Invalid(message) => message,
            Self::Conflict => {
                "message.these_settings_changed_elsewhere_your_draft_is_preserved_close_a"
            }
        })
    }
}
impl std::error::Error for SettingsFailure {}

pub fn config(stored: &Document) -> Result<Document> {
    let mut config: Document = serde_json::from_str(
        r#"{"repo":"","agent":"codex","baseBranch":"HEAD","jiraType":"cloud","jiraUrl":"","jiraEmail":"","jql":"assignee = currentUser() AND sprint in openSprints() ORDER BY updated DESC","jiraAutoTransition":false,"jiraStatusMap":{}}"#,
    )?;
    config.overlay(stored);
    Ok(config)
}

pub fn public_config(stored: &Document, tokens: &Document, platform: &str) -> Result<Document> {
    let mut config = config(stored)?;
    config.remove("jiraImportReservation");
    config.remove("connectorEffects");
    config.remove("connectorTicketResetPreview");
    config.remove("agentCliUpdate");
    config.set("platform", &platform)?;
    for (service, flag) in [
        ("jira", "tokenSet"),
        ("azure", "azureTokenSet"),
        ("tempo", "tempoTokenSet"),
    ] {
        config.set(flag, &tokens.truthy(service)?)?;
    }
    Ok(config)
}

pub fn remove_token(tokens: &Document, service: &str) -> Result<Document> {
    if !["jira", "azure", "tempo"].contains(&service) {
        return Err(SettingsFailure::UnknownService.into());
    }
    let mut next = tokens.clone();
    next.set(service, &"")?;
    Ok(next)
}

pub struct SettingsUpdate {
    pub settings: Document,
    pub tokens: Document,
}

// A truthy JSON object/array is never strictly equal to a separately supplied object/array.
fn same_mapping_value(left: Option<&RawValue>, right: Option<&RawValue>) -> Result<bool> {
    fn value(raw: Option<&RawValue>) -> Result<Option<Box<RawValue>>> {
        let mut document = Document::default();
        if let Some(raw) = raw {
            document.set("value", &raw)?;
        }
        Ok(document
            .truthy("value")?
            .then(|| raw.expect("truthy field exists").to_owned()))
    }
    let (left, right) = (value(left)?, value(right)?);
    same_value(left.as_deref(), right.as_deref())
}

fn same_value(left: Option<&RawValue>, right: Option<&RawValue>) -> Result<bool> {
    match (left, right) {
        (None, None) => Ok(true),
        (Some(left), Some(right))
            if left.get().starts_with('"') && right.get().starts_with('"') =>
        {
            Ok(serde_json::from_str::<JsonString>(left.get())?
                == serde_json::from_str::<JsonString>(right.get())?)
        }
        (Some(left), Some(right))
            if !left.get().starts_with(['{', '[', '"'])
                && !right.get().starts_with(['{', '[', '"']) =>
        {
            Ok(serde_json::from_str::<serde_json::Value>(left.get())?
                == serde_json::from_str::<serde_json::Value>(right.get())?)
        }
        _ => Ok(false),
    }
}

pub fn prepare_update(
    stored: &Document,
    tokens: &Document,
    supplied: &Document,
    repository: impl Fn(&RawValue) -> Result<JsonString>,
    jira_base: impl Fn(&JsonString) -> Result<JsonString>,
) -> Result<SettingsUpdate> {
    let previous = config(stored)?;
    if let Some(raw) = supplied.raw("expected") {
        let invalid = || SettingsFailure::Invalid("message.invalid_edit_condition");
        let expected: std::collections::BTreeMap<String, serde_json::Value> =
            serde_json::from_str(raw.get()).map_err(|_| invalid())?;
        let fields = [
            "repo",
            "baseBranch",
            "agent",
            "jiraAutoTransition",
            "jiraStatusMap",
        ];
        if expected
            .keys()
            .any(|key| !fields.contains(&key.as_str()) || supplied.raw(key).is_none())
            || fields
                .iter()
                .any(|key| supplied.raw(key).is_some() && !expected.contains_key(*key))
        {
            return Err(invalid().into());
        }
        for (key, value) in expected {
            if (key == "jiraAutoTransition" && !value.is_boolean())
                || (key == "jiraStatusMap" && !value.is_object())
                || (!matches!(key.as_str(), "jiraAutoTransition" | "jiraStatusMap")
                    && !value.is_string())
            {
                return Err(invalid().into());
            }
            if previous.get::<serde_json::Value>(&key)?.as_ref() != Some(&value) {
                return Err(SettingsFailure::Conflict.into());
            }
        }
    }
    if let Some(expected) = supplied.get::<u64>("expectedJiraRevision")? {
        let revision = previous
            .get::<crate::contracts::JiraConfiguration>("jiraConnection")?
            .unwrap_or_default()
            .revision;
        if expected != revision {
            return Err(SettingsFailure::Conflict.into());
        }
    }
    let old_mapping = previous
        .get::<Document>("jiraStatusMap")
        .ok()
        .flatten()
        .unwrap_or_default();
    let supplied_mapping = supplied
        .get::<Document>("jiraStatusMap")
        .ok()
        .flatten()
        .unwrap_or_default();
    let mut new_mapping = false;
    if supplied.raw("jiraStatusMap").is_some() {
        for status in STATUSES {
            new_mapping |=
                !same_mapping_value(supplied_mapping.raw(status), old_mapping.raw(status))?;
        }
    }
    let mut body = previous.clone();
    body.overlay(supplied);
    let mut next = previous.clone();
    if !previous.truthy("taskTemplates")? {
        next.set("taskTemplates", &Vec::<Document>::new())?;
    }
    let repo = if body.truthy("repo")? {
        repository(body.raw("repo").expect("truthy field exists"))?
    } else {
        JsonString::default()
    };
    next.set("repo", &repo)?;
    next.set("jiraType", &body.raw("jiraType"))?;
    let url = if body.truthy("jiraUrl")? {
        jira_base(&text(body.raw("jiraUrl"), 2000)?)?
    } else {
        JsonString::default()
    };
    next.set("jiraUrl", &url)?;
    next.set("jiraEmail", &text(body.raw("jiraEmail"), 300)?)?;
    next.set("jql", &text(body.raw("jql"), 4000)?)?;
    let agent = present(body.raw("agent"))
        .or_else(|| present(previous.raw("agent")))
        .map(|raw| serde_json::from_str::<JsonString>(raw.get()))
        .transpose()
        .map_err(|_| SettingsFailure::Invalid("message.invalid_agent"))?
        .unwrap_or_else(|| JsonString::from("codex"));
    provider(&previous, &agent)?;
    next.set("agent", &agent)?;
    let providers = crate::domain::agent_options::providers(&previous)?;
    let mut updated_providers = Vec::new();
    for mut profile in providers {
        let id = profile
            .get::<JsonString>("id")?
            .ok_or(SettingsFailure::Invalid("message.invalid_agent"))?;
        let mut input = Document::default();
        for (key, suffix) in [("model", "Model"), ("effort", "Effort")] {
            let mut field = id.clone();
            field.0.extend(suffix.encode_utf16());
            if let Some(raw) = present(supplied.raw_key(&field)).or_else(|| profile.raw(key)) {
                input.set(key, &raw)?;
            }
        }
        let selected = options(&id, &input, &previous)?;
        for (key, suffix) in [("model", "Model"), ("effort", "Effort")] {
            let mut field = id.clone();
            field.0.extend(suffix.encode_utf16());
            next.set_key(field, &selected[key])?;
            profile.set(key, &selected[key])?;
        }
        updated_providers.push(profile);
    }
    if previous.truthy("agentProviders")? {
        next.set("agentProviders", &updated_providers)?;
    }
    let mut branch = text(
        present(body.raw("baseBranch")).or_else(|| previous.raw("baseBranch")),
        180,
    )?;
    if branch.0.is_empty() {
        branch = JsonString::from("HEAD");
    }
    next.set("baseBranch", &branch)?;
    let automatic = present(body.raw("jiraAutoTransition"))
        .or_else(|| previous.raw("jiraAutoTransition"))
        .and_then(|raw| serde_json::from_str::<bool>(raw.get()).ok())
        .ok_or(SettingsFailure::Invalid("message.invalid_sync_option"))?;
    next.set("jiraAutoTransition", &automatic)?;
    let mapping = present(body.raw("jiraStatusMap"))
        .or_else(|| previous.raw("jiraStatusMap"))
        .and_then(|raw| serde_json::from_str::<Document>(raw.get()).ok())
        .ok_or(SettingsFailure::Invalid("message.invalid_jira_mapping"))?;
    if mapping.keys().any(|key| {
        !STATUSES
            .iter()
            .any(|status| key == &JsonString::from(*status))
    }) {
        return Err(SettingsFailure::Invalid("message.invalid_jira_mapping").into());
    }
    let empty = RawValue::from_string("\"\"".into())?;
    let mut normalized = Document::default();
    let mut has_status = false;
    for status in STATUSES {
        let value = text(Some(present(mapping.raw(status)).unwrap_or(&empty)), 100)?;
        has_status |= !value.0.is_empty();
        normalized.set(status, &value)?;
    }
    let url_changed = !same_value(previous.raw("jiraUrl"), next.raw("jiraUrl"))?;
    if previous.truthy("jiraUrl")? && url_changed && !new_mapping {
        next.set("jiraAutoTransition", &false)?;
        normalized = Document::default();
    } else if automatic && !has_status {
        return Err(
            SettingsFailure::Invalid("message.enter_at_least_one_target_jira_status").into(),
        );
    }
    next.set("jiraStatusMap", &normalized)?;
    if !next.string_equals("jiraType", "cloud")? && !next.string_equals("jiraType", "server")? {
        return Err(SettingsFailure::Invalid("message.invalid_jira_type").into());
    }
    let mut credentials = tokens.clone();
    if url_changed
        || !same_value(previous.raw("jiraEmail"), next.raw("jiraEmail"))?
        || !same_value(previous.raw("jiraType"), next.raw("jiraType"))?
    {
        credentials.set("jira", &"")?;
    }
    for (service, field, clear) in [
        ("jira", "token", "clearToken"),
        ("azure", "azureToken", "clearAzureToken"),
        ("tempo", "tempoToken", "clearTempoToken"),
    ] {
        if body.truthy(field)? {
            credentials.set(service, &text(body.raw(field), 4000)?)?;
        }
        if body.truthy(clear)? {
            credentials.set(service, &"")?;
        }
    }
    if url_changed && !body.truthy("tempoToken")? {
        credentials.set("tempo", &"")?;
    }
    Ok(SettingsUpdate {
        settings: next,
        tokens: credentials,
    })
}
