use crate::{
    contracts::Card,
    document::{Document, JsonString},
    domain::text_units,
};
use serde_json::json;

pub mod time_tracking;

pub fn create_card(body: &Document, id: String, timestamp: String) -> Result<Card, &'static str> {
    let title = text_units(
        &body
            .get::<JsonString>("title")
            .map_err(|_| "Texte invalide ou trop long.")?
            .ok_or("Texte invalide ou trop long.")?,
        300,
    )?;
    if title.0.is_empty() {
        return Err("Un titre est nécessaire.");
    }
    // shortcut: M01 only accepts a title; extend the complete task contract in M03.
    if body.keys().any(|key| key != &JsonString::from("title")) {
        return Err("Ces champs ne sont pas encore disponibles dans le prototype Rust.");
    }
    let fields = json!({"description":"", "instructions":"", "repo":"", "agent":"codex", "model":"", "effort":"", "mode":"edit", "checklist":[], "status":"todo", "createdAt":timestamp, "updatedAt":timestamp});
    Ok(Card {
        id,
        title,
        fields: serde_json::from_str(&fields.to_string()).map_err(|_| "Objet JSON requis.")?,
    })
}

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
    let mut criteria = Vec::new();
    if card.fields.truthy("checklist")? {
        for original in card
            .fields
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
    fields.set("checklist", &criteria)?;
    fields.set("status", &"todo")?;
    fields.set("createdAt", &timestamp)?;
    fields.set("updatedAt", &timestamp)?;
    Ok(Card { id, title, fields })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn body(value: serde_json::Value) -> Document {
        serde_json::from_str(&value.to_string()).unwrap()
    }

    #[test]
    fn validates_before_constructing_a_card() {
        assert_eq!(
            create_card(&body(json!({"title":" "})), "a".into(), "now".into()).unwrap_err(),
            "Un titre est nécessaire."
        );
        assert!(create_card(&body(json!({"title":3})), "a".into(), "now".into()).is_err());
        assert!(
            create_card(
                &body(json!({"title":"x", "repo":"ignored"})),
                "a".into(),
                "now".into()
            )
            .is_err()
        );
        let card = create_card(&body(json!({"title":" é "})), "a".into(), "now".into()).unwrap();
        assert_eq!(card.title, JsonString::from("é"));
        assert!(card.fields.string_equals("status", "todo").unwrap());
    }

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
