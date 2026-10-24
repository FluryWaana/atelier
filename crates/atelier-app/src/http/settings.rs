use super::{Shared, failure, object};
use crate::infrastructure::settings;
use atelier_core::application::{card_lifecycle::CardFailure, settings::SettingsFailure};
use axum::{
    Json,
    extract::{Path, Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};

async fn execute(state: Shared, service: Option<String>, request: Request) -> Response {
    let body = match object(request.into_body()).await {
        Ok(body) => body,
        Err((status, message)) => return failure(status, message),
    };
    match state
        .database
        .call(move |store| match service {
            Some(service) => {
                settings::remove_token(store, &service, &mut settings::prototype_protect)
            }
            None => settings::update(store, &body, &mut settings::prototype_protect),
        })
        .await
    {
        Ok(config) => Json(config).into_response(),
        Err(error) => {
            if matches!(
                error.downcast_ref::<SettingsFailure>(),
                Some(SettingsFailure::UnknownService)
            ) {
                failure(StatusCode::NOT_FOUND, "Service inconnu.")
            } else if error.is::<settings::VaultFailure>() {
                failure(StatusCode::SERVICE_UNAVAILABLE, &error.to_string())
            } else if error.is::<rusqlite::Error>()
                || error.is::<serde_json::Error>()
                || matches!(
                    error.downcast_ref::<CardFailure>(),
                    Some(CardFailure::Document(_))
                )
            {
                failure(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Modification des réglages impossible.",
                )
            } else {
                failure(StatusCode::BAD_REQUEST, &error.to_string())
            }
        }
    }
}

pub(super) async fn update(State(state): State<Shared>, request: Request) -> Response {
    execute(state, None, request).await
}

pub(super) async fn remove_token(
    State(state): State<Shared>,
    Path(service): Path<String>,
    request: Request,
) -> Response {
    execute(state, Some(service), request).await
}
