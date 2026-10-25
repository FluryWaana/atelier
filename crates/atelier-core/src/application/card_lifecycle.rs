use crate::{
    contracts::Card,
    document::{Document, JsonString},
};
use serde_json::value::RawValue;

#[derive(Debug)]
pub enum CardFailure {
    Missing,
    Invalid(&'static str),
    Conflict(&'static str),
    Document(serde_json::Error),
}

impl std::fmt::Display for CardFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Missing => "Carte introuvable.",
            Self::Invalid(message) | Self::Conflict(message) => message,
            Self::Document(_) => "Document enregistré invalide.",
        })
    }
}
impl std::error::Error for CardFailure {}
impl From<serde_json::Error> for CardFailure {
    fn from(error: serde_json::Error) -> Self {
        Self::Document(error)
    }
}

pub fn archive(
    mut card: Card,
    body: &Document,
    times: &[Document],
    busy: impl Fn(&str) -> bool,
    timestamp: String,
) -> Result<Card, CardFailure> {
    let archived = body
        .get::<bool>("archived")
        .ok()
        .flatten()
        .ok_or(CardFailure::Invalid("État d’archive invalide."))?;
    let mut active = busy(&card.id);
    if !active {
        for entry in times {
            if entry.string_equals("cardId", &card.id)?
                && (entry.truthy("runningSince")?
                    || entry.string_equals("exportState", "sending")?
                    || (archived && entry.string_equals("exportState", "uncertain")?))
            {
                active = true;
                break;
            }
        }
    }
    if active {
        return Err(CardFailure::Conflict(
            "Attends la fin des opérations et arrête le chronomètre avant d’archiver.",
        ));
    }
    card.fields.set("archived", &archived)?;
    card.fields.set("updatedAt", &timestamp)?;
    Ok(card)
}

