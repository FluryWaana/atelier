use super::store::{Documents, Store};
use crate::Result;
use atelier_core::{
    application::jira::{self, JiraFailure},
    contracts::{
        Card, JiraConfiguration, JiraStatus, JiraTransitionIntent, JiraTransitionRequest,
        JiraTransitionState as State, JiraTransitions,
    },
    document::Document,
};
fn conflict() -> crate::Error {
    JiraFailure::OperationConflict(
        "La carte ou la connexion Jira a changé. Recharge avant de réessayer.",
    )
    .into()
}

pub fn snapshot(store: &Store<'_>, id: &str, revision: u64) -> Result<(Card, String)> {
    let card = store
        .document::<Card>(Documents::Cards, id)?
        .ok_or("Carte introuvable.")?;
    let configuration = store
        .settings::<Document>()?
        .get::<JiraConfiguration>("jiraConnection")?
        .ok_or_else(conflict)?;
    if configuration.revision != revision {
        return Err(conflict());
    }
    let issue = jira::linked_issue(&card, &configuration.url)?;
    Ok((card, issue))
}
pub fn revalidate(store: &Store<'_>, expected: &Card, revision: u64) -> Result<()> {
    let (card, _) = snapshot(store, &expected.id, revision)?;
    if serde_json::to_value(jira::card_snapshot(&card))?
        != serde_json::to_value(jira::card_snapshot(expected))?
    {
        return Err(conflict());
    }
    Ok(())
}
pub fn reserve(
    store: &Store<'_>,
    input: &JiraTransitionRequest,
    operation: &str,
    now: &str,
) -> Result<String> {
    store.transaction(|store| {
        for status in [&input.from, &input.transition.target] {
            if status.id.is_empty()
                || status.id.len() > 64
                || !status.id.bytes().all(|byte| byte.is_ascii_digit())
                || status.name.is_empty()
                || status.name.len() > 400
                || status.name.chars().any(char::is_control)
            {
                return Err("Statut Jira invalide.".into());
            }
        }
        if input.transition.id.is_empty()
            || input.transition.id.len() > 64
            || !input
                .transition
                .id
                .bytes()
                .all(|byte| byte.is_ascii_digit())
            || input.transition.name.is_empty()
            || input.transition.name.len() > 400
            || input.transition.name.chars().any(char::is_control)
            || input.transition.requires_fields
        {
            return Err("Transition Jira invalide ou exigeant des champs supplémentaires.".into());
        }
        revalidate(store, &input.expected, input.revision)?;
        let (mut card, issue) = snapshot(store, &input.expected.id, input.revision)?;
        if jira::unresolved(&card.fields)? || card.fields.truthy("archived")? {
            return Err("Termine ou vérifie l’opération Jira avant de continuer.".into());
        }
        for run in store.documents::<Document>(Documents::Runs)? {
            if run.string_equals("cardId", &card.id)? && super::runs::active(&run)? {
                return Err("Arrête l’exécution avant de modifier le ticket Jira.".into());
            }
        }
        let base = store
            .settings::<Document>()?
            .get::<JiraConfiguration>("jiraConnection")?
            .ok_or_else(conflict)?
            .url;
        let intent = JiraTransitionIntent {
            id: operation.into(),
            state: State::Preparing,
            base,
            issue_id: issue.clone(),
            revision: input.revision,
            from: input.from.clone(),
            transition: input.transition.clone(),
            started_at: now.into(),
            observed: None,
        };
        card.fields.set("jiraTransition", &intent)?;
        store.put(Documents::Cards, &card)?;
        Ok(issue)
    })
}
pub fn sending(
    store: &Store<'_>,
    card_id: &str,
    operation: &str,
    choices: &JiraTransitions,
) -> Result<()> {
    store.transaction(|store| {
        let mut card = store
            .document::<Card>(Documents::Cards, card_id)?
            .ok_or_else(conflict)?;
        let mut intent = card
            .fields
            .get::<JiraTransitionIntent>("jiraTransition")?
            .filter(|intent| intent.id == operation && intent.state == State::Preparing)
            .ok_or_else(conflict)?;
        if choices.connection_revision != intent.revision
            || choices.current != intent.from
            || !choices
                .transitions
                .iter()
                .any(|transition| transition == &intent.transition && !transition.requires_fields)
        {
            return Err(
                "La transition Jira n’est plus disponible ou exige des champs supplémentaires."
                    .into(),
            );
        }
        if snapshot(store, card_id, intent.revision)?.1 != intent.issue_id {
            return Err(conflict());
        }
        intent.state = State::Sending;
        card.fields.set("jiraTransition", &intent)?;
        store.put(Documents::Cards, &card)
    })
}
pub fn complete(
    store: &Store<'_>,
    card_id: &str,
    operation: &str,
    state: State,
    observed: Option<JiraStatus>,
) -> Result<Card> {
    store.transaction(|store| {
        let mut card = store
            .document::<Card>(Documents::Cards, card_id)?
            .ok_or_else(conflict)?;
        let mut intent = card
            .fields
            .get::<JiraTransitionIntent>("jiraTransition")?
            .filter(|intent| intent.id == operation)
            .ok_or_else(conflict)?;
        let valid = match intent.state {
            State::Preparing => state == State::Failed,
            State::Sending => matches!(state, State::Uncertain | State::Confirmed),
            State::Uncertain => matches!(
                state,
                State::Uncertain | State::Confirmed | State::Dismissed
            ),
            _ => false,
        };
        if (intent.state == State::Uncertain
            && state == State::Confirmed
            && intent.from.id == intent.transition.target.id)
            || (state == State::Dismissed && observed.is_none())
            || !valid
            || (state == State::Confirmed
                && observed
                    .as_ref()
                    .is_none_or(|status| status.id != intent.transition.target.id))
        {
            return Err(conflict());
        }
        if jira::linked_issue(&card, &intent.base)? != intent.issue_id {
            return Err(conflict());
        }
        if let Some(status) = &observed {
            let mut ticket = card
                .fields
                .get::<Document>("ticket")?
                .ok_or_else(conflict)?;
            ticket.set("status", &status.name)?;
            ticket.set("statusId", &status.id)?;
            card.fields.set("ticket", &ticket)?;
        }
        intent.state = state;
        intent.observed = observed;
        card.fields.set("jiraTransition", &intent)?;
        store.put(Documents::Cards, &card)?;
        Ok(card)
    })
}
pub fn recover(store: &Store<'_>) -> Result<()> {
    store.transaction(|store| {
        for mut card in store.documents::<Document>(Documents::Cards)? {
            if let Some(mut intent) = card.get::<JiraTransitionIntent>("jiraTransition")? {
                let state = match intent.state {
                    State::Preparing => State::Failed,
                    State::Sending => State::Uncertain,
                    _ => continue,
                };
                intent.state = state;
                card.set("jiraTransition", &intent)?;
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
        if jira::unresolved(&card)?
            && (path.starts_with("/api/backups")
                || path.starts_with("/api/projects")
                || (affected.is_some()
                    && card.get::<String>("id")?.as_deref() == affected
                    && !path.ends_with("/jira/reconcile")))
        {
            return Ok(true);
        }
    }
    Ok(false)
}
