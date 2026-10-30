use super::{Shared, failure, object};
use crate::infrastructure::store::Documents;
use atelier_core::application::task_creation;
use axum::{
    Json,
    extract::{Request, State},
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
        Err(error) => super::responses::error(error, "message.card_creation_failed"),
    }
}
