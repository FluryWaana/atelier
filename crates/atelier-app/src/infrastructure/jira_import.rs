use super::store::{Documents, Store};
use crate::Result;
use atelier_core::{
    application::{jira::JiraFailure, task_creation},
    contracts::{
        Card, JiraBatchImport, JiraConfiguration, JiraImport, JiraImported, JiraIssueDetail,
    },
    document::Document,
};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
struct Reservation {
    id: String,
    inputs: Vec<JiraImport>,
    base: String,
    project: Document,
}
fn conflict() -> crate::Error {
    JiraFailure::OperationConflict(
        "message.the_data_for_this_import_has_changed_reload_the_settings_and_sel",
    )
    .into()
}

fn target(
    store: &Store<'_>,
    input: &JiraImport,
    project: &Document,
    base: &str,
) -> Result<Option<Card>> {
    let Some(expected) = &input.attach_to else {
        return Ok(None);
    };
    let card = store
        .document::<Card>(Documents::Cards, &expected.id)?
        .ok_or_else(conflict)?;
    let already_linked = if let Some(ticket) = card.fields.get::<Document>("ticket")? {
        ticket.string_equals("connector", "jira")?
            && ticket.string_equals("id", &input.issue_id)?
            && ticket
                .get::<String>("instance")?
                .or(ticket.get::<String>("base")?)
                .as_deref()
                == Some(base)
    } else {
        false
    };
    if (!already_linked
        && serde_json::to_value(atelier_core::application::jira::card_snapshot(&card))?
            != serde_json::to_value(atelier_core::application::jira::card_snapshot(expected))?)
        || card.fields.truthy("archived")?
    {
        return Err(conflict());
    }
    if atelier_core::application::jira::unresolved(&card.fields)? {
        return Err("message.check_the_uncertain_jira_submission_before_linking_this_issue".into());
    }
    let repo = card.fields.get::<String>("repo")?.unwrap_or_default();
    let project_repo = project.get::<String>("repo")?.unwrap_or_default();
    if repo.is_empty()
        || !atelier_core::domain::projects::same_repository(&repo, &project_repo, cfg!(windows))
    {
        return Err("message.the_card_does_not_belong_to_the_selected_project".into());
    }
    for run in store.documents::<Document>(Documents::Runs)? {
        if run.string_equals("cardId", &card.id)? && super::runs::active(&run)? {
            return Err("message.stop_the_run_before_linking_this_issue".into());
        }
    }
    Ok(Some(card))
}

pub fn reserve(store: &Store<'_>, input: JiraImport, id: String) -> Result<()> {
    reserve_inputs(store, vec![input], id)
}

pub fn reserve_batch(store: &Store<'_>, input: JiraBatchImport, id: String) -> Result<()> {
    if !input.confirmed || input.issue_ids.is_empty() || input.issue_ids.len() > 20 {
        return Err("message.confirm_a_batch_of_1_to_20_jira_issues".into());
    }
    let unique: std::collections::BTreeSet<_> = input.issue_ids.iter().collect();
    if unique.len() != input.issue_ids.len() {
        return Err("message.duplicate_jira_selection".into());
    }
    let inputs = input
        .issue_ids
        .into_iter()
        .map(|issue_id| JiraImport {
            revision: input.revision,
            issue_id,
            project_id: input.project_id.clone(),
            attach_to: None,
        })
        .collect();
    reserve_inputs(store, inputs, id)
}

fn reserve_inputs(store: &Store<'_>, inputs: Vec<JiraImport>, id: String) -> Result<()> {
    store.transaction(|store| {
        let mut settings = store.settings::<Document>()?;
        if settings.raw("jiraImportReservation").is_some() {
            return Err(conflict());
        }
        let connection = settings
            .get::<JiraConfiguration>("jiraConnection")?
            .ok_or_else(conflict)?;
        let first = inputs.first().ok_or_else(conflict)?;
        let project = store
            .document::<Document>(Documents::Projects, &first.project_id)?
            .ok_or("message.project_not_found")?;
        for input in &inputs {
            if input.issue_id.is_empty()
                || input.issue_id.len() > 32
                || !input.issue_id.bytes().all(|byte| byte.is_ascii_digit())
                || input.project_id.is_empty()
                || input.project_id.len() > 100
            {
                return Err("message.invalid_jira_issue_or_project".into());
            }
            if connection.revision != input.revision || input.project_id != first.project_id {
                return Err(conflict());
            }
            target(store, input, &project, &connection.url)?;
        }
        settings.set(
            "jiraImportReservation",
            &Reservation {
                id,
                inputs,
                base: connection.url,
                project,
            },
        )?;
        store.save_settings(&settings)
    })
}

pub fn release(store: &Store<'_>, id: &str) -> Result<()> {
    store.transaction(|store| {
        let mut settings = store.settings::<Document>()?;
        if settings
            .get::<Reservation>("jiraImportReservation")?
            .is_some_and(|reservation| reservation.id == id)
        {
            settings.remove("jiraImportReservation");
            store.save_settings(&settings)?;
        }
        Ok(())
    })
}

