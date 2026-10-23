use crate::{contracts::Card, domain::text};
use serde_json::{Value, json};

pub fn create_card(body: &Value, id: String, timestamp: String) -> Result<Card, &'static str> {
    let title = text(
        body.get("title")
            .and_then(Value::as_str)
            .ok_or("Texte invalide ou trop long.")?,
        300,
    )?;
    if title.is_empty() {
        return Err("Un titre est nécessaire.");
    }
    // shortcut: M01 only accepts a title; extend the complete task contract in M03.
    if body
        .as_object()
        .is_none_or(|object| object.keys().any(|key| key != "title"))
    {
        return Err("Ces champs ne sont pas encore disponibles dans le prototype Rust.");
    }
    let fields = json!({"description":"", "instructions":"", "repo":"", "agent":"codex", "model":"", "effort":"", "mode":"edit", "checklist":[], "status":"todo", "createdAt":timestamp, "updatedAt":timestamp});
    Ok(Card {
        id,
        title,
        fields: fields.as_object().cloned().ok_or("Objet JSON requis.")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_before_constructing_a_card() {
        assert_eq!(
            create_card(&json!({"title":" "}), "a".into(), "now".into()),
            Err("Un titre est nécessaire.")
        );
        assert!(create_card(&json!({"title":3}), "a".into(), "now".into()).is_err());
        assert!(
            create_card(
                &json!({"title":"x", "repo":"ignored"}),
                "a".into(),
                "now".into()
            )
            .is_err()
        );
        let card = create_card(&json!({"title":" é "}), "a".into(), "now".into()).unwrap();
        assert_eq!(card.title, "é");
        assert_eq!(card.fields["status"], "todo");
    }

    #[test]
    fn contracts_preserve_additional_fields_and_absence() {
        let value =
            json!({"id":"a", "title":"titre", "nullable":null, "extra":{"values":[false, 0, "é"]}});
        let card: Card = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(card).unwrap(), value);
    }
}
