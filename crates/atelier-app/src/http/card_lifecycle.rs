use super::{Shared, failure, object, timestamp};
use crate::infrastructure::store::Documents;
use atelier_core::{
    application::card_lifecycle::{self, CardFailure},
    contracts::Card,
    document::Document,
};
use axum::{
    Json,
    extract::{Path, Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};

enum Action {
    Archive,
    Remove,
}

async fn mutate(state: Shared, id: String, action: Action, request: Request) -> Response {
    let body = match object(request.into_body()).await {
        Ok(body) => body,
        Err((status, message)) => return failure(status, message),
    };
    let timestamp = match timestamp() {
        Ok(timestamp) => timestamp,
        Err(_) => return failure(StatusCode::INTERNAL_SERVER_ERROR, "Horloge indisponible."),
    };
    let changed = state
        .database
        .call(move |store| {
            store.transaction(|store| {
                let card = store
                    .document::<Card>(Documents::Cards, &id)?
                    .ok_or(CardFailure::Missing)?;
                let times = store.documents::<Document>(Documents::Times)?;
                // shortcut: M04/M06 must supply live activity and queued IDs when agents and the queue are enabled.
                match action {
                    Action::Archive => {
                        let card =
                            card_lifecycle::archive(card, &body, &times, |_| false, timestamp)?;
                        store.put(Documents::Cards, &card)?;
                        Ok(serde_json::value::to_raw_value(&card)?)
                    }
                    Action::Remove => {
                        card_lifecycle::remove(
                            &card,
                            &store.documents::<Document>(Documents::Cards)?,
                            &times,
                            &[],
                            |_| false,
                        )?;
                        store.delete(Documents::Cards, &id)?;
                        Ok(serde_json::value::to_raw_value(
                            &serde_json::json!({"ok":true}),
                        )?)
                    }
                }
            })
        })
        .await;
    match changed {
        Ok(result) => Json(result).into_response(),
        Err(error) => {
            if let Some(failure_kind) = error.downcast_ref::<CardFailure>() {
                let status = match failure_kind {
                    CardFailure::Missing => StatusCode::NOT_FOUND,
                    CardFailure::Invalid(_) => StatusCode::BAD_REQUEST,
                    CardFailure::Conflict(_) => StatusCode::CONFLICT,
                    CardFailure::Document(_) => StatusCode::INTERNAL_SERVER_ERROR,
                };
                failure(status, &failure_kind.to_string())
            } else {
                failure(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Modification de la carte impossible.",
                )
            }
        }
    }
}

pub(super) async fn archive(
    State(state): State<Shared>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    mutate(state, id, Action::Archive, request).await
}

pub(super) async fn remove(
    State(state): State<Shared>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    mutate(state, id, Action::Remove, request).await
}