/// Reads have no remote effect; an interrupted reservation can be discarded before serving requests.
pub fn recover(store: &Store<'_>) -> Result<()> {
    let mut settings = store.settings::<Document>()?;
    if settings.raw("jiraImportReservation").is_some() {
        settings.remove("jiraImportReservation");
        store.save_settings(&settings)?;
    }
    Ok(())
}

pub fn finish(store: &Store<'_>, id: &str, detail: JiraIssueDetail) -> Result<JiraImported> {
    finish_batch(store, id, vec![detail])?
        .pop()
        .ok_or_else(conflict)
}

pub fn finish_batch(
    store: &Store<'_>,
    id: &str,
    details: Vec<JiraIssueDetail>,
) -> Result<Vec<JiraImported>> {
    store.transaction(|store| {
        let mut settings = store.settings::<Document>()?;
        let reservation = settings
            .get::<Reservation>("jiraImportReservation")?
            .filter(|reservation| reservation.id == id)
            .ok_or_else(conflict)?;
        if details.len() != reservation.inputs.len() {
            return Err(conflict());
        }
        let results = reservation
            .inputs
            .iter()
            .zip(details)
            .map(|(input, detail)| finish_one(store, &reservation, input, detail))
            .collect::<Result<Vec<_>>>()?;
        settings.remove("jiraImportReservation");
        store.save_settings(&settings)?;
        Ok(results)
    })
}

fn finish_one(
    store: &Store<'_>,
    reservation: &Reservation,
    input: &JiraImport,
    detail: JiraIssueDetail,
) -> Result<JiraImported> {
    if !atelier_core::application::jira::issue_identity(&detail.issue.id, &detail.issue.key) {
        return Err("message.invalid_jira_issue".into());
    }
    let settings = store.settings::<Document>()?;
    let connection = settings
        .get::<JiraConfiguration>("jiraConnection")?
        .ok_or_else(conflict)?;
    if connection.revision != input.revision
        || detail.connection_revision != connection.revision
        || detail.issue.id != input.issue_id
        || connection.url != reservation.base
    {
        return Err(conflict());
    }
    if detail.issue.url
        != format!(
            "{}/browse/{}",
            reservation.base.trim_end_matches('/'),
            detail.issue.key
        )
    {
        return Err("message.inconsistent_jira_issue_address".into());
    }
    let project = store
        .document::<Document>(Documents::Projects, &input.project_id)?
        .ok_or_else(conflict)?;
    if serde_json::to_value(&project)? != serde_json::to_value(&reservation.project)? {
        return Err(conflict());
    }
    let target = target(store, input, &project, &reservation.base)?;
    let mut existing = None;
    for card in store.documents::<Card>(Documents::Cards)? {
        if let Some(ticket) = card.fields.get::<Document>("ticket")? {
            let base = ticket
                .get::<String>("instance")?
                .or(ticket.get::<String>("base")?);
            if ticket.string_equals("connector", "jira")?
                && base.as_deref() == Some(reservation.base.as_str())
                && ticket.string_equals("id", &detail.issue.id)?
            {
                if existing.is_some() {
                    return Err("message.multiple_cards_are_linked_to_the_same_jira_issue".into());
                }
                existing = Some(card);
            }
        }
    }
    let ticket = serde_json::json!({
        "connector":"jira", "instance":reservation.base,
        "id":detail.issue.id, "key":detail.issue.key,
        "url":detail.issue.url, "status":detail.issue.status
    });
    let result = if let Some(mut card) = target {
        if existing
            .as_ref()
            .is_some_and(|existing: &Card| existing.id != card.id)
        {
            return Err("message.this_issue_is_already_linked_to_another_card".into());
        }
        if card.fields.truthy("ticket")? {
            if existing
                .as_ref()
                .is_none_or(|existing| existing.id != card.id)
            {
                return Err("message.this_card_is_already_linked_to_another_issue".into());
            }
            JiraImported {
                card,
                created: false,
                attached: false,
            }
        } else {
            card.fields.set("ticket", &ticket)?;
            card.fields.set(
                "updatedAt",
                &task_creation::CreationContext::timestamp(store)?,
            )?;
            store.put(Documents::Cards, &card)?;
            JiraImported {
                card,
                created: false,
                attached: true,
            }
        }
    } else if let Some(card) = existing {
        JiraImported {
            card,
            created: false,
            attached: false,
        }
    } else {
        let mut body = Document::default();
        body.set("title", &detail.issue.title)?;
        body.set("description", &detail.description)?;
        body.set(
            "repo",
            &project
                .get::<String>("repo")?
                .ok_or("message.missing_project_repository")?,
        )?;
        let mut card = task_creation::create(store, &body, false)?;
        if let Some(status) = atelier_core::application::kanban::for_project(&project)?
            .mapped_status(&connection, &detail.issue.status_id)
        {
            card.fields.set("status", &status)?;
        }
        card.fields.set("ticket", &ticket)?;
        store.put(Documents::Cards, &card)?;
        JiraImported {
            card,
            created: true,
            attached: false,
        }
    };
    Ok(result)
}
