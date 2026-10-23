use crate::{
    contracts::Card,
    document::{Document, JsonString},
    domain::text_units,
};
use serde_json::json;

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
}