fn includes(value: Option<&RawValue>, id: &str) -> Result<bool, CardFailure> {
    let Some(value) = value else {
        return Ok(false);
    };
    let id = JsonString::from(id);
    match value.get().as_bytes()[0] {
        b'n' => Ok(false),
        b'[' => {
            for item in serde_json::from_str::<Vec<Box<RawValue>>>(value.get())? {
                if item.get().starts_with('"')
                    && serde_json::from_str::<JsonString>(item.get())? == id
                {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        b'"' => {
            let text: JsonString = serde_json::from_str(value.get())?;
            Ok(id.0.is_empty() || text.0.windows(id.0.len()).any(|window| window == id.0))
        }
        _ => Err(CardFailure::Invalid(
            "c.blockedBy?.includes is not a function",
        )),
    }
}

fn has_keys(value: Option<&RawValue>) -> Result<bool, CardFailure> {
    let Some(value) = value else {
        return Ok(false);
    };
    Ok(match value.get().as_bytes()[0] {
        b'{' => serde_json::from_str::<Document>(value.get())?
            .keys()
            .next()
            .is_some(),
        b'[' => !serde_json::from_str::<Vec<Box<RawValue>>>(value.get())?.is_empty(),
        b'"' => !serde_json::from_str::<JsonString>(value.get())?
            .0
            .is_empty(),
        _ => false,
    })
}

pub fn remove(
    card: &Card,
    cards: &[Document],
    times: &[Document],
    queued: &[String],
    busy: impl Fn(&str) -> bool,
) -> Result<(), CardFailure> {
    if card.fields.truthy("ticketCreation")? {
        return Err(CardFailure::Conflict(
            "Vérifie et rattache le ticket Jira de l’envoi incertain avant de supprimer cette carte.",
        ));
    }
    if queued.contains(&card.id) {
        return Err(CardFailure::Conflict(
            "Retire cette tâche de la file avant de la supprimer.",
        ));
    }
    for other in cards {
        if includes(other.raw("blockedBy"), &card.id)? {
            return Err(CardFailure::Conflict(
                "Retire les liens « Bloquée par » avant de supprimer cette tâche.",
            ));
        }
    }
    if busy(&card.id) {
        return Err(CardFailure::Conflict("Une exécution est active."));
    }
    for entry in times {
        if entry.string_equals("cardId", &card.id)? {
            return Err(CardFailure::Conflict(
                "Cette carte possède des entrées de temps. Conserve-la pour garder leur historique.",
            ));
        }
    }
    if card.fields.truthy("worktree")? || has_keys(card.fields.raw("serviceWorktrees"))? {
        return Err(CardFailure::Conflict(
            "Retire d’abord les worktrees de cette carte. Les branches seront conservées.",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn document(value: serde_json::Value) -> Document {
        serde_json::from_str(&value.to_string()).unwrap()
    }
    fn card() -> Card {
        serde_json::from_str(r#"{"id":"card","title":"\ud800","unknown":{"future":"\udc00"}}"#)
            .unwrap()
    }

    #[test]
    fn archiving_validates_before_activity_and_preserves_unknown_content() {
        let original = card();
        assert!(matches!(
            archive(
                original.clone(),
                &document(json!({"archived":"true"})),
                &[],
                |_| true,
                "now".into()
            ),
            Err(CardFailure::Invalid(_))
        ));
        let body = document(json!({"archived":true}));
        for entry in [
            json!({"cardId":"card", "runningSince":"now"}),
            json!({"cardId":"card", "exportState":"sending"}),
            json!({"cardId":"card", "exportState":"uncertain"}),
        ] {
            assert!(matches!(
                archive(
                    original.clone(),
                    &body,
                    &[document(entry)],
                    |_| false,
                    "now".into()
                ),
                Err(CardFailure::Conflict(_))
            ));
        }
        assert!(matches!(
            archive(original.clone(), &body, &[], |_| true, "now".into()),
            Err(CardFailure::Conflict(_))
        ));
        let archived = archive(
            original.clone(),
            &body,
            &[document(json!({"cardId":"other", "runningSince":"now"}))],
            |_| false,
            "now".into(),
        )
        .unwrap();
        assert_eq!(archived.title, original.title);
        assert_eq!(
            archived.fields.raw("unknown").unwrap().get(),
            original.fields.raw("unknown").unwrap().get()
        );
        assert_eq!(archived.fields.get::<bool>("archived").unwrap(), Some(true));
        let restored = archive(
            archived,
            &document(json!({"archived":false})),
            &[document(
                json!({"cardId":"card", "exportState":"uncertain"}),
            )],
            |_| false,
            "later".into(),
        )
        .unwrap();
        assert_eq!(
            restored.fields.get::<bool>("archived").unwrap(),
            Some(false)
        );
        assert_eq!(
            restored
                .fields
                .get::<String>("updatedAt")
                .unwrap()
                .as_deref(),
            Some("later")
        );
    }

    #[test]
    fn deletion_keeps_history_dependencies_queue_uncertain_tickets_and_worktrees() {
        let original = card();
        assert!(remove(&original, &[], &[], &["card".into()], |_| false).is_err());
        assert!(remove(&original, &[], &[], &[], |_| true).is_err());
        assert!(
            remove(
                &original,
                &[],
                &[document(json!({"cardId":"card", "exportState":"exported"}))],
                &[],
                |_| false
            )
            .is_err()
        );
        for blocked_by in [json!(["card"]), json!("other-card-link")] {
            assert!(
                remove(
                    &original,
                    &[document(json!({"blockedBy":blocked_by}))],
                    &[],
                    &[],
                    |_| false
                )
                .is_err()
            );
        }
        for (key, value) in [
            ("ticketCreation", json!({})),
            ("worktree", json!({})),
            ("serviceWorktrees", json!({"front":{}})),
        ] {
            let mut linked = original.clone();
            linked.fields.set(key, &value).unwrap();
            assert!(remove(&linked, &[], &[], &[], |_| false).is_err());
        }
        assert!(
            remove(
                &original,
                &[document(json!({"blockedBy":[1, "other"]}))],
                &[document(json!({"cardId":true}))],
                &[],
                |_| false
            )
            .is_ok()
        );
        assert!(remove(&original, &[], &[], &[], |_| false).is_ok());
    }
}
