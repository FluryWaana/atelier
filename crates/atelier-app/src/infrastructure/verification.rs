use super::store::{Documents, Store};
use crate::Result;
use atelier_core::{contracts::Card, document::Document};

pub fn project(store: &Store<'_>, card: &Card) -> Result<Document> {
    let repo = card.fields.get::<String>("repo")?.unwrap_or_default();
    store
        .documents::<Document>(Documents::Projects)?
        .into_iter()
        .find(|project| {
            project
                .get::<String>("repo")
                .ok()
                .flatten()
                .is_some_and(|source| {
                    !repo.is_empty()
                        && atelier_core::domain::projects::same_repository(
                            &source,
                            &repo,
                            cfg!(windows),
                        )
                })
        })
        .ok_or_else(|| "Choisis un projet pour configurer les vérifications de cette tâche.".into())
}
pub fn require_command(store: &Store<'_>, card: &Card, command: &str) -> Result<()> {
    if command.trim().is_empty()
        || command.len() > 8000
        || project(store, card)?
            .get::<String>("testCommand")?
            .as_deref()
            != Some(command)
    {
        return Err(
            "La commande du projet a changé ou n’est pas configurée. Recharge avant de vérifier."
                .into(),
        );
    }
    Ok(())
}
pub fn finish(store: &Store<'_>, id: &str, fingerprint: Option<&str>) -> Result<()> {
    store.transaction(|store| {
        let mut run = store
            .document::<Document>(Documents::Runs, id)?
            .ok_or("Exécution introuvable.")?;
        let card = store.document::<Card>(
            Documents::Cards,
            &run.get::<String>("cardId")?.ok_or("Carte absente.")?,
        )?;
        let state = if !run.string_equals("status", "completed")?
            || run.get::<i32>("exitCode")? != Some(0)
        {
            "failed"
        } else if let Some(card) = card {
            if fingerprint.is_some()
                && run
                    .get::<atelier_core::contracts::Worktree>("verificationWorktree")?
                    .is_some()
                && run.get::<String>("sourceFingerprint")?.as_deref() == fingerprint
                && run.get::<serde_json::Value>("requirements")?.as_ref()
                    == Some(&atelier_core::application::preparation::requirements(
                        &card,
                    )?)
                && card
                    .fields
                    .get::<atelier_core::contracts::Worktree>("worktree")?
                    == run.get::<atelier_core::contracts::Worktree>("verificationWorktree")?
                && require_command(
                    store,
                    &card,
                    &run.get::<String>("verificationCommand")?
                        .unwrap_or_default(),
                )
                .is_ok()
            {
                "passed"
            } else {
                "stale"
            }
        } else {
            "stale"
        };
        run.set("verificationState", &state)?;
        store.put(Documents::Runs, &run)
    })
}
