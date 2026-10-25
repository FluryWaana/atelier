use super::{
    forge, forge_association,
    store::{Documents, Store},
};
use crate::Result;
use atelier_core::{
    contracts::{Card, ForgeDraftIntent, ForgeDraftRequest, ForgeWriteState as Phase, PullRequest},
    document::Document,
};

fn conflict() -> crate::Error {
    atelier_core::application::forge::ForgeFailure::OperationConflict(
        "La tâche ou son opération de forge a changé. Recharge avant de réessayer.",
    )
    .into()
}
pub fn reserve(store: &Store<'_>, input: &ForgeDraftRequest, id: &str) -> Result<Card> {
    store.transaction(|store| {
        if !input.confirmed || uuid::Uuid::parse_str(id).is_err() {
            return Err("Confirme le changement d’état brouillon de la PR/MR.".into());
        }
        let mut card = forge_association::snapshot(store, &input.expected)?;
        let expected = card
            .fields
            .get::<PullRequest>("pullRequest")?
            .ok_or("Associe une PR/MR avant de modifier son état brouillon.")?;
        let configuration = forge::configuration(store, expected.kind, input.revision)?;
        if expected.base_url != configuration.base
            || expected.revision != input.revision
            || expected.status != "active"
            || expected.fork
            || expected.draft == input.draft
        {
            return Err("Relis une PR/MR ouverte avant de modifier son état brouillon.".into());
        }
        card.fields.set(
            "forgeDraft",
            &ForgeDraftIntent {
                id: id.into(),
                state: Phase::Preparing,
                expected,
                draft: input.draft,
                started_at: time::OffsetDateTime::now_utc()
                    .format(&time::format_description::well_known::Rfc3339)?,
                observed: None,
            },
        )?;
        store.put(Documents::Cards, &card)?;
        Ok(card)
    })
}
fn active(store: &Store<'_>, card_id: &str, id: &str) -> Result<(Card, ForgeDraftIntent)> {
    let card = store
        .document::<Card>(Documents::Cards, card_id)?
        .ok_or_else(conflict)?;
    let intent = card
        .fields
        .get::<ForgeDraftIntent>("forgeDraft")?
        .filter(|intent| intent.id == id)
        .ok_or_else(conflict)?;
    Ok((card, intent))
}
pub fn sending(store: &Store<'_>, expected: &Card, id: &str) -> Result<()> {
    store.transaction(|store| {
        let (mut card, mut intent) = active(store, &expected.id, id)?;
        forge::require_revision(store, intent.expected.revision)?;
        if serde_json::to_value(atelier_core::application::jira::card_snapshot(&card))? != serde_json::to_value(atelier_core::application::jira::card_snapshot(expected))? || intent.state != Phase::Preparing { return Err(conflict()); }
        intent.state = Phase::Sending;
        card.fields.set("forgeDraft", &intent)?;
        // A lost response may already have changed the remote state.
        forge_association::invalidate(store, &mut card, "L’état distant de la PR/MR est en cours de modification. Revalide la review après relecture.")?;
        store.put(Documents::Cards, &card)
    })
}
pub fn interrupted(store: &Store<'_>, card_id: &str, id: &str) -> Result<Card> {
    store.transaction(|store| {
        let (mut card, mut intent) = active(store, card_id, id)?;
        intent.state = match intent.state {
            Phase::Preparing => Phase::Failed,
            Phase::Sending => Phase::Uncertain,
            _ => return Err(conflict()),
        };
        card.fields.set("forgeDraft", &intent)?;
        store.put(Documents::Cards, &card)?;
        Ok(card)
    })
}
pub fn record(
    store: &Store<'_>,
    card_id: &str,
    id: &str,
    observed: PullRequest,
    reconciled: bool,
) -> Result<Card> {
    store.transaction(|store| {
        let (mut card, mut intent) = active(store, card_id, id)?;
        if !matches!(intent.state, Phase::Sending | Phase::Uncertain) {
            return Err(conflict());
        }
        let expected = &intent.expected;
        let configuration = forge::configuration(store, expected.kind, observed.revision)?;
        if observed.kind != expected.kind
            || observed.base_url != expected.base_url
            || configuration.base != expected.base_url
            || observed.repository != expected.repository
            || observed.number != expected.number
            || observed.fork
            || observed.source_branch != expected.source_branch
            || observed.target_branch != expected.target_branch
            || observed.url
                != forge::browser_link(&configuration, &observed.repository, observed.number)?
        {
            return Err("La PR/MR relue ne correspond pas à l’opération incertaine.".into());
        }
        // A contrary read cannot prove a timed-out request will never be applied later.
        if observed.draft == intent.draft {
            intent.state = if reconciled {
                Phase::Reconciled
            } else {
                Phase::Confirmed
            };
        } else {
            intent.state = Phase::Uncertain;
        }
        intent.observed = Some(observed.clone());
        if matches!(intent.state, Phase::Confirmed | Phase::Reconciled) {
            card.fields.set("forgeDraftEffect", &intent)?;
        }
        card.fields.set("forgeDraft", &intent)?;
        card.fields.set("pullRequest", &observed)?;
        forge_association::invalidate(
            store,
            &mut card,
            "L’état distant de la PR/MR a été relu. Revalide la review sur ses commits actuels.",
        )?;
        store.put(Documents::Cards, &card)?;
        Ok(card)
    })
}
pub fn recover(store: &Store<'_>) -> Result<()> {
    store.transaction(|store| {
        for mut card in store.documents::<Card>(Documents::Cards)? {
            if let Some(mut intent) = card.fields.get::<ForgeDraftIntent>("forgeDraft")? {
                intent.state = match intent.state {
                    Phase::Preparing => Phase::Failed,
                    Phase::Sending => Phase::Uncertain,
                    _ => continue,
                };
                card.fields.set("forgeDraft", &intent)?;
                store.put(Documents::Cards, &card)?;
            }
        }
        Ok(())
    })
}
pub fn blocks(store: &Store<'_>, path: &str) -> Result<bool> {
    let affected = path
        .strip_prefix("/api/cards/")
        .map(|tail| tail.split('/').next().unwrap_or_default());
    for card in store.documents::<Document>(Documents::Cards)? {
        let card_id = card.get::<String>("id")?;
        if atelier_core::application::forge::unresolved(&card)?
            && (path.starts_with("/api/backups")
                || path.starts_with("/api/projects")
                || affected.is_some_and(|id| card_id.as_deref() == Some(id))
                    && !path.ends_with("/forge/draft-reconcile"))
        {
            return Ok(true);
        }
    }
    Ok(false)
}
