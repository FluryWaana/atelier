use crate::{contracts::Card, document::Document};
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
            Self::Missing => "message.card_not_found",
            Self::Invalid(message) | Self::Conflict(message) => message,
            Self::Document(_) => "message.stored_document_invalid",
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
        .ok_or(CardFailure::Invalid("message.invalid_archive_state"))?;
    let mut active = busy(&card.id);
    if !active {
        for entry in times {
            if entry.string_equals("cardId", &card.id)?
                && (entry.has_text("runningSince")?
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
            "message.wait_for_operations_to_finish_and_stop_the_timer_before_archivin",
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
    let ids: Option<Vec<String>> = serde_json::from_str(value.get())
        .map_err(|_| CardFailure::Invalid("message.invalid_blocked_by"))?;
    Ok(ids.is_some_and(|ids| ids.iter().any(|candidate| candidate == id)))
}

fn has_keys(value: Option<&RawValue>) -> Result<bool, CardFailure> {
    let Some(value) = value else {
        return Ok(false);
    };
    let object: Option<Document> = serde_json::from_str(value.get())?;
    Ok(object.is_some_and(|object| object.keys().next().is_some()))
}
pub fn remove(
    card: &Card,
    cards: &[Document],
    times: &[Document],
    queued: &[String],
    busy: impl Fn(&str) -> bool,
) -> Result<(), CardFailure> {
    if card.fields.has_object("ticketCreation")? {
        return Err(CardFailure::Conflict(
            "message.verify_and_link_the_jira_ticket_from_the_uncertain_delivery_befo",
        ));
    }
    if queued.contains(&card.id) {
        return Err(CardFailure::Conflict(
            "message.remove_this_task_from_the_queue_before_deleting_it",
        ));
    }
    for other in cards {
        if includes(other.raw("blockedBy"), &card.id)? {
            return Err(CardFailure::Conflict(
                "message.remove_blocked_by_links_before_deleting_this_task",
            ));
        }
    }
    if busy(&card.id) {
        return Err(CardFailure::Conflict("message.a_run_is_active"));
    }
    for entry in times {
        if entry.string_equals("cardId", &card.id)? {
            return Err(CardFailure::Conflict(
                "message.this_card_has_time_entries_keep_it_to_preserve_their_history",
            ));
        }
    }
    if card.fields.has_object("worktree")? || has_keys(card.fields.raw("serviceWorktrees"))? {
        return Err(CardFailure::Conflict(
            "message.remove_this_cards_worktrees_first_branches_will_be_kept",
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
        serde_json::from_str(r#"{"id":"card","title":"\u00e9","unknown":{"future":"\u00e8"}}"#)
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
                &[],
                &[],
                |_| false
            )
            .is_err()
        );
        assert!(
            remove(
                &original,
                &[],
                &[document(json!({"cardId":true}))],
                &[],
                |_| false
            )
            .is_err()
        );
        assert!(remove(&original, &[], &[], &[], |_| false).is_ok());
    }
}
