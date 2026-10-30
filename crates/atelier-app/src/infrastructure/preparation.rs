use super::{
    runs,
    store::{Documents, Store},
};
use crate::Result;
use atelier_core::{
    application::{
        preparation::{self, Apply, Preparation},
        task_creation::CreationContext,
        task_editing,
    },
    contracts::Card,
    document::Document,
};

pub fn apply(store: &Store<'_>, input: Apply) -> Result<Card> {
    store.transaction(|store| {
        if !input.confirmed {
            return Err("message.confirm_replacing_the_criteria_with_this_preparation".into());
        }
        let mut card = store
            .document::<Card>(Documents::Cards, &input.expected.id)?
            .ok_or("message.card_not_found")?;
        if card.fields.flag("archived")?
            || card.fields.flag("brainstormSession")?
            || atelier_core::application::jira::unresolved(&card.fields)?
            || serde_json::to_value(atelier_core::application::jira::card_snapshot(&card))?
                != serde_json::to_value(atelier_core::application::jira::card_snapshot(
                    &input.expected,
                ))?
        {
            return Err("message.the_task_changed_reload_before_applying_the_preparation".into());
        }
        for run in store.documents::<Document>(Documents::Runs)? {
            if run.string_equals("cardId", &card.id)? && runs::active(&run)? {
                return Err(
                    "message.wait_for_runs_to_finish_before_applying_the_preparation".into(),
                );
            }
        }
        let mut run = store
            .document::<Document>(Documents::Runs, &input.run_id)?
            .ok_or("message.run_not_found")?;
        if !run.string_equals("purpose", "preparation")?
            || !run.string_equals("cardId", &card.id)?
            || !run.string_equals("status", "completed")?
        {
            return Err("message.preparation_unavailable".into());
        }
        if run.get::<bool>("preparationApplied")?.unwrap_or(false) {
            return Err(
                "message.this_preparation_was_already_applied_human_validations_are_prese".into(),
            );
        }
        if run.get::<serde_json::Value>("requirements")? != Some(preparation::requirements(&card)?)
        {
            return Err(
                "message.requirements_changed_since_this_preparation_run_the_analysis_aga".into(),
            );
        }
        let result = run
            .get::<Preparation>("preparation")?
            .ok_or("message.preparation_unavailable")?;
        card.fields
            .set("checklist", &preparation::criteria(&result)?)?;
        card.fields.set("preparationQuestions", &result.questions)?;
        card.fields
            .set("updatedAt", &CreationContext::timestamp(store)?)?;
        let invalidation = task_editing::invalidate_reviews(
            &card.id,
            &mut card.fields,
            store.documents(Documents::Runs)?,
            task_editing::REVIEW_CHANGE_REASON,
        )?;
        for changed in invalidation.runs {
            store.put(Documents::Runs, &changed)?;
        }
        run.set("preparationApplied", &true)?;
        store.put(Documents::Runs, &run)?;
        store.put(Documents::Cards, &card)?;
        Ok(card)
    })
}
