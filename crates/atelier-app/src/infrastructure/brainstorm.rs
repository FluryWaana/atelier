use super::store::{Documents, Store};
use crate::Result;
pub use atelier_core::application::brainstorm::Convert;
use atelier_core::{
    application::{
        brainstorm::{self, Answer, Exchange},
        task_creation,
    },
    contracts::Card,
    document::Document,
};
use std::collections::{BTreeMap, BTreeSet};

pub fn conversation(
    store: &Store<'_>,
    card: &Card,
    expected: &Card,
    reply: &str,
) -> Result<String> {
    if !card.fields.truthy("brainstormSession")?
        || serde_json::to_value(card)? != serde_json::to_value(expected)?
    {
        return Err("message.the_session_changed_reload_it_before_continuing".into());
    }
    let mut runs = Vec::new();
    for run in store.documents::<Document>(Documents::Runs)? {
        if run.string_equals("cardId", &card.id)? && run.string_equals("purpose", "brainstorm")? {
            let started = run
                .get::<String>("startedAt")?
                .ok_or("message.invalid_brainstorm_history")?;
            let timestamp = time::OffsetDateTime::parse(
                &started,
                &time::format_description::well_known::Rfc3339,
            )
            .map_err(|_| "message.invalid_brainstorm_history")?
            .unix_timestamp_nanos();
            let id = run
                .get::<String>("id")?
                .ok_or("message.invalid_brainstorm_history")?;
            runs.push((timestamp, id, run));
        }
    }
    runs.sort_by(|left, right| (&left.0, &left.1).cmp(&(&right.0, &right.1)));
    let mut history = Vec::new();
    for (_, _, run) in runs {
        if run.string_equals("status", "completed")?
            && let Some(assistant) = run.get::<Answer>("brainstormAnswer")?
        {
            history.push(Exchange {
                user: run
                    .get::<String>("userMessage")?
                    .ok_or("message.invalid_brainstorm_history")?,
                assistant,
            });
        }
    }
    let mut description = card
        .fields
        .get::<String>("description")?
        .unwrap_or_default();
    let instructions = card
        .fields
        .get::<String>("instructions")?
        .unwrap_or_default();
    if !instructions.is_empty() {
        description.push_str("\n\nInstructions enregistrées :\n");
        description.push_str(&instructions);
    }
    let criteria = card
        .fields
        .get::<Vec<atelier_core::domain::checklist::Criterion>>("checklist")?
        .unwrap_or_default();
    description.push_str(&atelier_core::domain::checklist::checklist_text(&criteria).utf8()?);
    Ok(brainstorm::prompt(
        &card.title.utf8()?,
        &description,
        &history,
        reply,
    )?)
}

pub fn convert(store: &Store<'_>, input: Convert) -> Result<Vec<Card>> {
    store.transaction(|store| {
        if !input.confirmed
            || input.indices.is_empty()
            || input.indices.len() > 8
            || input.indices.iter().copied().collect::<BTreeSet<_>>().len() != input.indices.len()
        {
            return Err("message.confirm_the_proposals_to_convert_into_tasks".into());
        }
        let card = store
            .document::<Card>(Documents::Cards, &input.expected.id)?
            .ok_or("message.session_not_found")?;
        if !card.fields.truthy("brainstormSession")?
            || card.fields.truthy("archived")?
            || atelier_core::application::jira::unresolved(&card.fields)?
            || serde_json::to_value(&card)? != serde_json::to_value(&input.expected)?
        {
            return Err("message.the_session_changed_reload_it_before_continuing".into());
        }
        let project_id = input
            .project
            .get::<String>("id")?
            .ok_or("message.project_not_found")?;
        let project = store
            .document::<Document>(Documents::Projects, &project_id)?
            .ok_or("message.project_not_found")?;
        if serde_json::to_value(&project)? != serde_json::to_value(&input.project)? {
            return Err("message.the_project_changed_select_it_again".into());
        }
        for run in store.documents::<Document>(Documents::Runs)? {
            if run.string_equals("cardId", &card.id)? && super::runs::active(&run)? {
                return Err(
                    "message.wait_for_the_brainstorm_to_finish_before_converting_proposals".into(),
                );
            }
        }
        let mut run = store
            .document::<Document>(Documents::Runs, &input.run_id)?
            .ok_or("message.run_not_found")?;
        if !run.string_equals("cardId", &card.id)?
            || !run.string_equals("purpose", "brainstorm")?
            || !run.string_equals("status", "completed")?
        {
            return Err("message.brainstorm_proposals_unavailable".into());
        }
        let answer = run
            .get::<Answer>("brainstormAnswer")?
            .ok_or("message.brainstorm_proposals_unavailable")?;
        let mut converted = run
            .get::<BTreeMap<usize, String>>("convertedProposals")?
            .unwrap_or_default();
        let mut created = Vec::new();
        for index in input.indices {
            let proposal = answer
                .tickets
                .get(index)
                .ok_or("message.brainstorm_proposal_not_found")?;
            if let Some(id) = converted.get(&index) {
                // Never recreate a converted proposal, even if its task has since been removed.
                if let Some(card) = store.document::<Card>(Documents::Cards, id)? {
                    created.push(card);
                }
                continue;
            }
            let mut body = Document::default();
            body.set("title", &proposal.title)?;
            body.set("description", &proposal.description)?;
            body.set(
                "repo",
                &project
                    .get::<String>("repo")?
                    .ok_or("message.missing_project_repository")?,
            )?;
            body.set(
                "checklist",
                &proposal
                    .criteria
                    .iter()
                    .map(|text| serde_json::json!({"text":text,"done":false}))
                    .collect::<Vec<_>>(),
            )?;
            let mut created_card = task_creation::create(store, &body, false)?;
            created_card.fields.set(
                "brainstormSource",
                &serde_json::json!({"sessionId":card.id,"runId":input.run_id,"proposal":index}),
            )?;
            created_card
                .fields
                .set("brainstormQuestions", &proposal.questions)?;
            store.put(Documents::Cards, &created_card)?;
            converted.insert(index, created_card.id.clone());
            created.push(created_card);
        }
        run.set("convertedProposals", &converted)?;
        store.put(Documents::Runs, &run)?;
        Ok(created)
    })
}
