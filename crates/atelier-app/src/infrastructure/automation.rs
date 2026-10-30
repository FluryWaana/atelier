use super::{
    runs,
    store::{Documents, Store},
    verification,
};
use crate::Result;
use atelier_core::{
    application::{
        automation::{Flow, PublicationPlan, State, Step},
        preparation,
        reviews::{Coverage, Priority, Review},
    },
    contracts::{Card, Worktree, WorktreeState},
    document::Document,
};

fn now() -> Result<String> {
    Ok(time::OffsetDateTime::now_utc().format(&time::format_description::well_known::Rfc3339)?)
}
pub(super) fn profile(store: &Store<'_>, id: &str) -> Result<Document> {
    atelier_core::domain::agent_options::providers(&atelier_core::application::settings::config(
        &store.settings::<Document>()?,
    )?)?
    .into_iter()
    .find(|profile| profile.string_equals("id", id).unwrap_or(false))
    .ok_or_else(|| "message.workflow_profile_not_found".into())
}
pub fn read(card: &Card) -> Result<Option<Flow>> {
    let Some(value) = card.fields.get::<Document>("automation")? else {
        return Ok(None);
    };
    if !value.has_text("id")? {
        if value.string_equals("state", "running")? {
            return Err(
                "message.active_workflow_has_no_identifier_inspect_processes_before_conti".into(),
            );
        }
        return Ok(None);
    }
    Ok(card.fields.get("automation")?)
}
fn save(store: &Store<'_>, card: &mut Card, flow: &mut Flow) -> Result<()> {
    let previous = card
        .fields
        .get::<Document>("automation")?
        .and_then(|value| value.get::<State>("state").ok().flatten());
    let status = card.fields.get::<String>("status")?;
    if !card.fields.flag("archived")?
        && status.as_deref() != Some("done")
        && previous.is_some_and(|previous| status.as_deref() == Some(previous.card_status()))
    {
        card.fields.set("status", &flow.state.card_status())?;
    }
    flow.updated_at = now()?;
    card.fields.set("updatedAt", &flow.updated_at)?;
    card.fields.set("automation", flow)?;
    store.put(Documents::Cards, card)
}
fn current(store: &Store<'_>, card: &Card, flow: &Flow) -> Result<()> {
    if flow.requirements != preparation::requirements(card)?
        || card.fields.get::<Worktree>("worktree")?.as_ref() != Some(&flow.worktree)
        || card
            .fields
            .get::<String>("instructions")?
            .unwrap_or_default()
            != flow.instructions
        || card.fields.get::<String>("agent")?.unwrap_or_default() != flow.agent
        || card.fields.get::<String>("model")?.unwrap_or_default() != flow.model
        || card.fields.get::<String>("effort")?.unwrap_or_default() != flow.effort
        || serde_json::to_value(profile(store, &flow.agent)?)?
            != serde_json::to_value(&flow.profile)?
        || serde_json::to_value(verification::project(store, card)?)?
            != serde_json::to_value(&flow.project)?
    {
        return Err(
            "message.workflow_requirements_profile_worktree_or_project_changed_start".into(),
        );
    }
    Ok(())
}

pub fn change(store: &Store<'_>, id: &str, body: &Document) -> Result<Card> {
    store.transaction(|store| change_in_transaction(store, id, body))
}

