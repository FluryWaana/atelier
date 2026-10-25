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
            return Err("Confirme le remplacement des critères par cette préparation.".into());
        }
        let mut card = store
            .document::<Card>(Documents::Cards, &input.expected.id)?
            .ok_or("Carte introuvable.")?;
        if card.fields.truthy("archived")?
            || card.fields.truthy("brainstormSession")?
            || atelier_core::application::jira::unresolved(&card.fields)?
            || serde_json::to_value(atelier_core::application::jira::card_snapshot(&card))?
                != serde_json::to_value(atelier_core::application::jira::card_snapshot(
                    &input.expected,
                ))?
        {
            return Err("La tâche a changé. Recharge avant d’appliquer la préparation.".into());
        }
        for run in store.documents::<Document>(Documents::Runs)? {
            if run.string_equals("cardId", &card.id)? && runs::active(&run)? {
                return Err(
                    "Attends la fin des exécutions avant d’appliquer la préparation.".into(),
                );
            }
        }
        let mut run = store
            .document::<Document>(Documents::Runs, &input.run_id)?
            .ok_or("Exécution introuvable.")?;
        if !run.string_equals("purpose", "preparation")?
            || !run.string_equals("cardId", &card.id)?
            || !run.string_equals("status", "completed")?
        {
            return Err("Préparation indisponible.".into());
        }
        if run.get::<bool>("preparationApplied")?.unwrap_or(false) {
            return Err(
                "Cette préparation a déjà été appliquée. Les validations humaines sont conservées."
                    .into(),
            );
        }
        if run.get::<serde_json::Value>("requirements")? != Some(preparation::requirements(&card)?)
        {
            return Err("Le besoin a changé depuis cette préparation. Relance l’analyse.".into());
        }
        let result = run
            .get::<Preparation>("preparation")?
            .ok_or("Préparation indisponible.")?;
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
