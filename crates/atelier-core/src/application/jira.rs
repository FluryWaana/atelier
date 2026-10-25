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
                "La connexion Jira a changé. Recharge les réglages avant de réessayer."
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
        return Err("Paramètres Jira invalides.".into());
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
        return Err("Renseigne l’adresse e-mail du compte Jira Cloud.".into());
    }
    let token = input.token.filter(|token| !token.is_empty());
    if token
        .as_ref()
        .is_some_and(|token| token.len() > 4000 || token.chars().any(char::is_control))
        || (input.clear_token && token.is_some())
    {
        return Err("Jeton Jira invalide.".into());
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
                .ok_or("Révision Jira épuisée.")?,
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
            return Err("Description Jira invalide ou trop longue.".into());
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
        return Err("Document Jira invalide.".into());
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
            return Err("Document Jira trop complexe.".into());
        }
        let kind = node
            .get("type")
            .and_then(serde_json::Value::as_str)
            .ok_or("Nœud Jira invalide.")?;
        match kind {
            "text" => append(
                &mut out,
                node["text"].as_str().ok_or("Texte Jira invalide.")?,
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
                    .ok_or("Attribut Jira invalide.")?;
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
            let children = content.as_array().ok_or("Contenu Jira invalide.")?;
            if nodes + stack.len() + children.len() > 8192 {
                return Err("Document Jira trop complexe.".into());
            }
            stack.extend(children.iter().rev().map(|child| (child, false)));
        }
    }
    Ok(out.trim_end_matches('\n').into())
}

pub fn unresolved(fields: &crate::document::Document) -> Result<bool, serde_json::Error> {
    use crate::contracts::{JiraTransitionIntent, JiraTransitionState};
    let reading = fields
        .get::<crate::contracts::JiraRefreshPreview>("jiraRefresh")?
        .is_some_and(|preview| preview.state == crate::contracts::JiraRefreshState::Reading);
    Ok(reading
        || fields
            .get::<JiraTransitionIntent>("jiraTransition")?
            .is_some_and(|intent| {
                matches!(
                    intent.state,
                    JiraTransitionState::Preparing
                        | JiraTransitionState::Sending
                        | JiraTransitionState::Uncertain
                )
            }))
}
pub fn linked_issue(card: &crate::contracts::Card, base: &str) -> Result<String, JiraFailure> {
    let ticket = card
        .fields
        .get::<crate::document::Document>("ticket")
        .map_err(|_| JiraFailure::Invalid("Liaison Jira invalide."))?
        .ok_or("Cette carte n’est pas liée à Jira.")?;
    let site = ticket
        .get::<String>("instance")
        .ok()
        .flatten()
        .or_else(|| ticket.get::<String>("base").ok().flatten());
    let id = ticket
        .get::<String>("id")
        .ok()
        .flatten()
        .ok_or("Identifiant Jira invalide.")?;
    let key = ticket
        .get::<String>("key")
        .ok()
        .flatten()
        .ok_or("Clé Jira invalide.")?;
    if !ticket.string_equals("connector", "jira").unwrap_or(false)
        || site.as_deref() != Some(base)
        || !issue_identity(&id, &key)
    {
        return Err("Reconnecte le site Jira associé à cette carte.".into());
    }
    Ok(id)
}
