use super::{Shared, failure, object};
use atelier_core::application::task_templates;
use axum::{
    Json,
    extract::{Path, Request, State},
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
        Err(error) => super::responses::error(error, "message.templates_update_failed"),
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
