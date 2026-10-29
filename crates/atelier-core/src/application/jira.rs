use crate::contracts::{JiraConfiguration, JiraConfigurationUpdate, JiraMode};

#[derive(Debug)]
pub enum JiraFailure {
    Conflict,
    OperationConflict(&'static str),
    Invalid(&'static str),
}
impl From<&'static str> for JiraFailure {
    fn from(message: &'static str) -> Self {
        Self::Invalid(message)
    }
}
impl std::fmt::Display for JiraFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Conflict => {
                "message.the_jira_connection_changed_reload_settings_before_trying_again"
            }
            Self::Invalid(message) | Self::OperationConflict(message) => message,
        })
    }
}
impl std::error::Error for JiraFailure {}

pub fn configure(
    previous: &JiraConfiguration,
    input: JiraConfigurationUpdate,
    normalize_url: impl FnOnce(&str) -> Result<String, &'static str>,
) -> Result<(JiraConfiguration, Option<String>), JiraFailure> {
    if previous.revision != input.expected_revision {
        return Err(JiraFailure::Conflict);
    }
    if input.email.len() > 300
        || input.email.chars().any(char::is_control)
        || input.query.len() > 16000
        || input.query.chars().count() > 4000
        || input.query.contains('\0')
    {
        return Err("message.invalid_jira_settings".into());
    }
    let url = if input.url.trim().is_empty() {
        String::new()
    } else {
        normalize_url(input.url.trim())?
    };
    let email = input.email.trim().to_owned();
    if input.mode == JiraMode::Cloud
        && !url.is_empty()
        && (email.is_empty() || !email.contains('@') || email.contains(':'))
    {
        return Err("message.enter_the_jira_cloud_account_email_address".into());
    }
    let token = input.token.filter(|token| !token.is_empty());
    if token
        .as_ref()
        .is_some_and(|token| token.len() > 4000 || token.chars().any(char::is_control))
        || (input.clear_token && token.is_some())
    {
        return Err("message.invalid_jira_token".into());
    }
    let scope_changed =
        previous.url != url || previous.email != email || previous.mode != input.mode;
    let token = if input.clear_token {
        Some(String::new())
    } else {
        token.or_else(|| scope_changed.then(String::new))
    };
    Ok((
        JiraConfiguration {
            mode: input.mode,
            url,
            email,
            query: input.query.trim().into(),
            revision: previous
                .revision
                .checked_add(1)
                .ok_or("message.jira_revision_limit_reached")?,
        },
        token,
    ))
}

pub fn issue_identity(id: &str, key: &str) -> bool {
    let Some((project, number)) = key.rsplit_once('-') else {
        return false;
    };
    !id.is_empty()
        && id.len() <= 32
        && id.bytes().all(|byte| byte.is_ascii_digit())
        && !project.is_empty()
        && project.len() <= 64
        && project.as_bytes()[0].is_ascii_alphabetic()
        && project
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        && !number.is_empty()
        && number.len() <= 20
        && number.bytes().all(|byte| byte.is_ascii_digit())
}

/// Text projection; rich formatting and attachments remain on the Jira site.
pub fn description(value: &serde_json::Value) -> Result<String, JiraFailure> {
    fn append(out: &mut String, text: &str) -> Result<(), JiraFailure> {
        if text.contains('\0') || out.encode_utf16().count() + text.encode_utf16().count() > 20_000
        {
            return Err("message.invalid_or_oversized_jira_description".into());
        }
        out.push_str(text);
        Ok(())
    }
    if value.is_null() {
        return Ok(String::new());
    }
    if let Some(text) = value.as_str() {
        let mut out = String::new();
        append(&mut out, text)?;
        return Ok(out);
    }
    if value["type"] != "doc" || value["version"] != 1 || !value["content"].is_array() {
        return Err("message.invalid_jira_document".into());
    }
    let mut out = String::new();
    let mut stack = vec![(value, false)];
    let mut nodes = 0usize;
    while let Some((node, closing)) = stack.pop() {
        if closing {
            if !out.ends_with('\n') {
                append(&mut out, "\n")?;
            }
            continue;
        }
        nodes += 1;
        if nodes > 4096 {
            return Err("message.jira_document_is_too_complex".into());
        }
        let kind = node
            .get("type")
            .and_then(serde_json::Value::as_str)
            .ok_or("message.invalid_jira_node")?;
        match kind {
            "text" => append(
                &mut out,
                node["text"].as_str().ok_or("message.invalid_jira_text")?,
            )?,
            "hardBreak" => append(&mut out, "\n")?,
            "mention" | "emoji" | "inlineCard" | "status" => {
                let attr = if kind == "inlineCard" { "url" } else { "text" };
                let text = node["attrs"][attr]
                    .as_str()
                    .or_else(|| {
                        if kind == "emoji" {
                            node["attrs"]["shortName"].as_str()
                        } else {
                            None
                        }
                    })
                    .ok_or("message.invalid_jira_attribute")?;
                append(&mut out, text)?;
            }
            _ => {}
        }
        if matches!(
            kind,
            "paragraph" | "heading" | "listItem" | "codeBlock" | "tableRow" | "blockquote"
        ) {
            stack.push((node, true));
        }
        if let Some(content) = node.get("content") {
            let children = content.as_array().ok_or("message.invalid_jira_content")?;
            if nodes + stack.len() + children.len() > 8192 {
                return Err("message.jira_document_is_too_complex".into());
            }
            stack.extend(children.iter().rev().map(|child| (child, false)));
        }
    }
    Ok(out.trim_end_matches('\n').into())
}

