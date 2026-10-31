use super::failure;
use crate::infrastructure::settings::VaultFailure;
use atelier_core::application::{
    agent_providers::ProviderFailure, backups::BackupFailure, card_lifecycle::CardFailure,
    forge::ForgeFailure, jira::JiraFailure, projects::ProjectFailure, settings::SettingsFailure,
    time_tracking::TimeConflict,
};
use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};

fn status(error: &crate::Error, default: StatusCode) -> StatusCode {
    if let Some(error) = error.downcast_ref::<CardFailure>() {
        return match error {
            CardFailure::Missing => StatusCode::NOT_FOUND,
            CardFailure::Invalid(_) => StatusCode::BAD_REQUEST,
            CardFailure::Conflict(_) => StatusCode::CONFLICT,
            CardFailure::Document(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };
    }
    if let Some(error) = error.downcast_ref::<ProjectFailure>() {
        return match error {
            ProjectFailure::Missing => StatusCode::NOT_FOUND,
            ProjectFailure::Invalid(_) => StatusCode::BAD_REQUEST,
            ProjectFailure::Conflict(_) => StatusCode::CONFLICT,
        };
    }
    if let Some(error) = error.downcast_ref::<ProviderFailure>() {
        return match error {
            ProviderFailure::Missing => StatusCode::NOT_FOUND,
            ProviderFailure::Busy => StatusCode::TOO_MANY_REQUESTS,
            ProviderFailure::Invalid(_) => StatusCode::BAD_REQUEST,
            ProviderFailure::Conflict(_) => StatusCode::CONFLICT,
        };
    }
    if let Some(error) = error.downcast_ref::<SettingsFailure>() {
        return match error {
            SettingsFailure::UnknownService => StatusCode::NOT_FOUND,
            SettingsFailure::Invalid(_) => StatusCode::BAD_REQUEST,
            SettingsFailure::Conflict => StatusCode::CONFLICT,
        };
    }
    if let Some(error) = error.downcast_ref::<BackupFailure>() {
        return match error {
            BackupFailure::Invalid(_) => StatusCode::BAD_REQUEST,
            BackupFailure::Conflict(_) => StatusCode::CONFLICT,
        };
    }
    if let Some(error) = error.downcast_ref::<JiraFailure>() {
        return match error {
            JiraFailure::Conflict | JiraFailure::OperationConflict(_) => StatusCode::CONFLICT,
            JiraFailure::Invalid(_) => StatusCode::BAD_REQUEST,
        };
    }
    if let Some(error) = error.downcast_ref::<ForgeFailure>() {
        return match error {
            ForgeFailure::Conflict | ForgeFailure::OperationConflict(_) => StatusCode::CONFLICT,
            ForgeFailure::Invalid(_) => StatusCode::BAD_REQUEST,
        };
    }
    if error.is::<TimeConflict>() {
        return StatusCode::CONFLICT;
    }
    if error.is::<VaultFailure>() {
        return StatusCode::SERVICE_UNAVAILABLE;
    }
    if error.is::<super::projects::BackupFailure>()
        || error.is::<serde_json::Error>()
        || error.is::<rusqlite::Error>()
        || error.is::<std::io::Error>()
        || error.is::<tokio::task::JoinError>()
    {
        return StatusCode::INTERNAL_SERVER_ERROR;
    }
    default
}

pub(super) fn error(error: crate::Error, internal_message: &str) -> Response {
    error_with_default(error, internal_message, StatusCode::BAD_REQUEST)
}

pub(super) fn conflict(error: crate::Error, internal_message: &str) -> Response {
    error_with_default(error, internal_message, StatusCode::CONFLICT)
}

fn error_with_default(
    error: crate::Error,
    internal_message: &str,
    default: StatusCode,
) -> Response {
    let status = status(&error, default);
    if status == StatusCode::INTERNAL_SERVER_ERROR {
        failure(status, internal_message)
    } else {
        failure(status, &error.to_string())
    }
}

pub(super) fn with_message<T: serde::Serialize>(
    result: crate::Result<T>,
    internal_message: &str,
) -> Response {
    match result {
        Ok(value) => Json(value).into_response(),
        Err(failure) => error(failure, internal_message),
    }
}

pub(super) fn response<T: serde::Serialize>(result: crate::Result<T>) -> Response {
    with_message(result, "message.unable_to_read_or_save_retry_or_reload")
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;

    #[tokio::test]
    async fn domain_statuses_are_consistent_and_internal_details_stay_private() {
        for (failure, expected) in [
            (
                Box::new(CardFailure::Missing) as crate::Error,
                StatusCode::NOT_FOUND,
            ),
            (
                Box::new(ProjectFailure::Conflict("synthetic")),
                StatusCode::CONFLICT,
            ),
            (
                Box::new(SettingsFailure::UnknownService),
                StatusCode::NOT_FOUND,
            ),
            (
                Box::new(ProviderFailure::Busy),
                StatusCode::TOO_MANY_REQUESTS,
            ),
            (
                Box::new(CardFailure::Invalid("synthetic")),
                StatusCode::BAD_REQUEST,
            ),
        ] {
            assert_eq!(
                error(failure, "message.storage_read_failed").status(),
                expected
            );
        }
        let response = error(
            std::io::Error::other("synthetic secret path and token").into(),
            "message.storage_read_failed",
        );
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let bytes = to_bytes(response.into_body(), 4096).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["error"], "message.storage_read_failed");
        for internal in [
            Box::new(rusqlite::Error::InvalidQuery) as crate::Error,
            Box::new(std::io::Error::other("synthetic private path")),
            Box::new(serde_json::from_str::<bool>("invalid synthetic document").unwrap_err()),
        ] {
            let response = conflict(internal, "message.storage_read_failed");
            assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
            let bytes = to_bytes(response.into_body(), 4096).await.unwrap();
            let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(body["error"], "message.storage_read_failed");
        }
        assert_eq!(
            conflict("message.operation_refused".into(), "internal").status(),
            StatusCode::CONFLICT
        );
        assert_eq!(
            conflict(CardFailure::Invalid("invalid").into(), "internal").status(),
            StatusCode::BAD_REQUEST
        );
    }
}
