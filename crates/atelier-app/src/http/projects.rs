use super::{Shared, failure, object};
use crate::{Result, infrastructure::store::Documents};
use atelier_core::{
    application::{
        projects::{self},
        task_creation::CreationContext,
        task_editing,
    },
    document::Document,
};
use axum::{
    extract::{Path, Request, State},
    http::StatusCode,
    response::Response,
};

enum Action {
    Create,
    Archive,
    Remove(String),
}

#[derive(Debug)]
struct BackupFailure;
impl std::fmt::Display for BackupFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Sauvegarde du projet impossible.")
    }
}
impl std::error::Error for BackupFailure {}

async fn mutate(state: Shared, action: Action, request: Request) -> Response {
    let body = match object(request.into_body()).await {
        Ok(body) => body,
        Err((status, message)) => return failure(status, message),
    };
    let result = state
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
        .await;
    response(result)
}

fn response(result: Result<Document>) -> Response {
    super::responses::with_message(result, "message.the_project_could_not_be_updated")
}
pub(super) async fn update(
    State(state): State<Shared>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    let body = match object(request.into_body()).await {
        Ok(body) => body,
        Err((status, message)) => return failure(status, message),
    };
    let backups = state.backups.clone();
    let result = state
        .database
        .call(move |store| {
            // Project middleware reserves runtime activity before this database operation.
            let edit = projects::prepare_update(
                store,
                &id,
                &body,
                cfg!(windows),
                |_| false,
                |_| false,
                false,
            )?;
            if edit.moved {
                backups.create(store).map_err(|_| BackupFailure)?;
            }
            store.transaction(|store| {
                store.put(Documents::Projects, &edit.project)?;
                if edit.moved {
                    let repo = edit
                        .project
                        .raw("repo")
                        .ok_or("message.directory_missing")?;
                    for mut card in edit.cards {
                        card.set("repo", &repo)?;
                        card.set("updatedAt", &store.timestamp()?)?;
                        store.put(Documents::Cards, &card)?;
                        let id = card.get::<String>("id")?.ok_or("message.card_id_missing")?;
                        let invalidation = task_editing::invalidate_reviews(
                            &id,
                            &mut card,
                            store.documents(Documents::Runs)?,
                            projects::LOCATION_CHANGE_REASON,
                        )?;
                        for run in invalidation.runs {
                            store.put(Documents::Runs, &run)?;
                        }
                        if invalidation.automation_changed {
                            store.put(Documents::Cards, &card)?;
                        }
                    }
                }
                Ok(edit.project)
            })
        })
        .await;
    response(result)
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

pub(super) async fn detect(
    State(state): State<Shared>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    let use_draft = request.method() == axum::http::Method::POST;
    let body = match object(request.into_body()).await {
        Ok(body) => body,
        Err((status, message)) => return failure(status, message),
    };
    let project = match state
        .database
        .call(move |store| store.document::<Document>(Documents::Projects, &id))
        .await
    {
        Ok(project) => project,
        Err(_) => {
            return failure(
                StatusCode::INTERNAL_SERVER_ERROR,
                "message.project_read_failed",
            );
        }
    };
    let root = state.prototype_directory.clone();
    let paths = state.prototype_paths.clone();
    // Both callbacks enforce the synthetic repositories subtree before any manifest access.
    let result = tokio::task::spawn_blocking(move || {
        use crate::infrastructure::launch_detection;
        projects::detect(
            project.as_ref(),
            &body,
            use_draft,
            |value| launch_detection::synthetic_repository(&root, &paths, value),
            |repo| launch_detection::detect_synthetic(&root, &paths, repo),
        )
    })
    .await;
    match result {
        Ok(result) => response(result),
        Err(_) => failure(
            StatusCode::INTERNAL_SERVER_ERROR,
            "message.project_detection_failed",
        ),
    }
}
