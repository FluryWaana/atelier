use super::{Shared, failure, object};
use crate::{Result, infrastructure::store::Documents};
use atelier_core::{
    application::backups::{self, BackupFailure},
    document::Document,
};
use axum::{
    Json,
    extract::{Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};

fn response(result: Result<Value>) -> Response {
    match result {
        Ok(value) => Json(value).into_response(),
        Err(error) => match error.downcast_ref::<BackupFailure>() {
            Some(BackupFailure::Conflict(message)) => failure(StatusCode::CONFLICT, message),
            Some(BackupFailure::Invalid(message)) => failure(StatusCode::BAD_REQUEST, message),
            None if error.is::<serde_json::Error>() => {
                failure(StatusCode::BAD_REQUEST, "message.corrupted_backup")
            }
            None => failure(
                StatusCode::INTERNAL_SERVER_ERROR,
                "message.the_backup_operation_could_not_be_completed",
            ),
        },
    }
}

pub(super) async fn list(State(state): State<Shared>) -> Response {
    let backups = state.backups.clone();
    response(
        state
            .database
            .call(move |_| Ok(json!({"directory":backups.directory,"backups":backups.list()?})))
            .await,
    )
}

async fn mutate(state: Shared, request: Request, restore: bool) -> Response {
    let body = match object(request.into_body()).await {
        Ok(body) => body,
        Err((status, message)) => return failure(status, message),
    };
    let directory = state.backups.clone();
    response(
        state
            .database
            .call(move |store| {
                let mut interrupted = false;
                for card in store.documents::<atelier_core::contracts::Card>(Documents::Cards)? {
                    interrupted |= atelier_core::application::worktrees::interrupted(&card)?;
                }
                if interrupted {
                    return Err(BackupFailure::Conflict(
                        "message.finish_or_verify_worktree_operations_before_creating_or_restorin",
                    )
                    .into());
                }
                let mut active_runs = false;
                for run in store.documents::<Document>(Documents::Runs)? {
                    active_runs |= crate::infrastructure::runs::active(&run)?;
                }
                if store
                    .settings::<Document>()?
                    .raw("jiraImportReservation")
                    .is_some()
                {
                    return Err(
                        BackupFailure::Conflict("message.a_jira_import_is_in_progress").into(),
                    );
                }
                backups::require_idle(
                    active_runs,
                    false,
                    false,
                    &store.documents(Documents::Times)?,
                )?;
                if !restore {
                    return Ok(serde_json::to_value(directory.create(store)?)?);
                }
                let name = backups::restore_name(&body)?;
                let restored = directory.restore_with(store, &name, |store| {
                    let mut settings: Document = store.settings()?;
                    backups::pause_restored_queue(&mut settings)?;
                    store.save_settings(&settings)
                })?;
                // Tokens are read from SQLite per request; there is no in-memory credential cache to clear.
                Ok(serde_json::to_value(restored)?)
            })
            .await,
    )
}

pub(super) async fn create(State(state): State<Shared>, request: Request) -> Response {
    mutate(state, request, false).await
}
pub(super) async fn restore(State(state): State<Shared>, request: Request) -> Response {
    mutate(state, request, true).await
}
