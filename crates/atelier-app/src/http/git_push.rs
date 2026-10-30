use super::{Shared, failure, responses::response};
use crate::infrastructure::{
    forge, forge_association, git_push as workflow, git_transport::HttpsAuthentication,
    store::Documents,
};
use atelier_core::contracts::{
    Card, ForgePublicationRequest, ForgeWriteState, GitPushIntent, GitPushRequest, PullRequest,
};
use axum::{
    extract::{Path, Request, State},
    http::StatusCode,
    response::Response,
};

pub(super) async fn preview(
    State(state): State<Shared>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    let input: GitPushRequest = match super::input::parse(request).await {
        Ok(input) => input,
        Err(response) => return *response,
    };
    if id != input.expected.id {
        return failure(StatusCode::BAD_REQUEST, "message.inconsistent_push_card");
    }
    let Ok(_permit) = state.integration_reads.try_acquire() else {
        return failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "message.remote_searches_are_busy_try_again_shortly",
        );
    };
    let mut stopped = state.shutdown.subscribe();
    let operation = async {
        let vault = state.vault.clone();
        let expected = input.expected.clone();
        let revision = input.revision;
        let (card, configuration, token) = state
            .database
            .call(move |store| {
                let card = forge_association::snapshot(store, &expected)?;
                let linked = card
                    .fields
                    .get::<PullRequest>("pullRequest")?
                    .ok_or("message.link_a_pr_mr_before_preparing_the_push")?;
                if linked.revision != revision {
                    return Err(
                        "message.read_the_pr_mr_link_again_after_changing_the_connection".into(),
                    );
                }
                let (configuration, token) =
                    forge::credentials(store, linked.kind, revision, &mut |value, decrypt| {
                        vault.protect(value, decrypt)
                    })?;
                Ok((card, configuration, token))
            })
            .await?;
        let preview = workflow::preview(
            &state.prototype_directory,
            &state.prototype_paths,
            &card,
            configuration,
            token,
        )
        .await?;
        state
            .database
            .call(move |store| workflow::save_preview(store, &card, input.revision, preview))
            .await
    };
    let stopping = *stopped.borrow();
    if stopping {
        return failure(StatusCode::CONFLICT, "message.atelier_is_shutting_down");
    }
    tokio::select! {result=tokio::time::timeout(std::time::Duration::from_secs(120),operation)=>response(result.unwrap_or_else(|_|Err("message.push_preview_timed_out".into()))),_=stopped.changed()=>failure(StatusCode::CONFLICT,"message.atelier_is_shutting_down")}
}
pub(super) async fn push(
    State(state): State<Shared>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    request_input(state, id, request, false).await
}
pub(super) async fn reconcile(
    State(state): State<Shared>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    request_input(state, id, request, true).await
}
pub(super) async fn publication_branch(
    State(state): State<Shared>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    let publication: ForgePublicationRequest = match super::input::parse(request).await {
        Ok(input) => input,
        Err(response) => return *response,
    };
    let input = GitPushRequest {
        expected: publication.expected.clone(),
        revision: publication.revision,
        confirmed: publication.confirmed,
    };
    execute(state, id, input, false, Some(publication)).await
}
async fn request_input(state: Shared, id: String, request: Request, reconcile: bool) -> Response {
    let input: GitPushRequest = match super::input::parse(request).await {
        Ok(input) => input,
        Err(response) => return *response,
    };
    execute(state, id, input, reconcile, None).await
}
async fn execute(
    state: Shared,
    id: String,
    input: GitPushRequest,
    reconcile: bool,
    publication: Option<ForgePublicationRequest>,
) -> Response {
    if id != input.expected.id || !input.confirmed {
        return failure(
            StatusCode::BAD_REQUEST,
            "message.confirm_the_commit_and_branch_to_push",
        );
    }
    let Ok(_permit) = state.integration_reads.try_acquire() else {
        return failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "message.remote_searches_are_busy_try_again_shortly",
        );
    };
    response(perform(&state, id, input, reconcile, publication).await)
}

