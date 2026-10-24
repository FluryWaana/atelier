use super::{Shared, failure, object};
use crate::infrastructure::{jira, settings::VaultFailure};
use atelier_core::{application::jira::JiraFailure, contracts::JiraConfigurationUpdate};
use axum::{
    Json,
    extract::{Query, Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Deserialize;

fn response<T: serde::Serialize>(result: crate::Result<T>) -> Response {
    match result {
        Ok(value) => Json(value).into_response(),
        Err(error) => {
            let status = if matches!(
                error.downcast_ref::<JiraFailure>(),
                Some(JiraFailure::Conflict)
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
        let result = jira::Jira::new(configuration, token)?.search().await?;
        let revision = result.connection_revision;
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
