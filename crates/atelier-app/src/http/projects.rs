use super::{Shared, failure, object};
use crate::infrastructure::store::Documents;
use atelier_core::{
    application::projects::{self, ProjectFailure},
    document::Document,
};
use axum::{
    Json,
    extract::{Path, Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};

enum Action {
    Create,
    Archive,
    Remove(String),
}

async fn mutate(state: Shared, action: Action, request: Request) -> Response {
    let body = match object(request.into_body()).await {
        Ok(body) => body,
        Err((status, message)) => return failure(status, message),
    };
    match state
        .database
        .call(move |store| {
            store.transaction(|store| {
                // shortcut: M04/M06 must provide live activity and development servers before enabling them.
                match action {
                    Action::Create => projects::create(store, &body, cfg!(windows)),
                    Action::Archive => {
                        let count =
                            projects::archive_tasks(store, &body, cfg!(windows), |_| false)?;
                        let mut result = Document::default();
                        result.set("count", &count)?;
                        Ok(result)
                    }
                    Action::Remove(id) => {
                        let archived =
                            projects::remove(store, &id, &body, cfg!(windows), |_| false, false)?;
                        let mut result = Document::default();
                        result.set("ok", &true)?;
                        result.set("archived", &archived)?;
                        Ok(result)
                    }
                }
            })
        })
        .await
    {
        Ok(result) => Json(result).into_response(),
        Err(error) => {
            let status = match error.downcast_ref::<ProjectFailure>() {
                Some(ProjectFailure::Missing) => StatusCode::NOT_FOUND,
                Some(ProjectFailure::Conflict(_)) => StatusCode::CONFLICT,
                _ if error.is::<rusqlite::Error>() || error.is::<serde_json::Error>() => {
                    StatusCode::INTERNAL_SERVER_ERROR
                }
                _ => StatusCode::BAD_REQUEST,
            };
            if status == StatusCode::INTERNAL_SERVER_ERROR {
                failure(status, "Modification du projet impossible.")
            } else {
                failure(status, &error.to_string())
            }
        }
    }
}

pub(super) async fn create(State(state): State<Shared>, request: Request) -> Response {
    mutate(state, Action::Create, request).await
}
pub(super) async fn archive(State(state): State<Shared>, request: Request) -> Response {
    mutate(state, Action::Archive, request).await
}
pub(super) async fn remove(
    State(state): State<Shared>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    mutate(state, Action::Remove(id), request).await
}

pub(super) async fn unavailable(State(state): State<Shared>, Path(id): Path<String>) -> Response {
    match state
        .database
        .call(move |store| store.document::<Document>(Documents::Projects, &id))
        .await
    {
        Ok(Some(_)) => failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "Cette opération de projet n’est pas encore disponible dans le prototype Rust.",
        ),
        Ok(None) => failure(StatusCode::NOT_FOUND, "Projet introuvable."),
        Err(_) => failure(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Lecture du projet impossible.",
        ),
    }
}
