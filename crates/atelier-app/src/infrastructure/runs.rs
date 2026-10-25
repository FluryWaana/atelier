use super::store::{Documents, Store};
use crate::Result;
use atelier_core::{contracts::Card, document::Document};

pub const LOG_LIMIT: usize = 5 * 1024 * 1024;

pub fn active(run: &Document) -> Result<bool> {
    Ok(matches!(
        run.get::<String>("status")?.as_deref(),
        Some("starting" | "running" | "stopping")
    ))
}

/// The caller reserves HTTP mutations before preparing the external launch.
pub fn reserve(store: &Store<'_>, run: &Document) -> Result<()> {
    store.transaction(|store| {
        let id = run
            .get::<String>("id")?
            .filter(|id| !id.is_empty())
            .ok_or("Identifiant d’exécution absent.")?;
        let card_id = run.get::<String>("cardId")?.ok_or("Carte absente.")?;
        let repo = run
            .get::<String>("repo")?
            .filter(|repo| !repo.is_empty())
            .ok_or("Dossier d’exécution absent.")?;
        if !run.string_equals("status", "starting")? {
            return Err("État initial d’exécution invalide.".into());
        }
        let mut card = store
            .document::<Card>(Documents::Cards, &card_id)?
            .ok_or("Carte introuvable.")?;
        if card.fields.truthy("archived")? {
            return Err("Restaure cette tâche avant de lancer un agent.".into());
        }
        if run.string_equals("kind", "pr-review")? {
            let context = run
                .get::<atelier_core::application::reviews::Context>("reviewContext")?
                .ok_or("Contexte de review absent.")?;
            super::reviews::matches(store, &card, &context)?;
        }
        if run.string_equals("purpose", "correction")? {
            if !run.string_equals("mode", "edit")? {
                return Err("Confirme la correction sélectionnée en mode Modification.".into());
            }
            let context = run
                .get::<atelier_core::application::reviews::Context>("reviewContext")?
                .ok_or("Contexte de review absent.")?;
            let review = run
                .get::<String>("correctionOf")?
                .ok_or("Review introuvable.")?;
            let indexes = run
                .get::<Vec<usize>>("correctionIndexes")?
                .ok_or("Sélection de constats invalide.")?;
            if super::reviews::correction(store, &card, &review, &indexes)?.0 != context {
                return Err("Les commits ou le besoin ont changé. Relance la review.".into());
            }
            let invalidation = atelier_core::application::task_editing::invalidate_reviews(
                &card_id,
                &mut card.fields,
                store.documents::<Document>(Documents::Runs)?,
                "Une correction a démarré. Relance la review sur les changements obtenus.",
            )?;
            for previous in invalidation.runs {
                store.put(Documents::Runs, &previous)?;
            }
            store.put(Documents::Cards, &card)?;
        }
        for dependency in card
            .fields
            .get::<Vec<String>>("blockedBy")?
            .unwrap_or_default()
        {
            let prerequisite = store
                .document::<Card>(Documents::Cards, &dependency)?
                .ok_or("Une tâche préalable est introuvable.")?;
            if !prerequisite.fields.string_equals("status", "done")? {
                return Err("Termine les tâches préalables avant de lancer un agent.".into());
            }
        }
        if store.document::<Document>(Documents::Runs, &id)?.is_some() {
            return Err("Identifiant d’exécution déjà utilisé.".into());
        }
        for previous in store.documents::<Document>(Documents::Runs)? {
            if active(&previous)?
                && (previous.string_equals("cardId", &card_id)?
                    || atelier_core::domain::projects::same_repository(
                        &previous.get::<String>("repo")?.unwrap_or_default(),
                        &repo,
                        cfg!(windows),
                    ))
            {
                return Err("Un agent travaille déjà sur cette tâche ou dans ce dossier.".into());
            }
        }
        let mut run = run.clone();
        run.set("logBytes", &0usize)?;
        run.set("logEntries", &0usize)?;
        run.set("logTruncated", &false)?;
        store.put(Documents::Runs, &run)
    })
}