pub(super) fn change_in_transaction(store: &Store<'_>, id: &str, body: &Document) -> Result<Card> {
    if body.get::<bool>("confirmed")? != Some(true) {
        return Err("message.confirm_the_automatic_workflow_step".into());
    }
    let expected = body
        .get::<Card>("expected")?
        .filter(|card| card.id == id)
        .ok_or("message.task_snapshot_required")?;
    let mut card = super::forge_association::snapshot(store, &expected)?;
    let mut flow = read(&card)?;
    match body.get::<String>("action")?.as_deref() {
        Some("start") => {
            if flow
                .as_ref()
                .is_some_and(|flow| matches!(flow.state, State::Running | State::Waiting))
            {
                return Err("message.stop_the_previous_workflow_before_starting_another".into());
            }
            let project = verification::project(store, &card)?;
            if body
                .get::<Document>("project")?
                .as_ref()
                .map(serde_json::to_value)
                .transpose()?
                != Some(serde_json::to_value(&project)?)
            {
                return Err(
                    "message.workflow_settings_changed_reload_the_project_before_confirming".into(),
                );
            }
            if project.get::<bool>("automationEnabled")? != Some(true)
                || project
                    .get::<String>("testCommand")?
                    .is_none_or(|value| value.trim().is_empty())
                || project
                    .get::<String>("prTarget")?
                    .is_none_or(|value| value.trim().is_empty())
            {
                return Err(
                    "message.enable_workflows_and_set_the_verification_command_and_target_in"
                        .into(),
                );
            }
            let worktree = card
                .fields
                .get::<Worktree>("worktree")?
                .filter(|worktree| worktree.state == WorktreeState::Ready)
                .ok_or("message.prepare_the_worktree_before_starting_the_workflow")?;
            let agent = card
                .fields
                .get::<String>("agent")?
                .filter(|value| !value.is_empty())
                .ok_or("message.profile_required")?;
            let auto_fix = project.get::<bool>("autoFix")?.unwrap_or(true);
            let profile = profile(store, &agent)?;
            if body
                .get::<Document>("profile")?
                .as_ref()
                .map(serde_json::to_value)
                .transpose()?
                != Some(serde_json::to_value(&profile)?)
            {
                return Err(
                    "message.the_workflow_profile_changed_review_its_settings_before_confirmi"
                        .into(),
                );
            }
            let stamp = now()?;
            flow = Some(Flow {
                id: uuid::Uuid::new_v4().to_string(),
                state: State::Running,
                step: Step::Development,
                project,
                requirements: preparation::requirements(&card)?,
                worktree,
                agent,
                profile,
                model: card.fields.get::<String>("model")?.unwrap_or_default(),
                effort: card.fields.get::<String>("effort")?.unwrap_or_default(),
                instructions: card
                    .fields
                    .get::<String>("instructions")?
                    .unwrap_or_default(),
                run_id: None,
                review_id: None,
                verification_id: None,
                publication_plan: None,
                commit_plan: None,
                review_requested: false,
                correction_indexes: Vec::new(),
                cycles: 0,
                auto_fix,
                started_at: stamp.clone(),
                updated_at: stamp,
                message: String::new(),
            });
        }
        Some("resume") => {
            let active = flow.as_mut().ok_or("message.no_workflow_to_resume")?;
            if !matches!(
                active.state,
                State::Interrupted | State::Error | State::Cancelled
            ) {
                return Err("message.this_workflow_cannot_resume_at_this_step".into());
            }
            current(store, &card, active)?;
            // Repeating an interrupted local step requires this new explicit confirmation.
            active.run_id = None;
            active.state = State::Running;
            active.message.clear();
            active.publication_plan = None;
            active.commit_plan = None;
            if matches!(active.step, Step::Commit | Step::Publication | Step::Review) {
                active.step = Step::Tests;
            }
        }
        Some("review") => {
            let active = flow
                .as_mut()
                .filter(|flow| flow.state == State::Waiting && flow.step == Step::Publication)
                .ok_or("message.the_workflow_is_not_awaiting_publication")?;
            current(store, &card, active)?;
            let branches = if let Some(linked) =
                card.fields
                    .get::<atelier_core::application::extensions::ModulePullRequestLink>(
                        "connectorPullRequest",
                    )? {
                (
                    linked.pull_request.source_branch,
                    linked.pull_request.target_branch,
                )
            } else {
                let linked = card
                    .fields
                    .get::<atelier_core::contracts::PullRequest>("pullRequest")?
                    .ok_or("message.publish_or_link_a_pr_mr_before_continuing_the_review")?;
                (linked.source_branch, linked.target_branch)
            };
            if active.project.get::<String>("prTarget")?.as_deref() != Some(branches.1.as_str())
                || branches.0 != active.worktree.branch
            {
                return Err("message.the_pr_mr_branches_do_not_match_the_workflow".into());
            }
            active.review_requested = true;
            active.step = Step::Tests;
            active.state = State::Running;
            active.run_id = None;
            active.message.clear();
        }
        Some("commit") => {
            let active = flow
                .as_mut()
                .filter(|flow| flow.state == State::Waiting && flow.step == Step::Publication)
                .ok_or("message.the_workflow_is_not_awaiting_publication")?;
            current(store, &card, active)?;
            let preview = card
                .fields
                .get::<atelier_core::contracts::LocalCommitPreview>("localCommitPreview")?
                .ok_or("message.prepare_the_commit_preview_before_authorizing_the_workflow")?;
            if preview.worktree != active.worktree
                || preview.snapshot.branch != active.worktree.branch
            {
                return Err("message.the_commit_does_not_match_the_workflow_worktree".into());
            }
            active.commit_plan = Some(Box::new(preview));
            active.publication_plan = None;
            active.review_requested = false;
            active.step = Step::Commit;
            active.state = State::Running;
            active.run_id = None;
            active.message.clear();
        }
        Some("publish") => {
            let active = flow
                .as_mut()
                .filter(|flow| flow.state == State::Waiting && flow.step == Step::Publication)
                .ok_or("message.the_workflow_is_not_awaiting_publication")?;
            current(store, &card, active)?;
            let target = active
                .project
                .get::<String>("prTarget")?
                .ok_or("message.target_branch_missing")?;
            let plan = if let Some(preview) =
                card.fields
                    .get::<atelier_core::application::extensions::RepositoryProposal>(
                        "connectorRepositoryPreview",
                    )? {
                if preview.context.worktree != active.worktree
                    || preview
                        .observation
                        .pull_request
                        .as_ref()
                        .is_some_and(|pr| !pr.draft || pr.auto_merge || pr.status != "active")
                    || preview.observation.target_branch != target
                    || !preview.draft
                    || preview.card_revision != super::connector_repositories::revision(&card)?
                {
                    return Err("message.connector_preview_workflow_mismatch".into());
                }
                super::connector_repositories::matches(store, &card, &preview.context)?;
                PublicationPlan::Module(Box::new(preview))
            } else if card.fields.raw("pullRequest").is_some() {
                let preview = card
                    .fields
                    .get::<atelier_core::contracts::GitPushPreview>("gitPushPreview")?
                    .ok_or("message.prepare_the_push_preview_before_authorizing_the_workflow")?;
                let linked = card
                    .fields
                    .get::<atelier_core::contracts::PullRequest>("pullRequest")?
                    .ok_or("message.pull_request_missing")?;
                if preview.worktree != active.worktree
                    || preview
                        .pull_request
                        .as_ref()
                        .is_none_or(|remote| !super::forge::same_pull_request(remote, &linked))
                    || linked.target_branch != target
                {
                    return Err("message.the_preview_does_not_match_the_workflow_branches".into());
                }
                super::forge::configuration(store, preview.configuration.kind, preview.revision)?;
                PublicationPlan::Push(Box::new(preview))
            } else {
                let preview = card
                    .fields
                    .get::<atelier_core::contracts::ForgePublicationPreview>(
                        "forgePublicationPreview",
                    )?
                    .ok_or(
                        "message.prepare_the_publication_preview_before_authorizing_the_workflow",
                    )?;
                if preview.worktree != active.worktree || preview.target != target || !preview.draft
                {
                    return Err(
                        "message.the_workflow_requires_the_expected_branches_and_a_draft_publicat"
                            .into(),
                    );
                }
                if preview.remote_source.is_none()
                    && body.get::<bool>("allowCreateBranch")? != Some(true)
                {
                    return Err(
                        "message.separately_confirm_creation_of_the_missing_remote_branch".into(),
                    );
                }
                super::forge::configuration(store, preview.configuration.kind, preview.revision)?;
                PublicationPlan::Create(Box::new(preview))
            };
            active.publication_plan = Some(plan);
            active.review_requested = false;
            active.step = Step::Tests;
            active.state = State::Running;
            active.run_id = None;
            active.message.clear();
        }
        _ => return Err("message.unknown_workflow_action".into()),
    }
    card.fields.set("status", &"doing")?;
    save(
        store,
        &mut card,
        flow.as_mut().ok_or("message.workflow_missing")?,
    )?;
    Ok(card)
}

