use super::super::Shared;
use crate::{
    Result,
    infrastructure::{
        connector_repositories::{self as repositories, Guard, Selection},
        connectors,
        extensions::Executable,
        store::Documents,
    },
};
use atelier_core::{
    application::extensions::{
        self as protocol, Installed, Invocation, Method, ModulePullRequestLink, Reply,
        RepositoryReviewContext, Request, Status,
    },
    contracts::Card,
};
use serde_json::json;
use std::{path::PathBuf, time::Duration};

pub async fn prepare_read(
    state: &Shared,
    profile: &Installed,
    request: &mut Request,
) -> Result<Option<Guard>> {
    if !matches!(
        request.method,
        Method::RepositoriesPreview | Method::RepositoriesRefresh | Method::RepositoriesReview
    ) {
        return Ok(None);
    }
    idle(state)?;
    let selection: Selection = serde_json::from_str(&request.input.to_string())?;
    let guard = Guard::before(
        &state.prototype_directory,
        &state.prototype_paths,
        profile.clone(),
        selection,
    )
    .await?;
    request.input = guard.input()?;
    Ok(Some(guard))
}
async fn dispatch(profile: &Installed, request: Request) -> Result<Reply> {
    let executable = Executable::open(
        PathBuf::from(&profile.binary.path),
        Some(profile.binary.sha256.clone()),
    )
    .await?;
    let reply = executable
        .prepare(&profile.manifest, request, Duration::from_secs(30))?
        .dispatch()
        .await?;
    if reply.status != Status::Ok {
        return Err("message.the_module_did_not_provide_a_usable_repository_observation".into());
    }
    Ok(reply)
}
pub async fn prepare_review(state: &Shared, card: &Card) -> Result<RepositoryReviewContext> {
    let linked = card
        .fields
        .get::<ModulePullRequestLink>("connectorPullRequest")?
        .ok_or("message.no_module_pr_mr_is_associated")?;
    let expected = card.clone();
    let module = linked.module.clone();
    let revision = linked.revision;
    let vault = state.vault.clone();
    let input = Invocation {
        expected_revision: revision,
        method: Method::RepositoriesReview,
        action: None,
        input: json!({"expected":expected,"target":linked.pull_request.target_branch,"title":card.title.utf8()?,"body":card.fields.get::<String>("description")?.unwrap_or_default(),"draft":linked.pull_request.draft}),
        confirmed: true,
    };
    let (profile, mut request) = state
        .database
        .call(move |store| {
            connectors::invocation(store, &module, &input, &mut |value, decrypt| {
                vault.protect(value, decrypt)
            })
        })
        .await?;
    let guard = prepare_read(state, &profile, &mut request)
        .await?
        .ok_or("message.the_module_preparation_is_missing")?;
    let reply = dispatch(&profile, request).await?;
    let proposal = guard.observe(&reply.output, true).await?;
    let context = proposal.context.clone();
    let expected = card.clone();
    let selected = context.clone();
    state
        .database
        .call(move |store| repositories::review_matches(store, &expected, &selected))
        .await?;
    Ok(context)
}
pub async fn remote_guard(
    state: &Shared,
    profile: &Installed,
    request: &Request,
    original: &protocol::RemoteOperation,
    reconcile: bool,
) -> Result<Option<Guard>> {
    if original.method != Method::RepositoriesPublish {
        return Ok(None);
    }
    idle(state)?;
    let proposal = protocol::repository_proposal(&original.input)?;
    let id = proposal.context.card_id.clone();
    let expected = state
        .database
        .call(move |store| {
            store
                .document::<Card>(Documents::Cards, &id)?
                .ok_or_else(|| "message.card_missing".into())
        })
        .await?;
    let selection = Selection {
        expected,
        target: proposal.observation.target_branch.clone(),
        title: proposal.title.clone(),
        body: proposal.body.clone(),
        draft: proposal.draft,
    };
    let guard = Guard::before(
        &state.prototype_directory,
        &state.prototype_paths,
        profile.clone(),
        selection,
    )
    .await?;
    if !reconcile {
        let mut read = request.clone();
        read.method = Method::RepositoriesPreview;
        read.action = None;
        read.operation_id = None;
        read.request_id = format!("request-{}", uuid::Uuid::new_v4());
        read.input = guard.input()?;
        let reply = dispatch(profile, read).await?;
        let fresh = guard.observe(&reply.output, false).await?;
        if fresh.card_revision != proposal.card_revision
            || fresh.context != proposal.context
            || fresh.observation != proposal.observation
        {
            return Err(
                "message.the_module_sources_or_destination_changed_read_the_publication_p".into(),
            );
        }
    }
    Ok(Some(guard))
}
fn idle(state: &Shared) -> Result<()> {
    if !state.runs.active_cards().is_empty()
        || !state.services.active_cards().is_empty()
        || !state.services.active_projects().is_empty()
    {
        return Err(
            "message.stop_agents_and_services_before_this_module_repository_operation".into(),
        );
    }
    Ok(())
}
pub fn guard_invocation(state: &Shared, method: Method) -> Result<()> {
    if matches!(
        method,
        Method::RepositoriesPreview | Method::RepositoriesRefresh | Method::RepositoriesReview
    ) {
        idle(state)?;
    }
    Ok(())
}
pub async fn validate_review(
    state: &Shared,
    card: &Card,
    id: &str,
) -> Result<atelier_core::document::Document> {
    let selected = id.to_owned();
    let expected = card.clone();
    let recorded = state
        .database
        .call(move |store| {
            let mut run = store
                .document::<atelier_core::document::Document>(Documents::Runs, &selected)?
                .ok_or("message.review_missing")?;
            let context = run
                .get::<RepositoryReviewContext>("connectorReviewContext")?
                .ok_or("message.connector_context_missing")?;
            if !run.string_equals("cardId", &expected.id)?
                || !run.string_equals("kind", "pr-review")?
                || !run.string_equals("status", "completed")?
                || run
                    .get::<atelier_core::application::reviews::Review>("review")?
                    .is_none()
            {
                return Err("message.the_module_review_is_incomplete".into());
            }
            run.set(
                "reviewStaleReason",
                &crate::infrastructure::reviews::UNCHECKED,
            )?;
            store.put(Documents::Runs, &run)?;
            repositories::review_matches(store, &expected, &context)?;
            Ok(context)
        })
        .await?;
    let current = prepare_review(state, card).await?;
    if recorded != current {
        return Err(
            "message.the_module_commits_criteria_or_pr_mr_changed_run_the_review_agai".into(),
        );
    }
    let expected = card.clone();
    let selected = id.to_owned();
    let timestamp =
        time::OffsetDateTime::now_utc().format(&time::format_description::well_known::Rfc3339)?;
    state
        .database
        .call(move |store| {
            repositories::validate_review(store, &expected, &selected, &current, &timestamp)
        })
        .await
}
