use super::failure;
use crate::infrastructure::settings::VaultFailure;
use atelier_core::application::{forge::ForgeFailure, jira::JiraFailure};
use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
pub(super) fn response<T: serde::Serialize>(result: crate::Result<T>) -> Response {
    match result {
        Ok(value) => Json(value).into_response(),
        Err(error) => {
            let status = if matches!(
                error.downcast_ref::<atelier_core::application::settings::SettingsFailure>(),
                Some(atelier_core::application::settings::SettingsFailure::Conflict)
            ) || matches!(
                error.downcast_ref::<JiraFailure>(),
                Some(JiraFailure::Conflict | JiraFailure::OperationConflict(_))
            ) || matches!(
                error.downcast_ref::<ForgeFailure>(),
                Some(ForgeFailure::Conflict | ForgeFailure::OperationConflict(_))
            ) || matches!(
                error.downcast_ref::<atelier_core::application::card_lifecycle::CardFailure>(),
                Some(atelier_core::application::card_lifecycle::CardFailure::Conflict(_))
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
                failure(status, "message.unable_to_read_or_save_retry_or_reload")
            } else {
                failure(status, &error.to_string())
            }
        }
    }
}