pub fn cancel(store: &Store<'_>, id: &str, operation: &str) -> Result<Option<String>> {
    store.transaction(|store| {
        let mut card = store
            .document::<Card>(Documents::Cards, id)?
            .ok_or("message.card_not_found")?;
        let mut flow = read(&card)?
            .filter(|flow| {
                flow.id == operation && matches!(flow.state, State::Running | State::Waiting)
            })
            .ok_or("message.the_workflow_changed_or_is_already_stopped")?;
        flow.state = State::Cancelled;
        flow.message =
            "message.workflow_stopped_check_files_and_remote_effects_before_resuming".into();
        let run = flow.run_id.clone();
        save(store, &mut card, &mut flow)?;
        Ok(run)
    })
}

pub fn recover_in_transaction(store: &Store<'_>) -> Result<()> {
    for mut card in store.documents::<Card>(Documents::Cards)? {
        if let Some(mut flow) = read(&card)?
            && flow.state == State::Running
        {
            flow.state = State::Interrupted;
            flow.message =
                "message.workflow_interrupted_after_restart_no_run_or_publication_was_rep".into();
            save(store, &mut card, &mut flow)?;
        }
    }
    Ok(())
}
pub fn recover(store: &Store<'_>) -> Result<()> {
    store.transaction(recover_in_transaction)
}
pub fn require_run(
    store: &Store<'_>,
    card: &Card,
    run_id: &str,
    operation: Option<&str>,
) -> Result<()> {
    let flow = read(card)?;
    if let Some(operation) = operation {
        let flow = flow
            .filter(|flow| {
                flow.id == operation
                    && flow.state == State::Running
                    && flow.run_id.as_deref() == Some(run_id)
            })
            .ok_or("message.the_workflow_launch_was_stopped_or_replaced")?;
        current(store, card, &flow)?;
    } else if flow.is_some_and(|flow| flow.state == State::Running) {
        return Err(
            "message.the_workflow_reserves_this_task_stop_it_before_starting_a_manual".into(),
        );
    }
    Ok(())
}
pub fn blocks(store: &Store<'_>, path: &str) -> Result<bool> {
    let affected = path
        .strip_prefix("/api/cards/")
        .map(|tail| tail.split('/').next().unwrap_or_default());
    for card in store.documents::<Card>(Documents::Cards)? {
        if read(&card)?.is_some_and(|flow| flow.state == State::Running)
            && (path.starts_with("/api/backups")
                || path.starts_with("/api/updates")
                || path.starts_with("/api/projects")
                || path.starts_with("/api/settings")
                || path.starts_with("/api/agent-providers")
                || affected == Some(card.id.as_str()))
        {
            return Ok(true);
        }
    }
    Ok(false)
}
pub fn running(store: &Store<'_>) -> Result<Option<(Card, Flow)>> {
    if super::connector_effects::read(&store.settings::<Document>()?)?
        .iter()
        .any(super::connector_effects::pending)
    {
        return Ok(None);
    }
    for card in store.documents::<Card>(Documents::Cards)? {
        if let Some(flow) = read(&card)?
            && flow.state == State::Running
        {
            return Ok(Some((card, flow)));
        }
    }
    Ok(None)
}
pub fn fail(store: &Store<'_>, id: &str, operation: &str, message: &str) -> Result<()> {
    store.transaction(|store| {
        let mut card = store
            .document::<Card>(Documents::Cards, id)?
            .ok_or("message.card_not_found")?;
        let Some(mut flow) =
            read(&card)?.filter(|flow| flow.id == operation && flow.state == State::Running)
        else {
            return Ok(());
        };
        flow.state = State::Error;
        flow.message = message.chars().take(1000).collect();
        save(store, &mut card, &mut flow)
    })
}

pub fn commit(
    store: &Store<'_>,
    id: &str,
    operation: &str,
) -> Result<(Card, atelier_core::contracts::LocalCommitPreview)> {
    let card = store
        .document::<Card>(Documents::Cards, id)?
        .ok_or("message.card_not_found")?;
    let card = super::forge_association::snapshot(store, &card)?;
    let flow = read(&card)?
        .filter(|flow| {
            flow.id == operation
                && flow.state == State::Running
                && flow.step == Step::Commit
                && flow.run_id.is_none()
        })
        .ok_or("message.the_workflow_commit_was_stopped_or_replaced")?;
    current(store, &card, &flow)?;
    Ok((
        card,
        *flow
            .commit_plan
            .ok_or("message.workflow_commit_preview_missing")?,
    ))
}
pub fn commit_complete(store: &Store<'_>, expected: &Card, operation: &str) -> Result<()> {
    store.transaction(|store| {
        super::forge_association::snapshot(store, expected)?;
        let (mut card, approved) = commit(store, &expected.id, operation)?;
        let effect = card
            .fields
            .get::<atelier_core::contracts::LocalCommitIntent>("localCommit")?
            .ok_or("message.missing_commit_intent")?;
        if effect.state != atelier_core::contracts::ForgeWriteState::Confirmed
            || effect.commit.is_none()
            || effect.observed != effect.commit
            || serde_json::to_value(effect.preview)? != serde_json::to_value(approved)?
        {
            return Err(
                "message.the_workflow_commit_is_not_confirmed_against_its_preview_read_th".into(),
            );
        }
        let mut flow = read(&card)?.ok_or("message.workflow_missing")?;
        flow.commit_plan = None;
        flow.publication_plan = None;
        flow.verification_id = None;
        flow.step = Step::Tests;
        flow.run_id = None;
        flow.review_requested = false;
        flow.message =
            "message.commit_observed_sources_will_be_checked_again_before_publication".into();
        save(store, &mut card, &mut flow)
    })
}

