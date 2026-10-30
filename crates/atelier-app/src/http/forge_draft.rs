use super::{Shared, failure, responses::response};
use crate::infrastructure::{forge, forge_association, forge_draft as workflow};
use atelier_core::contracts::{Card, ForgeDraftIntent, ForgeDraftRequest, ForgeWriteState};
use axum::{
    extract::{Path, Request, State},
    http::StatusCode,
    response::Response,
};

pub(super) async fn change(
    State(state): State<Shared>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    execute(state, id, request, false).await
}
pub(super) async fn reconcile(
    State(state): State<Shared>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    execute(state, id, request, true).await
}
async fn execute(state: Shared, id: String, request: Request, reconcile: bool) -> Response {
    let input: ForgeDraftRequest = match super::input::parse(request).await {
        Ok(input) => input,
        Err(response) => return *response,
    };
    if input.expected.id != id || !input.confirmed {
        return failure(
            StatusCode::BAD_REQUEST,
            "message.confirm_changing_the_pr_mr_draft_state",
        );
    }
    let Ok(_permit) = state.integration_reads.try_acquire() else {
        return failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "message.remote_searches_are_busy_try_again_shortly",
        );
    };
    let prepared = input.clone();
    let operation = uuid::Uuid::new_v4().to_string();
    let reserved = operation.clone();
    let card = if reconcile {
        state
            .database
            .call(move |store| {
                let current = store
                    .document::<Card>(
                        crate::infrastructure::store::Documents::Cards,
                        &prepared.expected.id,
                    )?
                    .ok_or("message.card_not_found")?;
                if serde_json::to_value(atelier_core::application::jira::card_snapshot(&current))?
                    != serde_json::to_value(atelier_core::application::jira::card_snapshot(
                        &prepared.expected,
                    ))?
                {
                    return Err("message.the_task_changed_reload_before_checking_the_forge".into());
                }
                let intent = current
                    .fields
                    .get::<ForgeDraftIntent>("forgeDraft")?
                    .filter(|intent| {
                        intent.state == ForgeWriteState::Uncertain
                            && intent.draft == prepared.draft
                            && intent.content == prepared.content
                    })
                    .ok_or("message.no_uncertain_forge_write_to_check")?;
                let configuration =
                    forge::configuration(store, intent.expected.kind, prepared.revision)?;
                if configuration.base != intent.expected.base_url {
                    return Err(
                        "message.reconnect_the_forge_site_of_the_uncertain_operation".into(),
                    );
                }
                Ok(current)
            })
            .await
    } else {
        state
            .database
            .call(move |store| workflow::reserve(store, &prepared, &reserved))
            .await
    };
    let card = match card {
        Ok(card) => card,
        Err(error) => return response::<Card>(Err(error)),
    };
    let intent = match card.fields.get::<ForgeDraftIntent>("forgeDraft") {
        Ok(Some(intent)) => intent,
        _ => {
            return failure(
                StatusCode::INTERNAL_SERVER_ERROR,
                "message.forge_intent_missing",
            );
        }
    };
    let _ = state.changed.send(());
    let mut stopped = state.shutdown.subscribe();
    let work = async {
        let vault = state.vault.clone();
        let kind = intent.expected.kind;
        let revision = input.revision;
        let (configuration, token) = state
            .database
            .call(move |store| {
                forge::credentials(store, kind, revision, &mut |value, decrypt| {
                    vault.protect(value, decrypt)
                })
            })
            .await?;
        // Reconciliation is read-only and remains possible if the worktree was removed externally.
        if !reconcile {
            let _directory = forge_association::verify(
                &state.prototype_directory,
                &state.prototype_paths,
                &card,
                &configuration,
                &intent.expected.repository,
            )
            .await?;
        }
        let client = forge::Forge::new(configuration, token, revision)?;
        let observed = if reconcile {
            client
                .pull_request(&intent.expected.repository, intent.expected.number)
                .await?
        } else {
            let prepared = if let Some(content) = &intent.content {
                client
                    .prepare_content(&intent.expected, content, &intent.id)
                    .await?
            } else {
                client
                    .prepare_draft(&intent.expected, intent.draft, &intent.id)
                    .await?
            };
            let expected = card.clone();
            let operation = intent.id.clone();
            state
                .database
                .call(move |store| workflow::sending(store, &expected, &operation))
                .await?;
            let _ = state.changed.send(());
            client.send_draft(prepared).await?
        };
        let card_id = id.clone();
        let operation = intent.id.clone();
        state
            .database
            .call(move |store| workflow::record(store, &card_id, &operation, observed, reconcile))
            .await
    };
    let stopping = *stopped.borrow();
    let result = if stopping {
        drop(work);
        Err("message.atelier_is_shutting_down".into())
    } else {
        tokio::select! {result=tokio::time::timeout(std::time::Duration::from_secs(120),work)=>result.unwrap_or_else(|_|Err("message.forge_write_timed_out_read_the_remote_state_without_resending".into())),_=stopped.changed()=>Err("message.atelier_is_shutting_down".into())}
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
    response(result)
}
