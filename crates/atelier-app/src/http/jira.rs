use super::{Shared, failure, object, responses::response};
use crate::infrastructure::jira;
use atelier_core::{application::jira::JiraFailure, contracts::JiraConfigurationUpdate};
use axum::{
    extract::{Path, Query, Request, State},
    http::StatusCode,
    response::Response,
};
use serde::Deserialize;

pub(super) async fn get(State(state): State<Shared>) -> Response {
    let vault = state.vault.clone();
    response(
        state
            .database
            .call(move |store| {
                jira::describe(store, &mut |value, decrypt| vault.protect(value, decrypt))
            })
            .await,
    )
}
pub(super) async fn save(State(state): State<Shared>, request: Request) -> Response {
    let body = match object(request.into_body()).await {
        Ok(body) => body,
        Err((status, message)) => return failure(status, message),
    };
    let input: JiraConfigurationUpdate = match serde_json::to_string(&body)
        .ok()
        .and_then(|body| serde_json::from_str(&body).ok())
    {
        Some(input) => input,
        None => return failure(StatusCode::BAD_REQUEST, "message.invalid_jira_settings"),
    };
    let vault = state.vault.clone();
    response(
        state
            .database
            .call(move |store| {
                jira::configure(store, input, &mut |value, decrypt| {
                    vault.protect(value, decrypt)
                })
            })
            .await,
    )
}

#[derive(Deserialize)]
pub(super) struct Revision {
    revision: u64,
}
pub(super) async fn search(State(state): State<Shared>, Query(query): Query<Revision>) -> Response {
    read_issue(state, query, Read::Search).await
}
pub(super) async fn detail(
    State(state): State<Shared>,
    Path(id): Path<String>,
    Query(query): Query<Revision>,
) -> Response {
    read_issue(state, query, Read::Issue(id)).await
}
pub(super) async fn board_configuration(
    State(state): State<Shared>,
    Path(id): Path<u64>,
    Query(query): Query<Revision>,
) -> Response {
    read_issue(state, query, Read::Board(id)).await
}
enum Read {
    Search,
    Issue(String),
    Board(u64),
}
async fn read_issue(state: Shared, query: Revision, read: Read) -> Response {
    let mut stopped = state.shutdown.subscribe();
    if *stopped.borrow() {
        return failure(StatusCode::CONFLICT, "message.atelier_is_shutting_down");
    }
    let Ok(_permit) = state.integration_reads.try_acquire() else {
        return failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "message.remote_searches_are_busy_try_again_shortly",
        );
    };
    let vault = state.vault.clone();
    let operation = async {
        let (configuration, token) = state
            .database
            .call(move |store| {
                jira::credentials(store, query.revision, &mut |value, decrypt| {
                    vault.protect(value, decrypt)
                })
            })
            .await?;
        let client = jira::Jira::new(configuration, token)?;
        let revision = query.revision;
        let result = match read {
            Read::Issue(id) => serde_json::to_value(client.detail(&id).await?)?,
            Read::Search => serde_json::to_value(client.search().await?)?,
            Read::Board(id) => serde_json::to_value(client.board_configuration(id).await?)?,
        };
        state
            .database
            .call(move |store| {
                let current = store
                    .settings::<atelier_core::document::Document>()?
                    .get::<atelier_core::contracts::JiraConfiguration>("jiraConnection")?
                    .unwrap_or_default();
                if current.revision != revision {
                    return Err(JiraFailure::Conflict.into());
                }
                Ok(())
            })
            .await?;
        Ok(result)
    };
    tokio::select! {
        result=operation=>response(result),
        _=stopped.changed()=>failure(StatusCode::CONFLICT,"message.atelier_is_shutting_down"),
    }
}

