use super::store::{Documents, Store};
use crate::Result;
use atelier_core::{
    application::automation::State as WorkflowState,
    contracts::{Card, QueuedRun, RunQueue, Worktree, WorktreeState},
    document::Document,
};
fn remove(queue: &mut RunQueue, id: &str) {
    queue.ids.retain(|card| card != id);
    queue.workflows.remove(id);
}
fn workflow_state(store: &Store<'_>, active: &QueuedRun) -> Result<Option<WorkflowState>> {
    let Some(card) = store.document::<Card>(Documents::Cards, &active.card_id)? else {
        return Ok(None);
    };
    Ok(super::automation::read(&card)?
        .filter(|flow| flow.id == active.run_id)
        .map(|flow| flow.state))
}
fn succeeded(run: &Document, active: &QueuedRun) -> Result<bool> {
    Ok(run.string_equals("status", "completed")?
        && run.string_equals("cardId", &active.card_id)?
        && run.get::<i32>("exitCode")? == Some(0)
        && !run.has_text("attention")?)
}
pub fn read(store: &Store<'_>) -> Result<RunQueue> {
    let queue = store
        .settings::<Document>()?
        .get::<RunQueue>("runQueue")?
        .unwrap_or_default();
    if queue.ids.len() > 100
        || queue.workflows.iter().any(|(id, operation)| {
            !queue.ids.contains(id)
                || operation
                    .as_ref()
                    .is_some_and(|operation| uuid::Uuid::parse_str(operation).is_err())
        })
        || queue.active.as_ref().is_some_and(|active| {
            queue
                .workflows
                .get(&active.card_id)
                .is_some_and(|operation| operation.as_deref() != Some(active.run_id.as_str()))
        })
        || queue
            .ids
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            != queue.ids.len()
        || queue
            .active
            .as_ref()
            .is_some_and(|active| queue.ids.first() != Some(&active.card_id))
    {
        return Err("message.invalid_run_queue".into());
    }
    Ok(queue)
}
fn save(store: &Store<'_>, mut queue: RunQueue) -> Result<RunQueue> {
    queue.revision = queue
        .revision
        .checked_add(1)
        .ok_or("message.queue_revision_limit_reached")?;
    let mut settings = store.settings::<Document>()?;
    settings.set("runQueue", &queue)?;
    store.save_settings(&settings)?;
    Ok(queue)
}
pub fn needs_tick(store: &Store<'_>) -> Result<bool> {
    let queue = read(store)?;
    let settling = match &queue.active {
        Some(active) if queue.workflows.contains_key(&active.card_id) => !matches!(
            workflow_state(store, active)?,
            Some(WorkflowState::Running | WorkflowState::Waiting)
        ),
        Some(_) => true,
        None => !queue.paused && !queue.ids.is_empty(),
    };
    Ok(settling || super::automation::running(store)?.is_some())
}
pub fn pause(store: &Store<'_>, message: &str) -> Result<RunQueue> {
    store.transaction(|store| {
        let mut queue = read(store)?;
        if queue.paused && queue.message == message {
            return Ok(queue);
        }
        queue.paused = true;
        queue.message = message.into();
        save(store, queue)
    })
}
pub fn change(store: &Store<'_>, body: &Document) -> Result<RunQueue> {
    store.transaction(|store| {
        let mut queue = read(store)?;
        if body.get::<u64>("revision")? != Some(queue.revision) {
            return Err(
                atelier_core::application::card_lifecycle::CardFailure::Conflict(
                    "message.the_queue_changed_reload_before_trying_again",
                )
                .into(),
            );
        }
        if body.get::<bool>("confirmed")? != Some(true) {
            return Err("message.confirm_changing_the_run_queue".into());
        }
        match body.get::<String>("action")?.as_deref() {
            Some("add") => {
                let workflow=body.get::<bool>("workflow")?.unwrap_or(false);
                let expected = body
                    .get::<Card>("expected")?
                    .ok_or("message.task_snapshot_required")?;
                let card = super::forge_association::snapshot(store, &expected)?;
                if card
                    .fields
                    .get::<Worktree>("worktree")?
                    .is_none_or(|worktree| worktree.state != WorktreeState::Ready)
                {
                    return Err("message.prepare_the_worktree_before_adding_this_task_to_the_queue".into());
                }
                if !queue.ids.contains(&card.id) {
                    if queue.ids.len() >= 100 {
                        return Err("message.queue_is_limited_to_100_tasks".into());
                    }
                    if workflow {
                        let project=super::verification::project(store,&card)?;
                        if project.get::<bool>("automationEnabled")?!=Some(true) || project.get::<String>("testCommand")?.is_none_or(|command|command.trim().is_empty()) || project.get::<String>("prTarget")?.is_none_or(|target|target.trim().is_empty()) {return Err("message.configure_and_enable_the_project_workflow_before_adding_it_to_th".into());}
                        let existing=super::automation::read(&card)?;
                        if existing.as_ref().is_some_and(|flow|flow.state==WorkflowState::Blocked) {return Err("message.check_the_blocked_workflow_before_adding_it_to_the_queue_again".into());}
                        queue.workflows.insert(card.id.clone(),existing.filter(|flow|flow.state!=WorkflowState::Ready).map(|flow|flow.id));
                    }
                    queue.ids.push(card.id);
                } else if queue.workflows.contains_key(&card.id)!=workflow {
                    return Err("message.remove_the_task_from_the_queue_before_changing_its_execution_mod".into());
                }
            }
            Some("remove") => {
                let id = body.get::<String>("cardId")?.ok_or("message.card_missing")?;
                if queue
                    .active
                    .as_ref()
                    .is_some_and(|active| active.card_id == id)
                {
                    return Err(
                        "message.wait_for_the_active_task_to_finish_before_removing_it_from_the_q".into(),
                    );
                }
                remove(&mut queue,&id);
            }
            Some("resume") => {
                queue.paused = false;
                queue.message.clear();
            }
            _ => return Err("message.unknown_queue_action".into()),
        }
        save(store, queue)
    })
}
pub fn recover(store: &Store<'_>) -> Result<()> {
    store.transaction(recover_in_transaction)
}
pub(super) fn recover_in_transaction(store: &Store<'_>) -> Result<()> {
    if store.settings::<Document>()?.raw("runQueue").is_none() {
        return Ok(());
    }
    let mut queue = read(store)?;
    if let Some(active) = queue.active.take() {
        let completed = if queue.workflows.contains_key(&active.card_id) {
            workflow_state(store, &active)? == Some(WorkflowState::Ready)
        } else {
            store
                .document::<Document>(Documents::Runs, &active.run_id)?
                .as_ref()
                .map(|run| succeeded(run, &active))
                .transpose()?
                .unwrap_or(false)
        };
        if completed {
            remove(&mut queue, &active.card_id);
        }
    }
    queue.paused = true;
    queue.message = "message.queue_paused_after_restart_check_tasks_before_resuming".into();
    save(store, queue)?;
    Ok(())
}
pub fn claim(store: &Store<'_>, run_id: String) -> Result<Option<(Card, Document)>> {
    store.transaction(|store| {
        if atelier_core::application::agent_cli::busy(&store.settings::<Document>()?)? {
            return Ok(None);
        }
        let mut queue = read(store)?;
        if queue.paused || queue.active.is_some() || queue.ids.is_empty() {
            return Ok(None);
        }
        if super::connector_effects::read(&store.settings::<Document>()?)?
            .iter()
            .any(super::connector_effects::pending)
        {
            queue.paused = true;
            queue.message =
                "message.queue_paused_complete_or_reconcile_the_module_write_before_resum".into();
            save(store, queue)?;
            return Ok(None);
        }
        let card = store
            .document::<Card>(Documents::Cards, &queue.ids[0])?
            .ok_or("message.card_not_found")?;
        super::forge_association::snapshot(store, &card)?;
        if let Some(bound) = queue.workflows.get(&card.id).cloned() {
            let existing = super::automation::read(&card)?;
            let action = if let Some(operation) = bound.as_ref() {
                let flow = existing
                    .as_ref()
                    .filter(|flow| &flow.id == operation)
                    .ok_or(
                        "message.the_queued_workflow_was_replaced_remove_and_add_the_task_again",
                    )?;
                match flow.state {
                    WorkflowState::Running | WorkflowState::Waiting | WorkflowState::Ready => None,
                    WorkflowState::Interrupted
                    | WorkflowState::Error
                    | WorkflowState::Cancelled => Some("resume"),
                    WorkflowState::Blocked => {
                        return Err(
                            "message.check_the_blocked_workflow_before_resuming_the_queue".into(),
                        );
                    }
                }
            } else {
                Some("start")
            };
            let card = if let Some(action) = action {
                let agent = card
                    .fields
                    .get::<String>("agent")?
                    .ok_or("message.profile_required")?;
                let mut body = Document::default();
                body.set("action", &action)?;
                body.set("expected", &card)?;
                body.set("confirmed", &true)?;
                body.set("project", &super::verification::project(store, &card)?)?;
                body.set("profile", &super::automation::profile(store, &agent)?)?;
                super::automation::change_in_transaction(store, &card.id, &body)?
            } else {
                card
            };
            let flow = super::automation::read(&card)?.ok_or("message.queued_workflow_missing")?;
            queue
                .workflows
                .insert(card.id.clone(), Some(flow.id.clone()));
            queue.active = Some(QueuedRun {
                card_id: card.id.clone(),
                run_id: flow.id,
            });
            save(store, queue)?;
            let mut body = Document::default();
            body.set("workflow", &true)?;
            return Ok(Some((card, body)));
        }
        if super::automation::read(&card)?
            .is_some_and(|flow| flow.state == atelier_core::application::automation::State::Running)
        {
            return Err(
                "message.the_workflow_reserves_this_task_stop_it_before_starting_a_manual".into(),
            );
        }
        let agent = card
            .fields
            .get::<String>("agent")?
            .filter(|agent| !agent.is_empty())
            .ok_or("message.profile_required")?;
        let mode = card
            .fields
            .get::<String>("mode")?
            .unwrap_or_else(|| "edit".into());
        if card.fields.flag("brainstormSession")? || !["analyze", "edit"].contains(&mode.as_str()) {
            return Err("message.only_saved_tasks_can_run_in_the_queue".into());
        }
        let mut body = Document::default();
        body.set("agent", &agent)?;
        body.set("mode", &mode)?;
        body.set("purpose", &"work")?;
        body.set(
            "expected",
            &atelier_core::application::jira::card_snapshot(&card),
        )?;
        queue.active = Some(QueuedRun {
            card_id: card.id.clone(),
            run_id,
        });
        save(store, queue)?;
        Ok(Some((card, body)))
    })
}
pub fn require_claim(store: &Store<'_>, run: &Document) -> Result<()> {
    let queue = read(store)?;
    if queue.paused
        || queue.active.is_none_or(|active| {
            queue.workflows.contains_key(&active.card_id)
                || run.get::<String>("id").ok().flatten() != Some(active.run_id)
                || run.get::<String>("cardId").ok().flatten() != Some(active.card_id)
        })
    {
        return Err("message.queue_is_paused_or_its_launch_changed".into());
    }
    Ok(())
}
pub fn settle(store: &Store<'_>) -> Result<bool> {
    store.transaction(|store| {
        let mut queue = read(store)?;
        let Some(active) = queue.active.take() else {
            return Ok(false);
        };
        if queue.workflows.contains_key(&active.card_id) {
            match workflow_state(store, &active)? {
                Some(WorkflowState::Running | WorkflowState::Waiting) => return Ok(false),
                Some(WorkflowState::Ready) => remove(&mut queue, &active.card_id),
                _ => {
                    queue.paused = true;
                    queue.message =
                        "message.workflow_stopped_blocked_or_interrupted_check_the_task_before_re"
                            .into();
                }
            }
            save(store, queue)?;
            return Ok(true);
        }
        let run = store.document::<Document>(Documents::Runs, &active.run_id)?;
        if run
            .as_ref()
            .is_some_and(|run| super::runs::active(run).unwrap_or(true))
        {
            return Ok(false);
        }
        if run
            .as_ref()
            .map(|run| succeeded(run, &active))
            .transpose()?
            .unwrap_or(false)
        {
            remove(&mut queue, &active.card_id);
        } else {
            queue.paused = true;
            queue.message =
                "message.run_stopped_uncertain_or_failed_check_the_task_before_resuming_t".into();
        }
        save(store, queue)?;
        Ok(true)
    })
}
