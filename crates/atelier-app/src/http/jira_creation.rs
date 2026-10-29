use super::{
    Shared, failure,
    jira_transitions::{client, input},
    responses::response,
};
use crate::{Result, infrastructure::jira_creation as workflow};
use atelier_core::{
    application::jira as domain,
    contracts::{
        Card, JiraConfiguration, JiraCreateRequest, JiraCreationIntent, JiraCreationResolve,
        JiraWriteState as Phase,
    },
    document::Document,
};
use axum::{
    extract::{Path, Request, State},
    http::StatusCode,
    response::Response,
};
use std::time::Duration;
pub(super) async fn create(
    State(state): State<Shared>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    let input: JiraCreateRequest = match input(request).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    if input.expected.id != id {
        return failure(StatusCode::BAD_REQUEST, "message.inconsistent_jira_card");
    }
    let Ok(_permit) = state.integration_reads.try_acquire() else {
        return failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "message.remote_searches_are_busy_try_again_shortly",
        );
    };
    let operation = uuid::Uuid::new_v4().to_string();
    let reserved = operation.clone();
    let prepared = input.clone();
    if let Err(error) = state
        .database
        .call(move |store| workflow::reserve(store, &prepared, &reserved))
        .await
    {
        return response::<Card>(Err(error));
    }
    let _ = state.changed.send(());
    let mut stopped = state.shutdown.subscribe();
    let ready = {
        let preparing = async {
            let client = client(&state, input.revision).await?;
            let metadata = client
                .creation_metadata(&input.project_key, &input.issue_type.id)
                .await?;
            if metadata.issue_type != input.issue_type {
                return Err("message.the_jira_issue_type_changed_reload_the_metadata".into());
            }
            domain::validate_creation(&metadata, &input.expected)?;
            let original = input.expected.clone();
            let reserved = operation.clone();
            let card = state
                .database
                .call(move |store| workflow::sending(store, &original, &reserved))
                .await?;
            let _ = state.changed.send(());
            Ok::<_, crate::Error>((client, metadata, card))
        };
        let stopping = *stopped.borrow();
        if stopping {
            Err("message.atelier_is_shutting_down".into())
        } else {
            tokio::select! {result=tokio::time::timeout(Duration::from_secs(60),preparing)=>result.unwrap_or_else(|_|Err("message.reading_jira_metadata_took_too_long".into())),_=stopped.changed()=>Err("message.atelier_is_shutting_down".into())}
        }
    };
    let (client, metadata, card) = match ready {
        Ok(ready) => ready,
        Err(error) => {
            let result = state
                .database
                .call(move |store| workflow::failed_or_uncertain(store, &id, &operation))
                .await;
            let _ = state.changed.send(());
            return match result {
                Ok(_) => response::<Card>(Err(error)),
                Err(error) => response::<Card>(Err(error)),
            };
        }
    };
    let stopping = *stopped.borrow();
    let result = if stopping {
        None
    } else {
        tokio::select! {result=tokio::time::timeout(Duration::from_secs(60),client.create_issue(&metadata,&card))=>result.ok().and_then(std::result::Result::ok),_=stopped.changed()=>None}
    };
    let result = state
        .database
        .call(move |store| match result {
            Some(issue) => workflow::record(store, &id, &operation, issue),
            None => workflow::failed_or_uncertain(store, &id, &operation),
        })
        .await;
    let _ = state.changed.send(());
    response(result)
}
pub(super) async fn reconcile(
    State(state): State<Shared>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    let input: JiraCreationResolve = match input(request).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    if input.expected.id != id {
        return failure(StatusCode::BAD_REQUEST, "message.inconsistent_jira_card");
    }
    if input.close_uncertainty && input.existing_key.is_some() {
        return failure(
            StatusCode::BAD_REQUEST,
            "message.choose_issue_verification_or_closing_uncertainty",
        );
    }
    if input.close_uncertainty {
        return response(
            state
                .database
                .call(move |store| {
                    workflow::close_uncertainty(store, &input.expected, input.revision)
                })
                .await,
        );
    }
    let Ok(_permit) = state.integration_reads.try_acquire() else {
        return failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "message.remote_searches_are_busy_try_again_shortly",
        );
    };
    let mut stopped = state.shutdown.subscribe();
    if *stopped.borrow() {
        return failure(StatusCode::CONFLICT, "message.atelier_is_shutting_down");
    }
    let operation = async {
        let expected = input.expected.clone();
        let (card, intent) = state
            .database
            .call(move |store| {
                let card = workflow::revalidate(store, &expected, input.revision)?;
                let intent = card
                    .fields
                    .get::<JiraCreationIntent>("jiraCreation")?
                    .filter(|intent| intent.state == Phase::Uncertain)
                    .ok_or("message.no_uncertain_jira_creation_to_verify")?;
                let configuration = store
                    .settings::<Document>()?
                    .get::<JiraConfiguration>("jiraConnection")?
                    .ok_or("message.jira_connection_missing")?;
                if configuration.url != intent.base {
                    return Err("message.reconnect_the_jira_site_of_the_uncertain_creation".into());
                }
                Ok((card, intent))
            })
            .await?;
        let client = client(&state, input.revision).await?;
        let issue = if let Some(key) = input.existing_key {
            if intent.result.is_none()
                && key.rsplit_once('-').map(|(project, _)| project)
                    != Some(intent.project_key.as_str())
            {
                return Err(
                    "message.the_jira_key_must_belong_to_the_project_of_the_uncertain_creatio"
                        .into(),
                );
            }
            client.issue_by_key(&key).await?
        } else if let Some(known) = &intent.result {
            client.issue_by_id(&known.id).await?
        } else {
            return Err("message.enter_the_key_of_the_issue_found_in_jira".into());
        };
        state
            .database
            .call(move |store| {
                workflow::revalidate(store, &card, input.revision)?;
                workflow::record(store, &card.id, &intent.id, issue)
            })
            .await
    };
    let result: Result<Card> = tokio::select! {result=tokio::time::timeout(Duration::from_secs(60),operation)=>result.unwrap_or_else(|_|Err("message.jira_verification_took_too_long".into())),_=stopped.changed()=>Err("message.atelier_is_shutting_down".into())};
    let _ = state.changed.send(());
    response(result)
}