pub(super) async fn import(State(state): State<Shared>, request: Request) -> Response {
    use crate::infrastructure::jira_import;
    use atelier_core::contracts::JiraImport;
    let body = match object(request.into_body()).await {
        Ok(body) => body,
        Err((status, message)) => return failure(status, message),
    };
    let input: JiraImport = match serde_json::to_string(&body)
        .ok()
        .and_then(|body| serde_json::from_str(&body).ok())
    {
        Some(input) => input,
        None => {
            return failure(
                StatusCode::BAD_REQUEST,
                "message.invalid_jira_issue_or_project",
            );
        }
    };
    let mut stopped = state.shutdown.subscribe();
    let Ok(_permit) = state.integration_reads.try_acquire() else {
        return failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "message.remote_searches_are_busy_try_again_shortly",
        );
    };
    let id = uuid::Uuid::new_v4().to_string();
    let reserved = id.clone();
    let revision = input.revision;
    let issue = input.issue_id.clone();
    if let Err(error) = state
        .database
        .call(move |store| jira_import::reserve(store, input, reserved))
        .await
    {
        return response::<serde_json::Value>(Err(error));
    }
    let vault = state.vault.clone();
    let reading = async {
        let (configuration, token) = state
            .database
            .call(move |store| {
                jira::credentials(store, revision, &mut |value, decrypt| {
                    vault.protect(value, decrypt)
                })
            })
            .await?;
        jira::Jira::new(configuration, token)?.detail(&issue).await
    };
    let stopping = *stopped.borrow();
    let fetched = if stopping {
        Err("message.atelier_is_shutting_down".into())
    } else {
        tokio::select! { result=reading=>result,_=stopped.changed()=>Err("message.atelier_is_shutting_down".into()) }
    };
    let result = match fetched {
        Ok(detail) => {
            let operation = id.clone();
            state
                .database
                .call(move |store| jira_import::finish(store, &operation, detail))
                .await
        }
        Err(error) => Err(error),
    };
    if result.is_err()
        && state
            .database
            .call(move |store| jira_import::release(store, &id))
            .await
            .is_err()
    {
        return failure(
            StatusCode::INTERNAL_SERVER_ERROR,
            "message.jira_import_interrupted_restart_atelier_before_trying_again",
        );
    }
    response(result)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct MetadataQuery {
    revision: u64,
    project: String,
    issue_type: Option<String>,
}

pub(super) async fn import_batch(State(state): State<Shared>, request: Request) -> Response {
    use crate::infrastructure::jira_import;
    use atelier_core::contracts::{JiraBatchImport, JiraImported};
    let input: JiraBatchImport = match super::jira_transitions::input(request).await {
        Ok(input) => input,
        Err(response) => return *response,
    };
    let mut stopped = state.shutdown.subscribe();
    if *stopped.borrow() {
        return failure(StatusCode::CONFLICT, "message.atelier_is_shutting_down");
    }
    let Ok(_permit) = state.integration_reads.try_acquire() else {
        return failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "message.remote_searches_are_busy_try_again_shortly",
        );
    };
    let id = uuid::Uuid::new_v4().to_string();
    let reserved = id.clone();
    let prepared = input.clone();
    if let Err(error) = state
        .database
        .call(move |store| jira_import::reserve_batch(store, prepared, reserved))
        .await
    {
        return response::<Vec<JiraImported>>(Err(error));
    }
    let reading = async {
        let client = super::jira_transitions::client(&state, input.revision).await?;
        let mut details = Vec::with_capacity(input.issue_ids.len());
        for issue in input.issue_ids {
            details.push(client.detail(&issue).await?);
        }
        Ok(details)
    };
    let fetched = tokio::select! {
        result=tokio::time::timeout(std::time::Duration::from_secs(60),reading)=>result.unwrap_or_else(|_|Err("message.reading_the_jira_batch_took_too_long_select_fewer_issues".into())),
        _=stopped.changed()=>Err("message.atelier_is_shutting_down".into()),
    };
    let result = match fetched {
        Ok(details) => {
            let reserved = id.clone();
            state
                .database
                .call(move |store| jira_import::finish_batch(store, &reserved, details))
                .await
        }
        Err(error) => Err(error),
    };
    if result.is_err()
        && state
            .database
            .call(move |store| jira_import::release(store, &id))
            .await
            .is_err()
    {
        return failure(
            StatusCode::INTERNAL_SERVER_ERROR,
            "message.jira_import_interrupted_restart_atelier_before_trying_again",
        );
    }
    response(result)
}
pub(super) async fn creation_metadata(
    State(state): State<Shared>,
    Query(query): Query<MetadataQuery>,
) -> Response {
    let mut stopped = state.shutdown.subscribe();
    if *stopped.borrow() {
        return failure(StatusCode::CONFLICT, "message.atelier_is_shutting_down");
    }
    let Ok(_permit) = state.integration_reads.try_acquire() else {
        return failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "message.remote_searches_are_busy_try_again_shortly",
        );
    };
    let operation = async {
        let client = super::jira_transitions::client(&state, query.revision).await?;
        let result = if let Some(issue_type) = query.issue_type {
            serde_json::to_value(
                client
                    .creation_metadata(&query.project, &issue_type)
                    .await?,
            )?
        } else {
            serde_json::json!({"revision":query.revision,"projectKey":query.project,"types":client.issue_types(&query.project).await?})
        };
        state
            .database
            .call(move |store| jira::require_revision(store, query.revision))
            .await?;
        Ok(result)
    };
    tokio::select! {result=tokio::time::timeout(std::time::Duration::from_secs(60),operation)=>response(result.unwrap_or_else(|_|Err("message.reading_jira_metadata_took_too_long".into()))),_=stopped.changed()=>failure(StatusCode::CONFLICT,"message.atelier_is_shutting_down")}
}
