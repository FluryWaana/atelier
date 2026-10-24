use super::{Shared, failure, object};
use crate::{
    Result,
    infrastructure::{store::Documents, worktrees},
};
use atelier_core::{
    application::{
        card_lifecycle::CardFailure, task_creation::CreationContext, worktrees as usecase,
    },
    contracts::{Card, Worktree, WorktreeState},
    document::Document,
};
use axum::{
    Json,
    extract::{Path, Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};

async fn save(
    state: &Shared,
    id: String,
    expected: Option<Worktree>,
    next: Option<Worktree>,
) -> Result<Card> {
    state
        .database
        .call(move |store| {
            store.transaction(|store| {
                let card = store
                    .document::<Card>(Documents::Cards, &id)?
                    .ok_or(CardFailure::Missing)?;
                let card = usecase::associate(
                    card,
                    expected.as_ref(),
                    next.as_ref(),
                    store.timestamp()?,
                    cfg!(windows),
                )?;
                store.put(Documents::Cards, &card)?;
                Ok(card)
            })
        })
        .await
}

async fn change(state: &Shared, id: String, body: Document, remove: bool) -> Result<Card> {
    let lookup = id.clone();
    let (card, settings) = state
        .database
        .call(move |store| {
            Ok((
                store
                    .document::<Card>(Documents::Cards, &lookup)?
                    .ok_or(CardFailure::Missing)?,
                store.settings::<Document>()?,
            ))
        })
        .await?;
    let original = card.fields.get::<Worktree>("worktree")?;
    let root = &state.prototype_directory;
    let roots = &state.prototype_paths;
    if remove {
        let discard = usecase::removal(&body)?;
        let worktree = original.ok_or("Aucun worktree associé.")?;
        let mut removing = worktree.clone();
        removing.state = WorktreeState::Removing;
        save(state, id.clone(), Some(worktree), Some(removing.clone())).await?;
        worktrees::remove(root, roots, &removing, discard).await?;
        return save(state, id, Some(removing), None).await;
    }
    let options = usecase::create_options(&card, &settings, &body)?;
    let worktree = match original {
        Some(worktree) => worktree,
        None => {
            let worktree =
                worktrees::prepare(root, roots, &options.repo, options.branch, options.base)
                    .await?;
            save(state, id.clone(), None, Some(worktree.clone())).await?;
            worktree
        }
    };
    if worktree.state == WorktreeState::Removing {
        worktrees::inspect(root, roots, &worktree).await?;
        worktrees::create(root, roots, &worktree).await?;
    } else {
        worktrees::create(root, roots, &worktree).await?;
    }
    let mut ready = worktree.clone();
    ready.state = WorktreeState::Ready;
    save(state, id, Some(worktree), Some(ready)).await
}

async fn mutate(state: Shared, id: String, request: Request, remove: bool) -> Response {
    if !cfg!(windows) {
        return failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "Les worktrees natifs attendent leur qualification sur ce système.",
        );
    }
    let Ok(_readers) = state.file_reads.try_acquire_many(2) else {
        return failure(
            StatusCode::CONFLICT,
            "Attends la fin de la lecture des fichiers avant de modifier le worktree.",
        );
    };
    let body = match object(request.into_body()).await {
        Ok(body) => body,
        Err((status, message)) => return failure(status, message),
    };
    let result = change(&state, id, body, remove).await;
    // Intent records and partial failures must be visible in every open board.
    let _ = state.changed.send(());
    match result {
        Ok(card) => Json(card).into_response(),
        Err(error) => {
            let status = match error.downcast_ref::<CardFailure>() {
                Some(CardFailure::Missing) => StatusCode::NOT_FOUND,
                Some(CardFailure::Conflict(_)) => StatusCode::CONFLICT,
                _ if error.is::<rusqlite::Error>() => StatusCode::INTERNAL_SERVER_ERROR,
                _ => StatusCode::BAD_REQUEST,
            };
            let message = if status == StatusCode::INTERNAL_SERVER_ERROR {
                "Enregistrement du worktree impossible.".to_owned()
            } else {
                error.to_string()
            };
            failure(status, &message)
        }
    }
}

pub(super) async fn create(
    State(state): State<Shared>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    mutate(state, id, request, false).await
}

pub(super) async fn remove(
    State(state): State<Shared>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    mutate(state, id, request, true).await
}
