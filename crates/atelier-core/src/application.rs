use crate::{contracts::Card, document::Document};

pub mod agent_account;
pub mod agent_cli;
pub mod agent_protocol;
pub mod agent_providers;
pub mod agent_status;
pub mod agent_usage;
pub mod automation;
pub mod backups;
pub mod brainstorm;
pub mod card_lifecycle;
pub mod extensions;
pub mod forge;
pub mod integrations;
pub mod jira;
pub mod kanban;
pub mod preparation;
pub mod projects;
pub mod reviews;
pub mod services;
pub mod settings;
pub mod task_creation;
pub mod task_editing;
pub mod task_templates;
pub mod time_reports;
pub mod time_tracking;
pub mod updates;
pub mod worktrees;

fn json_content(text: &str) -> &str {
    let text = text.trim();
    text.strip_prefix("```json")
        .or_else(|| text.strip_prefix("```"))
        .and_then(|text| text.strip_suffix("```"))
        .unwrap_or(text)
        .trim()
}

pub fn duplicate_card(card: &Card, id: String, timestamp: String) -> serde_json::Result<Card> {
    let mut units = 0;
    let title = format!("Copie : {}", card.title)
        .chars()
        .take_while(|character| {
            units += character.len_utf16();
            units <= 300
        })
        .collect();
    let mut fields = Document::default();
    for (key, default) in [
        ("description", ""),
        ("repo", ""),
        ("model", ""),
        ("effort", ""),
        ("mode", "edit"),
        ("instructions", ""),
        ("baseBranch", ""),
    ] {
        if card.fields.has_text(key)? {
            if let Some(value) = card.fields.raw(key) {
                fields.set(key, &value)?;
            }
        } else {
            fields.set(key, &default)?;
        }
    }
    if let Some(agent) = card.fields.raw("agent") {
        fields.set("agent", &agent)?;
    }
    fields.set("checklist", &reset_checklist(&card.fields)?)?;
    fields.set("status", &"todo")?;
    fields.set("createdAt", &timestamp)?;
    fields.set("updatedAt", &timestamp)?;
    Ok(Card { id, title, fields })
}

fn reset_checklist(source: &Document) -> serde_json::Result<Vec<Document>> {
    let mut criteria = Vec::new();
    if source.has_list("checklist")? {
        for original in source
            .get::<Vec<Document>>("checklist")?
            .unwrap_or_default()
        {
            let mut criterion = Document::default();
            if let Some(text) = original.raw("text") {
                criterion.set("text", &text)?;
            }
            criterion.set("done", &false)?;
            criteria.push(criterion);
        }
    }
    Ok(criteria)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn json_reports_accept_plain_or_complete_fences_without_repairing_invalid_output() {
        for report in [" {} ", "```json\n{}\n```", "```\n{}\n```"] {
            assert_eq!(json_content(report), "{}");
        }
        for invalid in ["```json\n{}", "{}\n```", "```python\n{}\n```"] {
            assert!(serde_json::from_str::<serde_json::Value>(json_content(invalid)).is_err());
        }
    }

    #[test]
    fn contracts_preserve_additional_fields_and_absence() {
        let value =
            json!({"id":"a", "title":"titre", "nullable":null, "extra":{"values":[false, 0, "é"]}});
        let card: Card = serde_json::from_str(&value.to_string()).unwrap();
        assert_eq!(serde_json::to_value(card).unwrap(), value);
    }

    #[test]
    fn duplication_bounds_unicode_without_creating_an_invalid_string() {
        let card = Card {
            id: "source".into(),
            title: format!("a{}", "🔧".repeat(149)),
            fields: Document::default(),
        };
        let copy = duplicate_card(&card, "copy".into(), "now".into()).unwrap();
        assert!(copy.title.starts_with("Copie : a"));
        assert_eq!(copy.title.encode_utf16().count(), 299);
        assert!(copy.title.ends_with('🔧'));
        assert_eq!(
            serde_json::from_str::<String>(&serde_json::to_string(&copy.title).unwrap()).unwrap(),
            copy.title
        );
    }

    #[test]
    fn duplication_copies_only_editable_content_and_resets_human_criteria() {
        let card: Card = serde_json::from_str(r#"{"id":"old","title":"Source","description":"\u00e9","agent":"work","model":"model","effort":"high","mode":"analyze","instructions":"Keep","repo":"/repo","baseBranch":"main","checklist":[{"text":"\u00e8","done":true,"unknown":1}],"status":"done","archived":true,"blockedBy":["other"],"ticket":{"id":"1"},"worktree":{"path":"/work"},"serviceWorktrees":{"front":{}},"pullRequest":{"id":1},"automation":{"state":"done"},"unknown":"discard"}"#).unwrap();
        let copy = duplicate_card(&card, "new".into(), "now".into()).unwrap();
        assert_eq!(copy.title, String::from("Copie : Source"));
        assert!(copy.fields.string_equals("status", "todo").unwrap());
        assert_eq!(
            copy.fields.get::<String>("description").unwrap(),
            Some("é".to_owned())
        );
        let criteria = copy
            .fields
            .get::<Vec<Document>>("checklist")
            .unwrap()
            .unwrap();
        assert_eq!(criteria[0].get::<bool>("done").unwrap(), Some(false));
        assert_eq!(
            criteria[0].get::<String>("text").unwrap(),
            Some("è".to_owned())
        );
        assert!(criteria[0].raw("unknown").is_none());
        for key in [
            "archived",
            "blockedBy",
            "ticket",
            "worktree",
            "serviceWorktrees",
            "pullRequest",
            "automation",
            "unknown",
        ] {
            assert!(
                copy.fields.raw(key).is_none(),
                "Unexpected copied field: {key}"
            );
        }
        assert!(card.fields.flag("archived").unwrap());
    }
}
