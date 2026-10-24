use super::{Shared, failure};
use crate::infrastructure::{files::Repository, store::Documents};
use atelier_core::{contracts::Card, document::JsonString};
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
}

async fn inspect(state: Shared, id: String, path: Option<String>) -> Response {
    if !cfg!(windows) {
        return failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "L’explorateur Git natif attend sa qualification sur ce système.",
        );
    }
    let Ok(_permit) = state.file_reads.try_acquire() else {
        return failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "L’explorateur est occupé. Réessaie dans un instant.",
        );
    };
    let card = match state
        .database
        .call(move |store| store.document::<Card>(Documents::Cards, &id))
        .await
    {
        Ok(Some(card)) => card,
        Ok(None) => return failure(StatusCode::NOT_FOUND, "Carte introuvable."),
        Err(_) => {
            return failure(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Lecture de la tâche impossible.",
            );
        }
    };
    if card.fields.truthy("worktree").unwrap_or(false) {
        return failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "L’inspection des worktrees nécessite leur association vérifiée à une tâche.",
        );
    }
    let repo = match card.fields.get::<JsonString>("repo") {
        Ok(Some(repo)) if !repo.0.is_empty() => repo,
        _ => {
            return failure(
                StatusCode::BAD_REQUEST,
                "Choisis un dépôt Git pour consulter ses fichiers.",
            );
        }
    };
    let repository =
        match Repository::synthetic(&state.prototype_directory, &state.prototype_paths, &repo) {
            Ok(repository) => repository,
            Err(error) => return failure(StatusCode::BAD_REQUEST, &error.to_string()),
        };
    let result = if let Some(path) = path {
        repository
            .preview(path)
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
            "Le fichier a disparu ou n’est plus accessible. Actualise la liste.",
        ),
        Err(error) => failure(StatusCode::BAD_REQUEST, &error.to_string()),
    }
}

pub(super) async fn list(State(state): State<Shared>, Path(id): Path<String>) -> Response {
    inspect(state, id, None).await
}

pub(super) async fn preview(
    State(state): State<Shared>,
    Path(id): Path<String>,
    Query(selection): Query<Selection>,
) -> Response {
    inspect(state, id, Some(selection.path)).await
}
