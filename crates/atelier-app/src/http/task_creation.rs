use super::{Shared, failure, object};
use crate::infrastructure::store::Documents;
use atelier_core::application::{card_lifecycle::CardFailure, task_creation};
use axum::{
    Json,
    extract::{Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};

pub(super) async fn create(State(state): State<Shared>, request: Request) -> Response {
    let brainstorming = request.uri().path() == "/api/brainstorms";
    let body = match object(request.into_body()).await {
        Ok(body) => body,
        Err((status, message)) => return failure(status, message),
    };
    match state
        .database
        .call(move |store| {
            store.transaction(|store| {
                let card = task_creation::create(store, &body, brainstorming)?;
                store.put(Documents::Cards, &card)?;
                Ok(card)
            })
        })
        .await
    {
        Ok(card) => Json(card).into_response(),
        Err(error) if error.is::<serde_json::Error>() || error.is::<rusqlite::Error>() => failure(
            StatusCode::INTERNAL_SERVER_ERROR,
            "message.card_creation_failed",
        ),
        Err(error) => {
            if matches!(
                error.downcast_ref::<CardFailure>(),
                Some(CardFailure::Document(_))
            ) {
                failure(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "message.card_creation_failed",
                )
            } else {
                failure(StatusCode::BAD_REQUEST, &error.to_string())
            }
        }
    }
}
