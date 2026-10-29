use super::{Shared, failure, responses::response};
use crate::infrastructure::{
    forge, forge_association, forge_publication as workflow, git_transport::HttpsAuthentication,
    store::Documents,
};
use atelier_core::contracts::{
    Card, ForgePublicationIntent, ForgePublicationRequest, ForgeWriteState,
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
    let input: ForgePublicationRequest = match super::jira_transitions::input(request).await {
        Ok(input) => input,
        Err(response) => return *response,
    };
    if input.expected.id != id {
        return failure(
            StatusCode::BAD_REQUEST,
            "message.inconsistent_publication_card",
        );
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
        let kind = input.kind;
        let revision = input.revision;
        let (configuration, token) = state
            .database
            .call(move |store| {
                forge_association::snapshot(store, &expected)?;
                forge::credentials(store, kind, revision, &mut |value, decrypt| {
                    vault.protect(value, decrypt)
                })
            })
            .await?;
        let preview = workflow::preview(
            &state.prototype_directory,
            &state.prototype_paths,
            &input,
            configuration,
            token,
        )
        .await?;
        state
            .database
            .call(move |store| workflow::save_preview(store, &input.expected, preview))
            .await
    };
    let stopping = *stopped.borrow();
    if stopping {
        return failure(StatusCode::CONFLICT, "message.atelier_is_shutting_down");
    }
    tokio::select! {result=tokio::time::timeout(std::time::Duration::from_secs(180),operation)=>response(result.unwrap_or_else(|_|Err("message.publication_preview_timed_out".into()))),_=stopped.changed()=>failure(StatusCode::CONFLICT,"message.atelier_is_shutting_down")}
}
pub(super) async fn create(
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
    let input: ForgePublicationRequest = match super::jira_transitions::input(request).await {
        Ok(input) => input,
        Err(response) => return *response,
    };
    if input.expected.id != id || !input.confirmed {
        return failure(
            StatusCode::BAD_REQUEST,
            "message.confirm_the_branches_and_content_of_the_pr_mr_to_create",
        );
    }
    let Ok(_permit) = state.integration_reads.try_acquire() else {
        return failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "message.remote_searches_are_busy_try_again_shortly",
        );
    };
    response(perform(&state, id, input, reconcile).await)
}

pub(super) async fn perform(
    state: &Shared,
    id: String,
    input: ForgePublicationRequest,
    reconcile: bool,
) -> crate::Result<Card> {
    if id != input.expected.id || !input.confirmed {
        return Err("message.confirm_the_branches_and_content_of_the_pr_mr_to_create".into());
    }
    let prepared = input.clone();
    let operation = uuid::Uuid::new_v4().to_string();
    let card = state
        .database
        .call(move |store| {
            if !reconcile {
                return workflow::reserve(store, &prepared, &operation);
            }
            let card = store
                .document::<Card>(Documents::Cards, &prepared.expected.id)?
                .ok_or("message.card_not_found")?;
            if serde_json::to_value(atelier_core::application::jira::card_snapshot(&card))?
                != serde_json::to_value(atelier_core::application::jira::card_snapshot(
                    &prepared.expected,
                ))?
            {
                return Err(
                    "message.the_task_changed_reload_before_checking_the_publication".into(),
                );
            }
            let intent = card
                .fields
                .get::<ForgePublicationIntent>("forgePublication")?
                .filter(|intent| intent.state == ForgeWriteState::Uncertain)
                .ok_or("message.no_uncertain_publication_to_check")?;
            let configuration =
                forge::configuration(store, intent.preview.configuration.kind, prepared.revision)?;
            if configuration.base != intent.preview.configuration.base {
                return Err("message.reconnect_the_forge_site_of_the_uncertain_publication".into());
            }
            Ok(card)
        })
        .await?;
    let intent = card
        .fields
        .get::<ForgePublicationIntent>("forgePublication")?
        .ok_or("message.publication_intent_missing")?;
    let _ = state.changed.send(());
    let mut stopped = state.shutdown.subscribe();
    let work = async {
        let vault = state.vault.clone();
        let kind = intent.preview.configuration.kind;
        let revision = input.revision;
        let (configuration, token) = state
            .database
            .call(move |store| {
                forge::credentials(store, kind, revision, &mut |value, decrypt| {
                    vault.protect(value, decrypt)
                })
            })
            .await?;
        let client = forge::Forge::new(configuration.clone(), token.clone(), revision)?;
        let mut known = intent.number;
        if !reconcile {
            let authentication = HttpsAuthentication::new(
                configuration.clone(),
                intent.preview.repository.clone(),
                token,
            )?;
            workflow::prepare(
                &state.prototype_directory,
                &state.prototype_paths,
                &card,
                &configuration,
                &authentication,
            )
            .await?;
            let prepared = client
                .prepare_publication(&intent.preview, &intent.id)
                .await?;
            let expected = card.clone();
            let operation = intent.id.clone();
            state
                .database
                .call(move |store| workflow::sending(store, &expected, &operation))
                .await?;
            let _ = state.changed.send(());
            let number = client.create_publication(prepared).await?;
            let card_id = id.clone();
            let operation = intent.id.clone();
            state
                .database
                .call(move |store| workflow::remember_number(store, &card_id, &operation, number))
                .await?;
            known = Some(number);
        }
        let found = client
            .reconcile_publication(&intent.preview, &intent.id, known)
            .await?;
        let card_id = id.clone();
        let operation = intent.id.clone();
        match found {
            Some(pr) => {
                state
                    .database
                    .call(move |store| workflow::record(store, &card_id, &operation, pr))
                    .await
            }
            None => Err(
                "message.no_pr_mr_with_the_marker_found_publication_remains_uncertain_no".into(),
            ),
        }
    };
    let stopping = *stopped.borrow();
    let result = if stopping {
        drop(work);
        Err("message.atelier_is_shutting_down".into())
    } else {
        tokio::select! {result=tokio::time::timeout(std::time::Duration::from_secs(300),work)=>result.unwrap_or_else(|_|Err("message.publication_interrupted_or_timed_out_read_the_forge_without_rese".into())),_=stopped.changed()=>Err("message.atelier_is_shutting_down".into())}
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
