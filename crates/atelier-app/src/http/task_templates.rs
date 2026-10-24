use super::{Shared, failure, object};
use atelier_core::application::{card_lifecycle::CardFailure, task_templates};
use axum::{
    Json,
    extract::{Path, Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};

async fn execute(state: Shared, remove: Option<String>, request: Request) -> Response {
    let body = match object(request.into_body()).await {
        Ok(body) => body,
        Err((status, message)) => return failure(status, message),
    };
    match state
        .database
        .call(move |store| {
            store.transaction(|store| {
                if let Some(id) = remove {
                    task_templates::remove(store, &id)?;
                    Ok(serde_json::value::to_raw_value(
                        &serde_json::json!({"ok":true}),
                    )?)
                } else {
                    let template =
                        task_templates::create(store, &body, || uuid::Uuid::new_v4().to_string())?;
                    Ok(serde_json::value::to_raw_value(&template)?)
                }
            })
        })
        .await
    {
        Ok(template) => Json(template).into_response(),
        Err(error) => {
            let status = match error.downcast_ref::<CardFailure>() {
                Some(CardFailure::Missing) => StatusCode::NOT_FOUND,
                Some(CardFailure::Conflict(_)) => StatusCode::CONFLICT,
                Some(CardFailure::Document(_)) => StatusCode::INTERNAL_SERVER_ERROR,
                _ if error.is::<rusqlite::Error>() || error.is::<serde_json::Error>() => {
                    StatusCode::INTERNAL_SERVER_ERROR
                }
                _ => StatusCode::BAD_REQUEST,
            };
            if status == StatusCode::INTERNAL_SERVER_ERROR {
                failure(status, "Modification des modèles impossible.")
            } else {
                failure(status, &error.to_string())
            }
        }
    }
}

pub(super) async fn create(State(state): State<Shared>, request: Request) -> Response {
    execute(state, None, request).await
}

pub(super) async fn remove(
    State(state): State<Shared>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    execute(state, Some(id), request).await
}
