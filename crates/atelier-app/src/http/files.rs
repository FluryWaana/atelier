use super::{Shared, failure};
use crate::infrastructure::{files::Repository, store::Documents, worktrees};
use atelier_core::{
    contracts::{Card, Worktree, WorktreeState},
    document::JsonString,
    domain::projects::same_repository,
};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Deserialize;

#[derive(Deserialize)]
pub(super) struct Selection {
    path: String,
    untracked: Option<bool>,
}

async fn inspect(
    state: Shared,
    id: String,
    selection: Option<Selection>,
    changes: bool,
) -> Response {
    let Ok(_permit) = state.file_reads.try_acquire() else {
        return failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "message.the_file_explorer_is_busy_try_again_shortly",
        );
    };
    let card = match state
        .database
        .call(move |store| store.document::<Card>(Documents::Cards, &id))
        .await
    {
        Ok(Some(card)) => card,
        Ok(None) => return failure(StatusCode::NOT_FOUND, "message.card_not_found"),
        Err(_) => {
            return failure(
                StatusCode::INTERNAL_SERVER_ERROR,
                "message.the_task_could_not_be_read",
            );
        }
    };
    let worktree = match card.fields.get::<Worktree>("worktree") {
        Ok(worktree) => worktree,
        Err(_) => {
            return failure(
                StatusCode::BAD_REQUEST,
                "message.invalid_git_worktree_association",
            );
        }
    };
    let repo = match card.fields.get::<JsonString>("repo") {
        Ok(Some(repo)) if !repo.0.is_empty() => repo,
        _ => {
            return failure(
                StatusCode::BAD_REQUEST,
                "message.choose_a_git_repository_to_browse_its_files",
            );
        }
    };
    let repository = if let Some(worktree) = &worktree {
        if worktree.state != WorktreeState::Ready {
            return failure(
                StatusCode::CONFLICT,
                "message.verify_or_finish_the_interrupted_operation_on_this_worktree",
            );
        }
        if !same_repository(
            &String::from_utf16_lossy(&repo.0),
            &worktree.source,
            cfg!(windows),
        ) {
            return failure(
                StatusCode::CONFLICT,
                "message.the_source_repository_no_longer_matches_the_worktree",
            );
        }
        worktrees::inspect(&state.prototype_directory, &state.prototype_paths, worktree).await
    } else {
        Repository::synthetic(&state.prototype_directory, &state.prototype_paths, &repo)
    };
    let repository = match repository {
        Ok(repository) => repository,
        Err(error) => return failure(StatusCode::BAD_REQUEST, &error.to_string()),
    };
    let base = worktree
        .as_ref()
        .map(|worktree| worktree.base_commit.as_str());
    let result = if changes {
        if let Some(selection) = selection {
            repository
                .file_diff(selection.path, base, selection.untracked)
                .await
                .and_then(|value| Ok(serde_json::to_value(value)?))
        } else {
            repository
                .changes(base)
                .await
                .and_then(|value| Ok(serde_json::to_value(value)?))
        }
    } else if let Some(selection) = selection {
        repository
            .preview(selection.path)
            .await
            .and_then(|value| Ok(serde_json::to_value(value)?))
    } else {
        repository
            .list()
            .await
            .and_then(|value| Ok(serde_json::to_value(value)?))
    };
    match result {
        Ok(value) => Json(value).into_response(),
        Err(error) if error.is::<std::io::Error>() => failure(
            StatusCode::BAD_REQUEST,
            "message.the_file_is_missing_or_no_longer_accessible_refresh_the_list",
        ),
        Err(error) => failure(StatusCode::BAD_REQUEST, &error.to_string()),
    }
}

pub(super) async fn list(State(state): State<Shared>, Path(id): Path<String>) -> Response {
    inspect(state, id, None, false).await
}

pub(super) async fn preview(
    State(state): State<Shared>,
    Path(id): Path<String>,
    Query(selection): Query<Selection>,
) -> Response {
    inspect(state, id, Some(selection), false).await
}

pub(super) async fn changes(State(state): State<Shared>, Path(id): Path<String>) -> Response {
    inspect(state, id, None, true).await
}

pub(super) async fn diff(
    State(state): State<Shared>,
    Path(id): Path<String>,
    Query(selection): Query<Selection>,
) -> Response {
    inspect(state, id, Some(selection), true).await
}