pub fn unresolved(fields: &crate::document::Document) -> Result<bool, serde_json::Error> {
    use crate::contracts::{JiraTransitionIntent, JiraWriteState};
    let reading = fields
        .get::<crate::contracts::JiraRefreshPreview>("jiraRefresh")?
        .is_some_and(|preview| preview.state == crate::contracts::JiraRefreshState::Reading);
    let creating = fields
        .get::<crate::contracts::JiraCreationIntent>("jiraCreation")?
        .is_some_and(|intent| {
            matches!(
                intent.state,
                crate::contracts::JiraWriteState::Preparing
                    | crate::contracts::JiraWriteState::Sending
                    | crate::contracts::JiraWriteState::Uncertain
            )
        });
    Ok(reading
        || creating
        || fields
            .get::<JiraTransitionIntent>("jiraTransition")?
            .is_some_and(|intent| {
                matches!(
                    intent.state,
                    JiraWriteState::Preparing | JiraWriteState::Sending | JiraWriteState::Uncertain
                )
            }))
}
pub fn linked_issue(card: &crate::contracts::Card, base: &str) -> Result<String, JiraFailure> {
    let ticket = card
        .fields
        .get::<crate::document::Document>("ticket")
        .map_err(|_| JiraFailure::Invalid("message.invalid_jira_link"))?
        .ok_or("message.this_card_is_not_linked_to_jira")?;
    let site = ticket
        .get::<String>("instance")
        .ok()
        .flatten()
        .or_else(|| ticket.get::<String>("base").ok().flatten());
    let id = ticket
        .get::<String>("id")
        .ok()
        .flatten()
        .ok_or("message.invalid_jira_issue_identifier")?;
    let key = ticket
        .get::<String>("key")
        .ok()
        .flatten()
        .ok_or("message.invalid_jira_issue_key")?;
    if !ticket.string_equals("connector", "jira").unwrap_or(false)
        || site.as_deref() != Some(base)
        || !issue_identity(&id, &key)
    {
        return Err("message.reconnect_the_jira_site_associated_with_this_card".into());
    }
    Ok(id)
}

/// Read-only previews are cached separately from the editable card precondition.
pub fn card_snapshot(card: &crate::contracts::Card) -> crate::contracts::Card {
    let mut card = card.clone();
    card.fields.remove("jiraRefresh");
    card
}

pub fn project_key(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value.as_bytes()[0].is_ascii_uppercase()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
}
pub fn creation_text(card: &crate::contracts::Card) -> Result<String, JiraFailure> {
    let mut text = card
        .fields
        .get::<String>("description")
        .map_err(|_| JiraFailure::Invalid("message.card_description_invalid"))?
        .unwrap_or_default();
    let criteria = card
        .fields
        .get::<Vec<crate::domain::checklist::Criterion>>("checklist")
        .map_err(|_| JiraFailure::Invalid("message.card_criteria_invalid"))?
        .unwrap_or_default();
    let checklist = crate::domain::checklist::checklist_text(&criteria)
        .utf8()
        .map_err(|_| JiraFailure::Invalid("message.card_criteria_invalid"))?;
    text.push_str(&checklist);
    let instructions = card
        .fields
        .get::<String>("instructions")
        .map_err(|_| JiraFailure::Invalid("message.card_instructions_invalid"))?
        .unwrap_or_default();
    if !instructions.is_empty() {
        text.push_str("\n\nInstructions pour l’agent :\n");
        text.push_str(&instructions);
    }
    if text.encode_utf16().count() > 60_000 || text.contains('\0') {
        return Err("message.card_content_is_too_large_or_invalid_for_jira".into());
    }
    Ok(text)
}

pub fn validate_creation(
    metadata: &crate::contracts::JiraCreationMetadata,
    card: &crate::contracts::Card,
) -> Result<(), JiraFailure> {
    let title = card
        .title
        .utf8()
        .map_err(|_| JiraFailure::Invalid("message.card_title_invalid"))?;
    if title.trim().is_empty()
        || title.encode_utf16().count() > 255
        || title.chars().any(char::is_control)
    {
        return Err("message.card_title_invalid".into());
    }
    let description = creation_text(card)?;
    if metadata.issue_type.subtask
        || !metadata
            .fields
            .iter()
            .any(|field| field.id == "summary" && field.can_set)
    {
        return Err("message.this_issue_type_cannot_be_created_from_atelier".into());
    }
    if !description.is_empty()
        && !metadata
            .fields
            .iter()
            .any(|field| field.id == "description" && field.can_set)
    {
        return Err(
            "message.the_jira_project_does_not_allow_this_description_create_the_issu".into(),
        );
    }
    for field in &metadata.fields {
        if field.required
            && !field.has_default
            && ((!field.can_set && ["summary", "description"].contains(&field.id.as_str()))
                || !["project", "issuetype", "summary", "description"].contains(&field.id.as_str()))
        {
            return Err(
                "message.this_project_requires_additional_fields_create_the_issue_in_jira".into(),
            );
        }
        if field.required && field.id == "description" && description.trim().is_empty() {
            return Err("message.this_jira_project_requires_a_description".into());
        }
    }
    Ok(())
}
