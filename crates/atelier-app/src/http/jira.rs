use super::{Shared, failure, object};
use crate::infrastructure::{jira, settings::VaultFailure};
use atelier_core::{application::jira::JiraFailure, contracts::JiraConfigurationUpdate};
use axum::{
    Json,
    extract::{Path, Query, Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Deserialize;

pub(super) fn response<T: serde::Serialize>(result: crate::Result<T>) -> Response {
    match result {
        Ok(value) => Json(value).into_response(),
        Err(error) => {
            let status = if matches!(
                error.downcast_ref::<JiraFailure>(),
                Some(JiraFailure::Conflict | JiraFailure::OperationConflict(_))
            ) {
                StatusCode::CONFLICT
            } else if error.is::<VaultFailure>() {
                StatusCode::SERVICE_UNAVAILABLE
            } else if error.is::<serde_json::Error>() || error.is::<rusqlite::Error>() {
                StatusCode::INTERNAL_SERVER_ERROR
            } else {
                StatusCode::BAD_REQUEST
            };
            if status == StatusCode::INTERNAL_SERVER_ERROR {
                failure(
                    status,
                    "Lecture ou enregistrement de la connexion Jira impossible.",
                )
            } else {
                failure(status, &error.to_string())
            }
        }
    }
}

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
        None => return failure(StatusCode::BAD_REQUEST, "Paramètres Jira invalides."),
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
    read_issue(state, query, None).await
}
pub(super) async fn detail(
    State(state): State<Shared>,
    Path(id): Path<String>,
    Query(query): Query<Revision>,
) -> Response {
    read_issue(state, query, Some(id)).await
}
async fn read_issue(state: Shared, query: Revision, id: Option<String>) -> Response {
    let mut stopped = state.shutdown.subscribe();
    if *stopped.borrow() {
        return failure(StatusCode::CONFLICT, "Atelier est en cours d’arrêt.");
    }
    let Ok(_permit) = state.integration_reads.try_acquire() else {
        return failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "Les recherches distantes sont occupées. Réessaie dans un instant.",
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
        let result = if let Some(id) = id {
            serde_json::to_value(client.detail(&id).await?)?
        } else {
            serde_json::to_value(client.search().await?)?
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
        _=stopped.changed()=>failure(StatusCode::CONFLICT,"Atelier est en cours d’arrêt."),
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
        None => return failure(StatusCode::BAD_REQUEST, "Ticket ou projet Jira invalide."),
    };
    let mut stopped = state.shutdown.subscribe();
    let Ok(_permit) = state.integration_reads.try_acquire() else {
        return failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "Les recherches distantes sont occupées. Réessaie dans un instant.",
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
        Err("Atelier est en cours d’arrêt.".into())
    } else {
        tokio::select! { result=reading=>result,_=stopped.changed()=>Err("Atelier est en cours d’arrêt.".into()) }
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
            "Import Jira interrompu. Redémarre Atelier avant de réessayer.",
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
        Err(response) => return response,
    };
    let mut stopped = state.shutdown.subscribe();
    if *stopped.borrow() {
        return failure(StatusCode::CONFLICT, "Atelier est en cours d’arrêt.");
    }
    let Ok(_permit) = state.integration_reads.try_acquire() else {
        return failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "Les recherches distantes sont occupées. Réessaie dans un instant.",
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
        result=tokio::time::timeout(std::time::Duration::from_secs(60),reading)=>result.unwrap_or_else(|_|Err("Lecture du lot Jira trop longue. Réduis la sélection.".into())),
        _=stopped.changed()=>Err("Atelier est en cours d’arrêt.".into()),
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
            "Import Jira interrompu. Redémarre Atelier avant de réessayer.",
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
        return failure(StatusCode::CONFLICT, "Atelier est en cours d’arrêt.");
    }
    let Ok(_permit) = state.integration_reads.try_acquire() else {
        return failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "Les recherches distantes sont occupées. Réessaie dans un instant.",
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
    tokio::select! {result=tokio::time::timeout(std::time::Duration::from_secs(60),operation)=>response(result.unwrap_or_else(|_|Err("Lecture des métadonnées Jira trop longue.".into()))),_=stopped.changed()=>failure(StatusCode::CONFLICT,"Atelier est en cours d’arrêt.")}
}
