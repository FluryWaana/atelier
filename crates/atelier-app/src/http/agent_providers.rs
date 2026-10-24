use super::{Shared, failure, object};
use crate::infrastructure::{agent_providers, settings::VaultFailure};
use atelier_core::application::agent_providers::ProviderFailure;
use axum::{
    Json,
    extract::{Path, Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};

fn response(result: crate::Result<Vec<atelier_core::document::Document>>) -> Response {
    match result {
        Ok(providers) => Json(providers).into_response(),
        Err(error) => {
            let status = match error.downcast_ref::<ProviderFailure>() {
                Some(ProviderFailure::Missing) => StatusCode::NOT_FOUND,
                Some(ProviderFailure::Conflict(_)) => StatusCode::CONFLICT,
                _ if error.is::<VaultFailure>() => StatusCode::SERVICE_UNAVAILABLE,
                _ if error.is::<rusqlite::Error>() || error.is::<serde_json::Error>() => {
                    StatusCode::INTERNAL_SERVER_ERROR
                }
                _ => StatusCode::BAD_REQUEST,
            };
            if status == StatusCode::INTERNAL_SERVER_ERROR {
                failure(status, "Modification des profils impossible.")
            } else {
                failure(status, &error.to_string())
            }
        }
    }
}

pub(super) async fn describe(State(state): State<Shared>) -> Response {
    response(state.database.call(agent_providers::describe).await)
}

pub(super) async fn save(State(state): State<Shared>, request: Request) -> Response {
    let body = match object(request.into_body()).await {
        Ok(body) => body,
        Err((status, message)) => return failure(status, message),
    };
    response(
        state
            .database
            .call(move |store| agent_providers::save(store, &body))
            .await,
    )
}

pub(super) async fn remove(
    State(state): State<Shared>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    if let Err((status, message)) = object(request.into_body()).await {
        return failure(status, message);
    }
    response(
        state
            .database
            .call(move |store| agent_providers::remove(store, &id))
            .await,
    )
}
