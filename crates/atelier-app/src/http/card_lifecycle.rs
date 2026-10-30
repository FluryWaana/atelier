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
        Err(_) => {
            return failure(
                StatusCode::INTERNAL_SERVER_ERROR,
                "message.clock_unavailable",
            );
        }
    };
    let changed = state
        .database
        .call(move |store| {
            store.transaction(|store| {
                let card = store
                    .document::<Card>(Documents::Cards, &id)?
                    .ok_or(CardFailure::Missing)?;
                let times = store.documents::<Document>(Documents::Times)?;
                let active = crate::infrastructure::runs::active_cards(store)?;
                match action {
                    Action::Archive => {
                        let card = card_lifecycle::archive(
                            card,
                            &body,
                            &times,
                            |id| active.contains(id),
                            timestamp,
                        )?;
                        store.put(Documents::Cards, &card)?;
                        Ok(serde_json::value::to_raw_value(&card)?)
                    }
                    Action::Remove => {
                        card_lifecycle::remove(
                            &card,
                            &store.documents::<Document>(Documents::Cards)?,
                            &times,
                            &crate::infrastructure::queue::read(store)?.ids,
                            |id| active.contains(id),
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
        Err(error) => super::responses::error(error, "message.card_update_failed"),
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
