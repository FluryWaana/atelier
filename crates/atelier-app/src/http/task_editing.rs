use super::{Shared, failure, object};
use crate::infrastructure::store::Documents;
use atelier_core::{
    application::{card_lifecycle::CardFailure, task_editing},
    contracts::Card,
};
use axum::{
    Json,
    extract::{Path, Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};

enum Outcome {
    Saved(Card),
    Unavailable(&'static str),
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
    match state.database.call(move |store| store.transaction(|store| {
        let original = store.document::<Card>(Documents::Cards, &id)?.ok_or(CardFailure::Missing)?;
        // shortcut: M04/M06 must supply live activity before enabling agents and automation.
        let edit = task_editing::prepare(store, &original, &body, false)?;
        if edit.sync_status && edit.card.fields.truthy("ticket")? { return Ok(Outcome::Unavailable("La synchronisation du ticket n’est pas encore disponible dans le prototype Rust.")); }
        let mut card = edit.card;
        store.put(Documents::Cards, &card)?;
        if edit.requirements_changed {
            let invalidation = task_editing::invalidate_reviews(&card.id, &mut card.fields, store.documents(Documents::Runs)?, task_editing::REVIEW_CHANGE_REASON)?;
            for run in invalidation.runs { store.put(Documents::Runs, &run)?; }
            if invalidation.automation_changed { store.put(Documents::Cards, &card)?; }
        }
        Ok(Outcome::Saved(card))
    })).await {
        Ok(Outcome::Saved(card)) => Json(card).into_response(),
        Ok(Outcome::Unavailable(message)) => failure(StatusCode::SERVICE_UNAVAILABLE, message),
        Err(error) => {
            let status = match error.downcast_ref::<CardFailure>() {
                Some(CardFailure::Missing) => StatusCode::NOT_FOUND,
                Some(CardFailure::Conflict(_)) => StatusCode::CONFLICT,
                Some(CardFailure::Document(_)) => StatusCode::INTERNAL_SERVER_ERROR,
                _ if error.is::<rusqlite::Error>() || error.is::<serde_json::Error>() => StatusCode::INTERNAL_SERVER_ERROR,
                _ => StatusCode::BAD_REQUEST,
            };
            if status == StatusCode::INTERNAL_SERVER_ERROR { failure(status, "Modification de la carte impossible.") }
            else { failure(status, &error.to_string()) }
        }
    }
}