pub(super) async fn perform(
    state: &Shared,
    id: String,
    input: GitPushRequest,
    reconcile: bool,
    publication: Option<ForgePublicationRequest>,
) -> crate::Result<Card> {
    if id != input.expected.id || !input.confirmed {
        return Err("message.confirm_the_commit_and_branch_to_push".into());
    }
    let requested = input.clone();
    let operation = uuid::Uuid::new_v4().to_string();
    let card = state
        .database
        .call(move |store| {
            if let Some(publication) = publication {
                return crate::infrastructure::forge_publication::reserve_branch(
                    store,
                    &publication,
                    &operation,
                );
            }
            if !reconcile {
                return workflow::reserve(store, &requested, &operation);
            }
            let card = store
                .document::<Card>(Documents::Cards, &requested.expected.id)?
                .ok_or("message.card_not_found")?;
            if serde_json::to_value(atelier_core::application::jira::card_snapshot(&card))?
                != serde_json::to_value(atelier_core::application::jira::card_snapshot(
                    &requested.expected,
                ))?
            {
                return Err("message.the_task_changed_reload_before_checking_the_push".into());
            }
            let intent = card
                .fields
                .get::<GitPushIntent>("gitPush")?
                .filter(|intent| intent.state == ForgeWriteState::Uncertain)
                .ok_or("message.no_uncertain_push_to_check")?;
            let configuration =
                forge::configuration(store, intent.preview.configuration.kind, requested.revision)?;
            if configuration.base != intent.preview.configuration.base {
                return Err("message.reconnect_the_forge_site_of_the_uncertain_push".into());
            }
            Ok(card)
        })
        .await?;
    let intent = card
        .fields
        .get::<GitPushIntent>("gitPush")?
        .ok_or("message.push_intent_missing")?;
    let _ = state.changed.send(());
    let mut stopped = state.shutdown.subscribe();
    let work = async {
        let vault = state.vault.clone();
        let revision = input.revision;
        let kind = intent.preview.configuration.kind;
        let (configuration, token) = state
            .database
            .call(move |store| {
                forge::credentials(store, kind, revision, &mut |value, decrypt| {
                    vault.protect(value, decrypt)
                })
            })
            .await?;
        if !reconcile && let Some(linked) = intent.preview.pull_request.as_ref() {
            let remote = forge::Forge::new(configuration.clone(), token.clone(), revision)?
                .pull_request(&intent.preview.repository, linked.number)
                .await?;
            workflow::require_pull_request(&intent.preview, &remote)?;
        }
        let authentication = HttpsAuthentication::new(
            configuration.clone(),
            intent.preview.repository.clone(),
            token,
        )?;
        if !reconcile {
            let prepared = workflow::prepare(
                &state.prototype_directory,
                &state.prototype_paths,
                &card,
                &configuration,
                &authentication,
            )
            .await?;
            let expected = card.clone();
            let operation = intent.id.clone();
            state
                .database
                .call(move |store| workflow::sending(store, &expected, &operation))
                .await?;
            let _ = state.changed.send(());
            authentication.push(prepared).await?;
        }
        let observed = authentication
            .remote_head(
                &intent.preview.origin,
                &format!("refs/heads/{}", intent.preview.worktree.branch),
            )
            .await?;
        let card_id = id.clone();
        let operation = intent.id.clone();
        state
            .database
            .call(move |store| {
                workflow::record(
                    store,
                    &card_id,
                    &operation,
                    observed,
                    input.revision,
                    reconcile,
                )
            })
            .await
    };
    let stopping = *stopped.borrow();
    let result = if stopping {
        drop(work);
        Err("message.atelier_is_shutting_down".into())
    } else {
        tokio::select! {result=tokio::time::timeout(std::time::Duration::from_secs(300),work)=>result.unwrap_or_else(|_|Err("message.push_interrupted_or_timed_out_read_the_remote_branch".into())),_=stopped.changed()=>Err("message.atelier_is_shutting_down".into())}
    };
    let result = match result {
        Err(error) if !reconcile => {
            let operation = intent.id;
            match state
                .database
                .call(move |store| workflow::interrupted(store, &id, &operation))
                .await
            {
                Ok(_) => Err(error),
                Err(recording) => Err(recording),
            }
        }
        result => result,
    };
    let _ = state.changed.send(());
    result
}
