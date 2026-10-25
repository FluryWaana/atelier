use crate::{
    contracts::Card,
    document::{Document, JsonString},
};

pub mod agent_protocol;
pub mod agent_providers;
pub mod backups;
pub mod card_lifecycle;
pub mod jira;
pub mod projects;
pub mod services;
pub mod settings;
pub mod task_creation;
pub mod task_editing;
pub mod task_templates;
pub mod time_reports;
pub mod time_tracking;
pub mod worktrees;

pub fn duplicate_card(card: &Card, id: String, timestamp: String) -> serde_json::Result<Card> {
    let mut title = JsonString::from("Copie — ");
    title.0.extend_from_slice(&card.title.0);
    title.0.truncate(300);
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
        if card.fields.truthy(key)? {
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
    if source.truthy("checklist")? {
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
    fn contracts_preserve_additional_fields_and_absence() {
        let value =
            json!({"id":"a", "title":"titre", "nullable":null, "extra":{"values":[false, 0, "é"]}});
        let card: Card = serde_json::from_str(&value.to_string()).unwrap();
        assert_eq!(serde_json::to_value(card).unwrap(), value);
    }

    #[test]
    fn duplication_copies_only_editable_content_and_resets_human_criteria() {
        let card: Card = serde_json::from_str(r#"{"id":"old","title":"Source","description":"\ud800","agent":"work","model":"model","effort":"high","mode":"analyze","instructions":"Keep","repo":"/repo","baseBranch":"main","checklist":[{"text":"\udc00","done":true,"unknown":1}],"status":"done","archived":true,"blockedBy":["other"],"ticket":{"id":"1"},"worktree":{"path":"/work"},"serviceWorktrees":{"front":{}},"pullRequest":{"id":1},"automation":{"state":"done"},"unknown":"discard"}"#).unwrap();
        let copy = duplicate_card(&card, "new".into(), "now".into()).unwrap();
        assert_eq!(copy.title, JsonString::from("Copie — Source"));
        assert!(copy.fields.string_equals("status", "todo").unwrap());
        assert_eq!(
            copy.fields.get::<JsonString>("description").unwrap(),
            Some(JsonString(vec![0xd800]))
        );
        let criteria = copy
            .fields
            .get::<Vec<Document>>("checklist")
            .unwrap()
            .unwrap();
        assert_eq!(criteria[0].get::<bool>("done").unwrap(), Some(false));
        assert_eq!(
            criteria[0].get::<JsonString>("text").unwrap(),
            Some(JsonString(vec![0xdc00]))
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
        assert!(card.fields.truthy("archived").unwrap());
    }
}
