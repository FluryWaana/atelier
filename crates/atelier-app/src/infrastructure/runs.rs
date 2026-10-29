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
        if super::connector_effects::read(&store.settings::<Document>()?)?
            .iter()
            .any(super::connector_effects::pending)
        {
            return Err("message.complete_or_reconcile_module_writes_before_starting_a_run".into());
        }
        if atelier_core::application::agent_cli::busy(&store.settings::<Document>()?)? {
            return Err("message.cli_update_blocks_agent".into());
        }
        let id = run
            .get::<String>("id")?
            .filter(|id| !id.is_empty())
            .ok_or("message.run_id_missing")?;
        let card_id = run.get::<String>("cardId")?.ok_or("message.card_missing")?;
        let repo = run
            .get::<String>("repo")?
            .filter(|repo| !repo.is_empty())
            .ok_or("message.run_directory_missing")?;
        if !run.string_equals("status", "starting")? {
            return Err("message.initial_run_state_invalid".into());
        }
        if run.get::<bool>("queued")?.unwrap_or(false) {
            super::queue::require_claim(store, run)?;
        }
        let mut card = store
            .document::<Card>(Documents::Cards, &card_id)?
            .ok_or("message.card_not_found")?;
        if card.fields.truthy("archived")? {
            return Err("message.restore_task_before_agent".into());
        }
        super::automation::require_run(
            store,
            &card,
            &id,
            run.get::<String>("automationId")?.as_deref(),
        )?;
        if atelier_core::application::jira::unresolved(&card.fields)? {
            return Err(
                "message.complete_or_reconcile_the_jira_operation_before_starting_a_run".into(),
            );
        }
        if atelier_core::application::forge::unresolved(&card.fields)? {
            return Err("message.check_the_forge_operation_before_starting_a_run".into());
        }
        if super::tempo_export::blocks(store, &format!("/api/cards/{card_id}/runs/start"))? {
            return Err("message.finish_or_check_the_time_export_before_starting_a_run".into());
        }
        if run.string_equals("purpose", "verification")? {
            let worktree = run
                .get::<atelier_core::contracts::Worktree>("verificationWorktree")?
                .filter(|worktree| worktree.state == atelier_core::contracts::WorktreeState::Ready)
                .ok_or("message.prepare_the_worktree_before_verification")?;
            if !atelier_core::domain::projects::same_repository(
                &repo,
                &worktree.path,
                cfg!(windows),
            ) {
                return Err("message.requirements_or_worktree_changed_before_verification".into());
            }
            super::forge_association::snapshot(store, &card)?;
            super::verification::require_command(
                store,
                &card,
                &run.get::<String>("verificationCommand")?
                    .ok_or("message.invalid_verification_command")?,
            )?;
            if run.get::<serde_json::Value>("requirements")?.as_ref()
                != Some(&atelier_core::application::preparation::requirements(
                    &card,
                )?)
                || card
                    .fields
                    .get::<atelier_core::contracts::Worktree>("worktree")?
                    != run.get::<atelier_core::contracts::Worktree>("verificationWorktree")?
            {
                return Err("message.requirements_or_worktree_changed_before_verification".into());
            }
        }
        if run.string_equals("kind", "pr-review")? {
            if let Some(context) = run
                .get::<atelier_core::application::extensions::RepositoryReviewContext>(
                "connectorReviewContext",
            )? {
                super::connector_repositories::review_matches(store, &card, &context)?;
            } else {
                let context = run
                    .get::<atelier_core::application::reviews::Context>("reviewContext")?
                    .ok_or("message.missing_review_context")?;
                super::reviews::matches(store, &card, &context)?;
            }
        }
        if run.string_equals("purpose", "correction")? {
            if !run.string_equals("mode", "edit")? {
                return Err("message.confirm_the_selected_correction_in_edit_mode".into());
            }
            if let Some(context) = run
                .get::<atelier_core::application::extensions::RepositoryReviewContext>(
                "connectorReviewContext",
            )? {
                let review = run
                    .get::<String>("correctionOf")?
                    .ok_or("message.review_missing")?;
                let indexes = run
                    .get::<Vec<usize>>("correctionIndexes")?
                    .ok_or("message.findings_missing")?;
                if super::connector_repositories::correction(store, &card, &review, &indexes)?.0
                    != context
                {
                    return Err("message.connector_commits_criteria_changed".into());
                }
            } else {
                let context = run
                    .get::<atelier_core::application::reviews::Context>("reviewContext")?
                    .ok_or("message.missing_review_context")?;
                let review = run
                    .get::<String>("correctionOf")?
                    .ok_or("message.review_not_found")?;
                let indexes = run
                    .get::<Vec<usize>>("correctionIndexes")?
                    .ok_or("message.invalid_finding_selection")?;
                if super::reviews::correction(store, &card, &review, &indexes)?.0 != context {
                    return Err(
                        "message.commits_or_requirements_changed_run_the_review_again".into(),
                    );
                }
            }
            let invalidation = atelier_core::application::task_editing::invalidate_reviews(
                &card_id,
                &mut card.fields,
                store.documents::<Document>(Documents::Runs)?,
                "message.a_correction_started_review_the_resulting_changes_again",
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
                .ok_or("message.prerequisite_task_missing")?;
            if !prerequisite.fields.string_equals("status", "done")? {
                return Err("message.complete_prerequisite_tasks_before_starting_an_agent".into());
            }
        }
        if store.document::<Document>(Documents::Runs, &id)?.is_some() {
            return Err("message.run_id_already_used".into());
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
                return Err(
                    "message.an_agent_is_already_working_on_this_task_or_in_this_directory".into(),
                );
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
            .ok_or("message.run_not_found")?;
        let current = run
            .get::<String>("status")?
            .ok_or("message.run_state_missing")?;
        let allowed = matches!(
            (current.as_str(), status),
            ("starting", "running" | "stopping" | "failed")
                | ("running", "stopping" | "completed" | "failed")
                | ("stopping", "cancelled" | "failed")
        );
        if !allowed {
            return Err("message.run_transition_invalid".into());
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
                Err("message.structured_response_too_large")
            } else if purpose == "brainstorm" {
                atelier_core::application::brainstorm::parse(&text)
                    .map(|value| serde_json::to_value(value).expect("brainstorm serialization"))
            } else if purpose == "preparation" {
                atelier_core::application::preparation::parse(&text)
                    .map(|value| serde_json::to_value(value).expect("preparation serialization"))
            } else if let Some(context) =
                run.get::<atelier_core::application::extensions::RepositoryReviewContext>(
                    "connectorReviewContext",
                )?
            {
                atelier_core::application::reviews::parse_requirements(&text, &context.requirements)
                    .map(|value| serde_json::to_value(value).expect("review serialization"))
            } else {
                let context = run
                    .get::<atelier_core::application::reviews::Context>("reviewContext")?
                    .ok_or("message.missing_review_context")?;
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
        return Err("message.log_stream_invalid".into());
    }
    store.transaction(|store| {
        let mut run = store
            .document::<Document>(Documents::Runs, id)?
            .ok_or("message.run_not_found")?;
        if !active(&run)? {
            return Err("message.run_finished".into());
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
        if run.string_equals("purpose", "verification")?
            && run.string_equals("verificationState", "running")?
        {
            run.set("verificationState", &"interrupted")?;
            store.put(Documents::Runs, &run)?;
        }
        if !active(&run)? {
            continue;
        }
        let id = run.get::<String>("id")?.ok_or("message.run_id_missing")?;
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