pub fn transition(
    store: &Store<'_>,
    id: &str,
    status: &str,
    timestamp: &str,
    code: Option<i32>,
) -> Result<Document> {
    store.transaction(|store| {
        let mut run = store
            .document::<Document>(Documents::Runs, id)?
            .ok_or("Exécution introuvable.")?;
        let current = run
            .get::<String>("status")?
            .ok_or("État d’exécution absent.")?;
        let allowed = matches!(
            (current.as_str(), status),
            ("starting", "running" | "stopping" | "failed")
                | ("running", "stopping" | "completed" | "failed")
                | ("stopping", "cancelled" | "failed")
        );
        if !allowed {
            return Err("Transition d’exécution périmée ou invalide.".into());
        }
        run.set("status", &status)?;
        let purpose = run.get::<String>("purpose")?.unwrap_or_default();
        if status == "completed"
            && ["brainstorm", "preparation", "review"].contains(&purpose.as_str())
        {
            let text = if run.string_equals("agentType", "command")? {
                run.get::<String>("structuredOutput")?.unwrap_or_default()
            } else {
                run.get::<String>("summary")?.unwrap_or_default()
            };
            let parsed = if run.get::<bool>("structuredOverflow")?.unwrap_or(false) {
                Err("Réponse structurée trop volumineuse.")
            } else if purpose == "brainstorm" {
                atelier_core::application::brainstorm::parse(&text)
                    .map(|value| serde_json::to_value(value).expect("brainstorm serialization"))
            } else if purpose == "preparation" {
                atelier_core::application::preparation::parse(&text)
                    .map(|value| serde_json::to_value(value).expect("preparation serialization"))
            } else {
                let context = run
                    .get::<atelier_core::application::reviews::Context>("reviewContext")?
                    .ok_or("Contexte de review absent.")?;
                atelier_core::application::reviews::parse(&text, &context)
                    .map(|value| serde_json::to_value(value).expect("review serialization"))
            };
            let (answer_key, error_key) = if purpose == "brainstorm" {
                ("brainstormAnswer", "brainstormError")
            } else if purpose == "preparation" {
                ("preparation", "preparationError")
            } else {
                ("review", "reviewError")
            };
            match parsed {
                Ok(answer) => run.set(answer_key, &answer)?,
                Err(message) => run.set(error_key, &message)?,
            }
        }
        if !active(&run)? {
            run.set("endedAt", &timestamp)?;
            run.set("exitCode", &code)?;
            run.remove("interactions");
            run.set("waiting", &false)?;
            run.remove("liveText");
            run.remove("structuredOutput");
        }
        store.put(Documents::Runs, &run)?;
        Ok(run)
    })
}

/// Only complete, already redacted messages may cross this storage boundary.
pub fn append(store: &Store<'_>, id: &str, stream: &str, message: &str) -> Result<()> {
    if !["stdout", "stderr", "system"].contains(&stream) {
        return Err("Flux de journal invalide.".into());
    }
    store.transaction(|store| {
        let mut run = store
            .document::<Document>(Documents::Runs, id)?
            .ok_or("Exécution introuvable.")?;
        if !active(&run)? {
            return Err("Exécution terminée.".into());
        }
        if stream == "stdout"
            && matches!(
                run.get::<String>("purpose")?.as_deref(),
                Some("brainstorm" | "preparation" | "review")
            )
            && run.string_equals("agentType", "command")?
        {
            let mut output = run.get::<String>("structuredOutput")?.unwrap_or_default();
            if output.len().saturating_add(message.len()) > 64 * 1024 {
                run.set("structuredOverflow", &true)?;
            } else if !run.get::<bool>("structuredOverflow")?.unwrap_or(false) {
                output.push_str(message);
                run.set("structuredOutput", &output)?;
            }
        }
        let bytes = run.get::<usize>("logBytes")?.unwrap_or(0);
        let entries = run.get::<usize>("logEntries")?.unwrap_or(0);
        if run.get::<bool>("logTruncated")?.unwrap_or(false) {
            return store.put(Documents::Runs, &run);
        }
        if entries.saturating_add(chunks(message).count()) > 10_000
            || bytes
                .checked_add(message.len())
                .is_none_or(|size| size > LOG_LIMIT)
        {
            // Drop the entire message: clipping could expose a partially redacted credential.
            store.log(id, "system", "Journal limité à 5 Mio ou 10 000 entrées.")?;
            run.set("logTruncated", &true)?;
        } else if !message.is_empty() {
            for chunk in chunks(message) {
                store.log(id, stream, chunk)?;
            }
            run.set("logBytes", &(bytes + message.len()))?;
            run.set("logEntries", &(entries + chunks(message).count()))?;
        }
        store.put(Documents::Runs, &run)
    })
}

fn chunks(mut text: &str) -> impl Iterator<Item = &str> {
    std::iter::from_fn(move || {
        if text.is_empty() {
            return None;
        }
        let mut end = text.len().min(16 * 1024);
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        let (head, tail) = text.split_at(end);
        text = tail;
        Some(head)
    })
}

/// Never adopt an old PID or restart an external effect automatically.
pub fn recover(store: &Store<'_>, timestamp: &str) -> Result<usize> {
    store.transaction(|store| recover_in_transaction(store, timestamp))
}

fn recover_in_transaction(store: &Store<'_>, timestamp: &str) -> Result<usize> {
    let mut recovered = 0;
    for mut run in store.documents::<Document>(Documents::Runs)? {
        if !active(&run)? {
            continue;
        }
        let id = run
            .get::<String>("id")?
            .ok_or("Identifiant d’exécution absent.")?;
        run.set("status", &"interrupted")?;
        run.set("endedAt", &timestamp)?;
        run.remove("interactions");
        run.remove("pid");
        run.remove("liveText");
        run.remove("structuredOutput");
        run.set("waiting", &false)?;
        store.log(&id, "system", "Atelier a été interrompu pendant cette exécution. Vérifie le dépôt avant de relancer explicitement.")?;
        store.put(Documents::Runs, &run)?;
        recovered += 1;
    }
    Ok(recovered)
}
