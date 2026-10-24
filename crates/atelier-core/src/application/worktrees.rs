use super::{card_lifecycle::CardFailure, task_creation::Result};
use crate::{
    contracts::{Card, Worktree},
    document::{Document, JsonString},
    domain::projects::same_repository,
};

pub struct CreateOptions {
    pub repo: String,
    pub branch: String,
    pub base: String,
}

pub fn validate_reference(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 180
        || value.starts_with('-')
        || value.contains(['\0', '\n', '\r'])
        || value.contains("@{")
    {
        return Err(CardFailure::Invalid("Nom de branche ou référence de départ invalide.").into());
    }
    Ok(())
}

pub fn create_options(card: &Card, settings: &Document, body: &Document) -> Result<CreateOptions> {
    if card.fields.truthy("archived")? {
        return Err(
            CardFailure::Conflict("Restaure cette tâche avant de reprendre le travail.").into(),
        );
    }
    let repo = card
        .fields
        .get::<JsonString>("repo")?
        .ok_or("Choisis le dépôt Git source de cette carte.")?
        .utf8()?;
    let branch = body
        .get::<String>("branch")?
        .filter(|value| !value.is_empty())
        .or(card
            .fields
            .get::<String>("branch")?
            .filter(|value| !value.is_empty()))
        .unwrap_or_else(|| format!("atelier/{}", card.id));
    let base = body
        .get::<String>("base")?
        .filter(|value| !value.is_empty())
        .or(card
            .fields
            .get::<String>("baseBranch")?
            .filter(|value| !value.is_empty()))
        .or(settings
            .get::<String>("baseBranch")?
            .filter(|value| !value.is_empty()))
        .unwrap_or_else(|| "HEAD".into());
    validate_reference(&branch)?;
    validate_reference(&base)?;
    Ok(CreateOptions { repo, branch, base })
}

pub fn removal(body: &Document) -> Result<bool> {
    if body.get::<bool>("confirmed")? != Some(true) {
        return Err(CardFailure::Invalid(
            "Confirme le retrait du worktree. Sa branche sera conservée.",
        )
        .into());
    }
    Ok(body.get::<bool>("discardChanges")?.unwrap_or(false))
}

pub fn associate(
    mut card: Card,
    expected: Option<&Worktree>,
    next: Option<&Worktree>,
    timestamp: String,
    windows: bool,
) -> Result<Card> {
    if card.fields.get::<Worktree>("worktree")?.as_ref() != expected {
        return Err(CardFailure::Conflict(
            "L’association du worktree a changé. Actualise la tâche.",
        )
        .into());
    }
    if let Some(worktree) = next {
        let repo = card
            .fields
            .get::<JsonString>("repo")?
            .ok_or("Dépôt absent.")?
            .utf8()?;
        if !same_repository(&repo, &worktree.source, windows) {
            return Err(CardFailure::Conflict(
                "Le dépôt de la tâche a changé pendant l’opération.",
            )
            .into());
        }
        card.fields.set("branch", &worktree.branch)?;
        card.fields.set("worktree", worktree)?;
    } else {
        card.fields.remove("worktree");
    }
    card.fields.set("updatedAt", &timestamp)?;
    Ok(card)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::WorktreeState;
    use serde_json::json;

    #[test]
    fn confirmations_are_explicit_and_stale_associations_cannot_overwrite_a_task() {
        for body in [
            json!({}),
            json!({"confirmed":false}),
            json!({"confirmed":"true"}),
            json!({"confirmed":true,"discardChanges":"true"}),
        ] {
            assert!(removal(&serde_json::from_value(body).unwrap()).is_err());
        }
        assert!(!removal(&serde_json::from_value(json!({"confirmed":true})).unwrap()).unwrap());
        let card: Card = serde_json::from_value(
            json!({"id":"task","title":"Synthetic","repo":"/repo","description":"keep"}),
        )
        .unwrap();
        let options = create_options(&card, &Document::default(), &Document::default()).unwrap();
        assert_eq!(
            (options.branch.as_str(), options.base.as_str()),
            ("atelier/task", "HEAD")
        );
        let worktree = Worktree {
            path: "/managed/worktree".into(),
            source: "/repo".into(),
            branch: options.branch,
            base: options.base,
            base_commit: "a".repeat(40),
            state: WorktreeState::Creating,
        };
        let reserved = associate(card.clone(), None, Some(&worktree), "now".into(), false).unwrap();
        assert!(associate(reserved.clone(), None, None, "later".into(), false).is_err());
        let mut relocated = reserved.clone();
        relocated.fields.set("repo", &"/different").unwrap();
        assert!(
            associate(
                relocated,
                Some(&worktree),
                Some(&worktree),
                "later".into(),
                false
            )
            .is_err()
        );
        let released = associate(reserved, Some(&worktree), None, "later".into(), false).unwrap();
        assert!(
            released
                .fields
                .get::<Worktree>("worktree")
                .unwrap()
                .is_none()
        );
        assert!(
            released
                .fields
                .string_equals("description", "keep")
                .unwrap()
        );
        for reference in ["--force", "@{-1}", "bad\nref", ""] {
            assert!(validate_reference(reference).is_err());
        }
    }
}