pub fn publication(
    store: &Store<'_>,
    id: &str,
    operation: &str,
) -> Result<(Card, Flow, PublicationPlan)> {
    let card = store
        .document::<Card>(Documents::Cards, id)?
        .ok_or("message.card_not_found")?;
    let card = super::forge_association::snapshot(store, &card)?;
    let flow = read(&card)?
        .filter(|flow| {
            flow.id == operation
                && flow.state == State::Running
                && flow.step == Step::Publication
                && flow.run_id.is_none()
        })
        .ok_or("message.workflow_publication_was_stopped_or_replaced")?;
    current(store, &card, &flow)?;
    let plan = flow
        .publication_plan
        .clone()
        .ok_or("message.workflow_publication_preview_missing")?;
    Ok((card, flow, plan))
}

pub fn publication_complete(store: &Store<'_>, expected: &Card, operation: &str) -> Result<()> {
    store.transaction(|store| {
        super::forge_association::snapshot(store, expected)?;
        let (mut card, mut flow, plan) = publication(store, &expected.id, operation)?;
        if let PublicationPlan::Module(approved) = &plan {
            let linked = card
                .fields
                .get::<atelier_core::application::extensions::ModulePullRequestLink>(
                    "connectorPullRequest",
                )?
                .ok_or("message.connector_pull_request_missing")?;
            if linked.module != approved.context.module
                || linked.revision != approved.context.revision
                || linked.pull_request.reference != approved.observation.reference
                || linked.pull_request.source_commit != approved.context.source
                || linked.pull_request.target_commit != approved.context.target
                || linked.pull_request.status != "active"
                || !linked.pull_request.draft
                || linked.pull_request.auto_merge
            {
                return Err("message.connector_pull_request_mismatch".into());
            }
            flow.publication_plan = None;
            flow.review_requested = true;
            flow.step = Step::Review;
            flow.run_id = None;
            flow.message = "message.module_publication_observed_review".into();
            return save(store, &mut card, &mut flow);
        }
        let linked = card
            .fields
            .get::<atelier_core::contracts::PullRequest>("pullRequest")?
            .ok_or("message.the_published_pr_mr_is_missing")?;
        let (kind, revision, repository, commit, target_commit) = match &plan {
            PublicationPlan::Create(preview) => (
                preview.configuration.kind,
                preview.revision,
                &preview.repository,
                &preview.source_commit,
                Some(preview.target_commit.as_str()),
            ),
            PublicationPlan::Push(preview) => (
                preview.configuration.kind,
                preview.revision,
                &preview.repository,
                &preview.commit,
                preview
                    .pull_request
                    .as_ref()
                    .and_then(|remote| remote.target_commit.as_deref()),
            ),
            PublicationPlan::Module(_) => unreachable!("module publication handled above"),
        };
        if linked.kind != kind
            || linked.revision != revision
            || linked.repository != *repository
            || linked.source_commit.as_ref() != Some(commit)
            || linked.target_commit.as_deref() != target_commit
            || linked.source_branch != flow.worktree.branch
            || flow.project.get::<String>("prTarget")?.as_deref()
                != Some(linked.target_branch.as_str())
            || linked.status != "active"
            || linked.fork
        {
            return Err(
                "message.the_observed_pr_mr_does_not_match_the_approved_publication_check".into(),
            );
        }
        flow.publication_plan = None;
        flow.review_requested = true;
        flow.step = Step::Review;
        flow.run_id = None;
        flow.message =
            "message.publication_observed_review_will_examine_the_verified_commits".into();
        save(store, &mut card, &mut flow)
    })
}

pub fn claim(
    store: &Store<'_>,
    id: &str,
    operation: &str,
    run_id: String,
) -> Result<(Card, Flow, Document)> {
    store.transaction(|store| {
        let card = store
            .document::<Card>(Documents::Cards, id)?
            .ok_or("message.card_not_found")?;
        let mut card = super::forge_association::snapshot(store, &card)?;
        let mut flow = read(&card)?
            .filter(|flow| {
                flow.id == operation && flow.state == State::Running && flow.run_id.is_none()
            })
            .ok_or("message.the_workflow_step_changed")?;
        current(store, &card, &flow)?;
        flow.run_id = Some(run_id);
        save(store, &mut card, &mut flow)?;
        let mut body = Document::default();
        body.set("automationId", &flow.id)?;
        body.set("expected", &card)?;
        body.set("agent", &flow.agent)?;
        body.set("model", &flow.model)?;
        body.set("effort", &flow.effort)?;
        body.set("confirmed", &true)?;
        match flow.step {
            Step::Development => {
                body.set("purpose", &"work")?;
                body.set("mode", &"edit")?;
            }
            Step::Tests => {
                body.set(
                    "command",
                    &flow
                        .project
                        .get::<String>("testCommand")?
                        .ok_or("message.verification_command_missing")?,
                )?;
            }
            Step::Review => {
                body.set("purpose", &"review")?;
                body.set("mode", &"analyze")?;
            }
            Step::Correction => {
                body.set("purpose", &"correction")?;
                body.set("mode", &"edit")?;
                body.set("reviewId", &flow.review_id)?;
                body.set("indexes", &flow.correction_indexes)?;
            }
            Step::Publication => return Err("message.publication_requires_explicit_review".into()),
            Step::Commit => return Err("message.the_commit_requires_a_confirmed_preview".into()),
        }
        Ok((card, flow, body))
    })
}

