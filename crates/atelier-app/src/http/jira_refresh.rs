use super::{
    Shared, failure,
    jira_transitions::{client, input},
    responses::response,
};
use crate::infrastructure::jira_refresh as workflow;
use atelier_core::contracts::{Card, JiraRefreshApply, JiraRefreshRequest};
use axum::{
    extract::{Path, Request, State},
    http::StatusCode,
    response::Response,
};
use std::time::Duration;
pub(super) async fn preview(
    State(state): State<Shared>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    let input: JiraRefreshRequest = match input(request).await {
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
    let revision = input.revision;
    let issue = match state
        .database
        .call(move |store| workflow::reserve(store, &input, &reserved))
        .await
    {
        Ok(issue) => issue,
        Err(error) => return response::<Card>(Err(error)),
    };
    let mut stopped = state.shutdown.subscribe();
    let stopping = *stopped.borrow();
    let result = {
        let reading = async {
            let detail = client(&state, revision).await?.detail(&issue).await?;
            let card = id.clone();
            let reserved = operation.clone();
            state
                .database
                .call(move |store| workflow::ready(store, &card, &reserved, detail))
                .await
        };
        if stopping {
            Err("message.atelier_is_shutting_down".into())
        } else {
            tokio::select! {result=tokio::time::timeout(Duration::from_secs(60),reading)=>result.unwrap_or_else(|_|Err("message.reading_the_jira_issue_took_too_long".into())),_=stopped.changed()=>Err("message.atelier_is_shutting_down".into())}
        }
    };
    if result.is_err()
        && let Err(error) = state
            .database
            .call(move |store| workflow::interrupted(store, &id, &operation))
            .await
    {
        return response::<Card>(Err(error));
    }
    let _ = state.changed.send(());
    response(result)
}
pub(super) async fn apply(
    State(state): State<Shared>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    let input: JiraRefreshApply = match input(request).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    if input.expected.id != id {
        return failure(StatusCode::BAD_REQUEST, "message.inconsistent_jira_card");
    }
    response(
        state
            .database
            .call(move |store| workflow::apply(store, &input))
            .await,
    )
}