pub fn settle(store: &Store<'_>, id: &str, operation: &str) -> Result<()> {
    store.transaction(|store| {
        let mut card = store
            .document::<Card>(Documents::Cards, id)?
            .ok_or("message.card_not_found")?;
        let mut flow = read(&card)?
            .filter(|flow| flow.id == operation && flow.state == State::Running)
            .ok_or("message.the_workflow_step_changed")?;
        current(store, &card, &flow)?;
        let run_id = flow.run_id.as_ref().ok_or("message.workflow_run_missing")?;
        let run = store
            .document::<Document>(Documents::Runs, run_id)?
            .ok_or("message.launch_interrupted_check_the_worktree_before_resuming")?;
        if runs::active(&run)? {
            return Ok(());
        }
        if !run.string_equals("automationId", operation)?
            || !run.string_equals("cardId", id)?
            || !run.string_equals("status", "completed")?
            || run.get::<i32>("exitCode")? != Some(0)
            || run.has_text("attention")?
        {
            return Err(
                "message.the_workflow_step_did_not_complete_read_the_log_before_resuming".into(),
            );
        }
        match flow.step {
            Step::Development | Step::Correction => {
                flow.step = Step::Tests;
                flow.review_requested = false;
                flow.publication_plan = None;
                flow.commit_plan = None;
            }
            Step::Tests => {
                if !run.string_equals("purpose", "verification")?
                    || !run.string_equals("verificationState", "passed")?
                {
                    return Err(
                        "message.workflow_verification_failed_or_its_sources_changed".into(),
                    );
                }
                flow.verification_id = Some(run_id.clone());
                if flow.publication_plan.is_some() {
                    flow.step = Step::Publication;
                } else if flow.review_requested {
                    flow.step = Step::Review;
                } else {
                    flow.step = Step::Publication;
                    flow.state = State::Waiting;
                    flow.message =
                        "message.verification_passed_review_the_commit_push_and_pr_mr_before_cont"
                            .into();
                }
            }
            Step::Review => {
                if run.has_text("reviewStaleReason")? || !run.has_text("reviewValidatedAt")? {
                    return Err(
                        "message.revalidate_the_workflow_review_against_the_current_commits".into(),
                    );
                }
                let review = run
                    .get::<Review>("review")?
                    .ok_or("message.review_incomplete")?;
                if review.criteria.len()
                    != flow.requirements["criteria"].as_array().map_or(0, Vec::len)
                    || review
                        .criteria
                        .iter()
                        .enumerate()
                        .any(|(index, criterion)| criterion.index != index)
                {
                    return Err(
                        "message.the_workflow_report_does_not_exactly_cover_the_saved_criteria"
                            .into(),
                    );
                }
                flow.review_id = Some(run_id.clone());
                let indexes = review
                    .findings
                    .iter()
                    .enumerate()
                    .filter(|(_, finding)| matches!(finding.priority, Priority::P0 | Priority::P1))
                    .map(|(index, _)| index)
                    .collect::<Vec<_>>();
                if !indexes.is_empty() && flow.auto_fix && flow.cycles < 2 {
                    flow.cycles += 1;
                    flow.correction_indexes = indexes;
                    flow.step = Step::Correction;
                } else {
                    let blocked = !indexes.is_empty()
                        || review.criteria.is_empty()
                        || review
                            .criteria
                            .iter()
                            .any(|criterion| criterion.status != Coverage::Covered);
                    flow.state = if blocked {
                        State::Blocked
                    } else {
                        State::Ready
                    };
                    flow.message = if blocked {
                        "message.human_review_required_blocking_findings_or_incomplete_criteria"
                    } else {
                        "message.pr_mr_analyzed_human_acceptance_of_the_criteria_is_still_require"
                    }
                    .into();
                }
            }
            Step::Publication => return Err("message.publication_requires_explicit_review".into()),
            Step::Commit => return Err("message.the_commit_requires_a_confirmed_preview".into()),
        }
        flow.run_id = None;
        save(store, &mut card, &mut flow)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn doc(value: serde_json::Value) -> Document {
        serde_json::from_str(&value.to_string()).unwrap()
    }
    fn seed(store: &Store<'_>) -> (Card, Document) {
        let project = doc(
            json!({"id":"project","name":"Synthetic","repo":"synthetic/source","automationEnabled":true,"autoFix":true,"testCommand":"synthetic-verification","prTarget":"main"}),
        );
        store.put(Documents::Projects, &project).unwrap();
        let card:Card=serde_json::from_str(&json!({"id":"task","title":"Synthetic workflow","description":"No process or remote call in this storage fixture","agent":"codex","mode":"edit","repo":"synthetic/source","checklist":[{"text":"Synthetic criterion","done":false}],"worktree":{"source":"synthetic/source","path":"synthetic/work","branch":"task","base":"main","baseCommit":"a".repeat(40),"state":"ready"}}).to_string()).unwrap();
        store.put(Documents::Cards, &card).unwrap();
        (card, project)
    }
    fn body(card: &Card, project: &Document, action: &str) -> Document {
        let profile = atelier_core::domain::agent_options::providers(&Document::default())
            .unwrap()
            .remove(0);
        doc(
            json!({"expected":card,"project":project,"profile":profile,"action":action,"confirmed":true}),
        )
    }
    fn card(store: &Store<'_>) -> Card {
        store.document(Documents::Cards, "task").unwrap().unwrap()
    }
    fn synthetic_stage_result(
        store: &Store<'_>,
        flow: &Flow,
        suffix: &str,
        fields: serde_json::Value,
    ) {
        let id = format!("synthetic-{suffix}");
        claim(store, "task", &flow.id, id.clone()).unwrap();
        let mut run = doc(
            json!({"id":id,"cardId":"task","automationId":flow.id,"status":"completed","exitCode":0}),
        );
        run.overlay(&doc(fields));
        store.put(Documents::Runs, &run).unwrap();
    }

    #[test]
    fn persisted_reservations_cancel_late_launches_and_restart_requires_explicit_resume() {
        let home = tempfile::tempdir().unwrap();
        let path = home.path().join("synthetic.sqlite");
        let connection = super::super::store::open(&path).unwrap();
        let store = Store::new(&connection);
        let (initial, project) = seed(&store);
        let mut request = body(&initial, &project, "start");
        request.set("confirmed", &false).unwrap();
        assert!(change(&store, "task", &request).is_err());
        request.set("confirmed", &true).unwrap();
        let started = change(&store, "task", &request).unwrap();
        assert!(started.fields.string_equals("status", "doing").unwrap());
        let flow = read(&started).unwrap().unwrap();
        assert!(blocks(&store, "/api/backups/restore").unwrap());
        assert!(blocks(&store, "/api/projects/project").unwrap());
        assert!(blocks(&store, "/api/updates/apply").unwrap());
        assert!(!blocks(&store, "/api/cards/other").unwrap());
        let (reserved, _, _) = claim(&store, "task", &flow.id, "late-run".into()).unwrap();
        assert!(require_run(&store, &reserved, "manual", None).is_err());
        assert!(require_run(&store, &reserved, "late-run", Some(&flow.id)).is_ok());
        cancel(&store, "task", &flow.id).unwrap();
        assert!(
            card(&store)
                .fields
                .string_equals("status", "standby")
                .unwrap()
        );
        assert!(require_run(&store, &card(&store), "late-run", Some(&flow.id)).is_err());
        let late = doc(
            json!({"id":"late-run","cardId":"task","automationId":flow.id,"repo":"synthetic/work","status":"starting"}),
        );
        assert!(runs::reserve(&store, &late).is_err());
        assert!(settle(&store, "task", &flow.id).is_err());
        let resumed = change(&store, "task", &body(&card(&store), &project, "resume")).unwrap();
        assert!(read(&resumed).unwrap().unwrap().run_id.is_none());
        claim(&store, "task", &flow.id, "interrupted-launch".into()).unwrap();
        drop(connection);
        let connection = super::super::store::open(&path).unwrap();
        let store = Store::new(&connection);
        recover(&store).unwrap();
        assert!(running(&store).unwrap().is_none());
        let interrupted = card(&store);
        assert!(
            interrupted
                .fields
                .string_equals("status", "standby")
                .unwrap()
        );
        assert!(read(&interrupted).unwrap().unwrap().state == State::Interrupted);
        assert!(
            store
                .documents::<Document>(Documents::Runs)
                .unwrap()
                .is_empty()
        );
        let mut changed = project.clone();
        changed.set("testCommand", &"changed").unwrap();
        store.put(Documents::Projects, &changed).unwrap();
        assert!(change(&store, "task", &body(&interrupted, &changed, "resume")).is_err());
        assert_eq!(
            serde_json::to_value(card(&store)).unwrap(),
            serde_json::to_value(interrupted).unwrap()
        );
        let replacement = change(&store, "task", &body(&card(&store), &changed, "start")).unwrap();
        let replacement = read(&replacement).unwrap().unwrap();
        assert_ne!(replacement.id, flow.id);
        assert!(replacement.step == Step::Development && replacement.state == State::Running);
    }

    #[test]
    fn successful_local_steps_wait_for_publication_and_only_two_blocking_corrections_are_allowed() {
        let home = tempfile::tempdir().unwrap();
        let connection = super::super::store::open(&home.path().join("synthetic.sqlite")).unwrap();
        let store = Store::new(&connection);
        let (initial, project) = seed(&store);
        let started = change(&store, "task", &body(&initial, &project, "start")).unwrap();
        let mut flow = read(&started).unwrap().unwrap();
        synthetic_stage_result(&store, &flow, "development", json!({"purpose":"work"}));
        settle(&store, "task", &flow.id).unwrap();
        flow = read(&card(&store)).unwrap().unwrap();
        assert!(flow.step == Step::Tests);
        synthetic_stage_result(
            &store,
            &flow,
            "tests",
            json!({"purpose":"verification","verificationState":"stale"}),
        );
        assert!(settle(&store, "task", &flow.id).is_err());
        let mut run = store
            .document::<Document>(Documents::Runs, "synthetic-tests")
            .unwrap()
            .unwrap();
        run.set("verificationState", &"passed").unwrap();
        store.put(Documents::Runs, &run).unwrap();
        settle(&store, "task", &flow.id).unwrap();
        assert!(read(&card(&store)).unwrap().unwrap().state == State::Waiting);
        assert!(change(&store, "task", &body(&card(&store), &project, "review")).is_err());
        let mut linked = card(&store);
        linked.fields.set("pullRequest",&json!({"revision":1,"kind":"github","repository":"synthetic/repo","number":1,"title":"Synthetic","url":"https://synthetic.example.test/pr/1","status":"open","draft":true,"sourceBranch":"task","targetBranch":"main","fork":false,"baseUrl":"https://synthetic.example.test","checkedAt":"synthetic"})).unwrap();
        store.put(Documents::Cards, &linked).unwrap();
        for cycle in 0..3 {
            let resumed = change(&store, "task", &body(&card(&store), &project, "review")).unwrap();
            flow = read(&resumed).unwrap().unwrap();
            assert!(flow.step == Step::Tests);
            synthetic_stage_result(
                &store,
                &flow,
                &format!("published-tests-{cycle}"),
                json!({"purpose":"verification","verificationState":"passed"}),
            );
            settle(&store, "task", &flow.id).unwrap();
            flow = read(&card(&store)).unwrap().unwrap();
            assert!(flow.step == Step::Review);
            synthetic_stage_result(
                &store,
                &flow,
                &format!("review-{cycle}"),
                json!({"purpose":"review","reviewValidatedAt":"synthetic","review":{"summary":"Synthetic fixture, not an executed agent","findings":[{"priority":"P1","title":"Synthetic blocker","file":"file.rs","line":1,"problem":"Synthetic problem","suggestion":"Synthetic correction"}],"criteria":[{"index":0,"status":"covered","evidence":"Synthetic state fixture only"}]}}),
            );
            settle(&store, "task", &flow.id).unwrap();
            flow = read(&card(&store)).unwrap().unwrap();
            if cycle < 2 {
                assert!(flow.step == Step::Correction && flow.cycles == cycle + 1);
                synthetic_stage_result(
                    &store,
                    &flow,
                    &format!("correction-{cycle}"),
                    json!({"purpose":"correction"}),
                );
                settle(&store, "task", &flow.id).unwrap();
                flow = read(&card(&store)).unwrap().unwrap();
                synthetic_stage_result(
                    &store,
                    &flow,
                    &format!("corrected-tests-{cycle}"),
                    json!({"purpose":"verification","verificationState":"passed"}),
                );
                settle(&store, "task", &flow.id).unwrap();
            } else {
                assert!(flow.state == State::Blocked && flow.cycles == 2);
                assert!(
                    card(&store)
                        .fields
                        .string_equals("status", "standby")
                        .unwrap()
                );
            }
        }
        assert_eq!(
            card(&store)
                .fields
                .get::<Vec<serde_json::Value>>("checklist")
                .unwrap()
                .unwrap()[0]["done"],
            false
        );
    }

    #[test]
    fn publication_freezes_the_preview_requires_separate_branch_consent_and_never_replays_after_recovery()
     {
        let home = tempfile::tempdir().unwrap();
        let connection = super::super::store::open(&home.path().join("synthetic.sqlite")).unwrap();
        let store = Store::new(&connection);
        store.save_settings(&doc(json!({"forgeRevision":1,"forgeConnections":{"github":{"kind":"github","base":"https://github-synthetic.example.test"}}}))).unwrap();
        let (initial, project) = seed(&store);
        let started = change(&store, "task", &body(&initial, &project, "start")).unwrap();
        let mut flow = read(&started).unwrap().unwrap();
        synthetic_stage_result(
            &store,
            &flow,
            "publication-development",
            json!({"purpose":"work"}),
        );
        settle(&store, "task", &flow.id).unwrap();
        flow = read(&card(&store)).unwrap().unwrap();
        synthetic_stage_result(
            &store,
            &flow,
            "publication-tests",
            json!({"purpose":"verification","verificationState":"passed"}),
        );
        settle(&store, "task", &flow.id).unwrap();
        assert!(change(&store, "task", &body(&card(&store), &project, "publish")).is_err());
        let mut ready = card(&store);
        let preview:atelier_core::contracts::ForgePublicationPreview=serde_json::from_str(&json!({"configuration":{"kind":"github","base":"https://github-synthetic.example.test"},"revision":1,"repository":"owner/repo","worktree":flow.worktree,"target":"main","origin":"https://github-synthetic.example.test/owner/repo.git","remoteSource":null,"localChanges":false,"sourceCommit":"a".repeat(40),"targetCommit":"b".repeat(40),"title":"Synthetic approved title","body":"Synthetic approved body","draft":true}).to_string()).unwrap();
        ready
            .fields
            .set("forgePublicationPreview", &preview)
            .unwrap();
        store.put(Documents::Cards, &ready).unwrap();
        assert!(change(&store, "task", &body(&ready, &project, "publish")).is_err());
        let mut input = body(&ready, &project, "publish");
        input.set("allowCreateBranch", &true).unwrap();
        let authorized = change(&store, "task", &input).unwrap();
        flow = read(&authorized).unwrap().unwrap();
        assert!(
            flow.state == State::Running
                && flow.step == Step::Tests
                && flow.publication_plan.is_some()
        );
        assert!(publication(&store, "task", &flow.id).is_err());
        synthetic_stage_result(
            &store,
            &flow,
            "publication-reverification",
            json!({"purpose":"verification","verificationState":"passed"}),
        );
        settle(&store, "task", &flow.id).unwrap();
        let (_, _, frozen) = publication(&store, "task", &flow.id).unwrap();
        assert_eq!(
            serde_json::to_value(frozen).unwrap(),
            serde_json::to_value(PublicationPlan::Create(Box::new(preview))).unwrap()
        );
        let original = card(&store);
        let mut changed = original.clone();
        changed.fields.set("pullRequest",&json!({"revision":1,"kind":"github","repository":"owner/repo","number":7,"title":"Synthetic approved title","url":"https://github-synthetic.example.test/owner/repo/pull/7","status":"active","draft":true,"sourceBranch":"task","targetBranch":"main","sourceCommit":"a".repeat(40),"targetCommit":"c".repeat(40),"fork":false,"baseUrl":"https://github-synthetic.example.test","checkedAt":"synthetic"})).unwrap();
        store.put(Documents::Cards, &changed).unwrap();
        assert!(publication_complete(&store, &changed, &flow.id).is_err());
        assert!(read(&card(&store)).unwrap().unwrap().step == Step::Publication);
        store.put(Documents::Cards, &original).unwrap();
        let before = store.documents::<Document>(Documents::Runs).unwrap().len();
        recover(&store).unwrap();
        assert!(publication(&store, "task", &flow.id).is_err());
        assert!(running(&store).unwrap().is_none());
        let resumed = change(&store, "task", &body(&card(&store), &project, "resume")).unwrap();
        let flow = read(&resumed).unwrap().unwrap();
        assert!(flow.step == Step::Tests && flow.publication_plan.is_none());
        assert_eq!(
            store.documents::<Document>(Documents::Runs).unwrap().len(),
            before
        );
        cancel(&store, "task", &flow.id).unwrap();
        assert!(publication(&store, "task", &flow.id).is_err());
        assert!(
            store
                .document::<Document>(Documents::Cards, "task")
                .unwrap()
                .unwrap()
                .raw("gitPush")
                .is_none()
        );
        assert!(
            store
                .document::<Document>(Documents::Cards, "task")
                .unwrap()
                .unwrap()
                .raw("forgePublication")
                .is_none()
        );
    }

    #[test]
    fn workflow_commit_keeps_the_approved_snapshot_and_never_treats_commit_as_verification() {
        let home = tempfile::tempdir().unwrap();
        let connection = super::super::store::open(&home.path().join("synthetic.sqlite")).unwrap();
        let store = Store::new(&connection);
        let (initial, project) = seed(&store);
        let started = change(&store, "task", &body(&initial, &project, "start")).unwrap();
        let mut flow = read(&started).unwrap().unwrap();
        synthetic_stage_result(
            &store,
            &flow,
            "commit-development",
            json!({"purpose":"work"}),
        );
        settle(&store, "task", &flow.id).unwrap();
        flow = read(&card(&store)).unwrap().unwrap();
        synthetic_stage_result(
            &store,
            &flow,
            "commit-prior-tests",
            json!({"purpose":"verification","verificationState":"passed"}),
        );
        settle(&store, "task", &flow.id).unwrap();
        let mut ready = card(&store);
        assert!(change(&store, "task", &body(&ready, &project, "commit")).is_err());
        let preview = atelier_core::contracts::LocalCommitPreview {
            worktree: flow.worktree.clone(),
            snapshot: atelier_core::contracts::LocalCommitSnapshot {
                branch: "task".into(),
                head: "a".repeat(40),
                tree: "b".repeat(40),
                paths: vec!["file.txt".into()],
            },
            author_name: "Synthetic author".into(),
            author_email: "synthetic@example.invalid".into(),
            message: "Synthetic immutable commit".into(),
        };
        ready.fields.set("localCommitPreview", &preview).unwrap();
        store.put(Documents::Cards, &ready).unwrap();
        let authorized = change(&store, "task", &body(&ready, &project, "commit")).unwrap();
        let (_, frozen) = commit(&store, "task", &flow.id).unwrap();
        assert_eq!(
            serde_json::to_value(frozen).unwrap(),
            serde_json::to_value(&preview).unwrap()
        );
        assert!(read(&authorized).unwrap().unwrap().step == Step::Commit);
        assert!(commit_complete(&store, &authorized, &flow.id).is_err());
        let mut observed = authorized;
        let mut effect = atelier_core::contracts::LocalCommitIntent {
            id: "synthetic-effect".into(),
            state: atelier_core::contracts::ForgeWriteState::Confirmed,
            preview: preview.clone(),
            commit: Some("c".repeat(40)),
            observed: Some("d".repeat(40)),
        };
        observed.fields.set("localCommit", &effect).unwrap();
        store.put(Documents::Cards, &observed).unwrap();
        assert!(commit_complete(&store, &observed, &flow.id).is_err());
        effect.observed = effect.commit.clone();
        observed.fields.set("localCommit", &effect).unwrap();
        store.put(Documents::Cards, &observed).unwrap();
        commit_complete(&store, &observed, &flow.id).unwrap();
        let finished = card(&store);
        let flow = read(&finished).unwrap().unwrap();
        assert!(
            flow.state == State::Running
                && flow.step == Step::Tests
                && flow.commit_plan.is_none()
                && flow.verification_id.is_none()
        );
        assert_eq!(finished.title, initial.title);
        assert_eq!(
            finished
                .fields
                .get::<Vec<serde_json::Value>>("checklist")
                .unwrap()
                .unwrap()[0]["done"],
            false
        );
        recover(&store).unwrap();
        let resumed = change(&store, "task", &body(&card(&store), &project, "resume")).unwrap();
        let flow = read(&resumed).unwrap().unwrap();
        assert!(flow.step == Step::Tests && flow.commit_plan.is_none());
        cancel(&store, "task", &flow.id).unwrap();
        assert!(commit(&store, "task", &flow.id).is_err());
    }

    #[test]
    fn workflow_board_statuses_never_accept_criteria_or_overwrite_a_newer_human_status() {
        let home = tempfile::tempdir().unwrap();
        let connection = super::super::store::open(&home.path().join("synthetic.sqlite")).unwrap();
        let store = Store::new(&connection);
        let (initial, project) = seed(&store);
        let mut started = change(&store, "task", &body(&initial, &project, "start")).unwrap();
        let mut flow = read(&started).unwrap().unwrap();
        // Validated-result fixture only; no real review or native proof is represented.
        flow.step = Step::Review;
        started.fields.set("automation", &flow).unwrap();
        store.put(Documents::Cards, &started).unwrap();
        synthetic_stage_result(
            &store,
            &flow,
            "ready-board-status",
            json!({"purpose":"review","reviewValidatedAt":"synthetic","review":{"summary":"Synthetic state fixture only","findings":[],"criteria":[{"index":0,"status":"covered","evidence":"Synthetic state fixture only"}]}}),
        );
        settle(&store, "task", &flow.id).unwrap();
        let mut ready = card(&store);
        assert!(read(&ready).unwrap().unwrap().state == State::Ready);
        assert!(ready.fields.string_equals("status", "review").unwrap());
        assert_eq!(
            ready
                .fields
                .get::<Vec<serde_json::Value>>("checklist")
                .unwrap()
                .unwrap()[0]["done"],
            false
        );
        ready.fields.set("status", &"done").unwrap();
        store.put(Documents::Cards, &ready).unwrap();
        let restarted = change(&store, "task", &body(&ready, &project, "start")).unwrap();
        assert!(restarted.fields.string_equals("status", "doing").unwrap());
        let flow = read(&restarted).unwrap().unwrap();
        let mut human = restarted;
        human.fields.set("status", &"done").unwrap();
        store.put(Documents::Cards, &human).unwrap();
        cancel(&store, "task", &flow.id).unwrap();
        assert!(card(&store).fields.string_equals("status", "done").unwrap());
        assert_eq!(card(&store).title, initial.title);
    }
}
