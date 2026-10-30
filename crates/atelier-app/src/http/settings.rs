use super::{Shared, failure, object};
use crate::infrastructure::settings;
use axum::{
    Json,
    extract::{Path, Request, State},
    response::{IntoResponse, Response},
};

async fn execute(state: Shared, service: Option<String>, request: Request) -> Response {
    let vault = state.vault.clone();
    let body = match object(request.into_body()).await {
        Ok(body) => body,
        Err((status, message)) => return failure(status, message),
    };
    match state
        .database
        .call(move |store| match service {
            Some(service) => settings::remove_token(store, &service, &mut |value, decrypt| {
                vault.protect(value, decrypt)
            }),
            None => settings::update(store, &body, &mut |value, decrypt| {
                vault.protect(value, decrypt)
            }),
        })
        .await
    {
        Ok(config) => Json(config).into_response(),
        Err(error) => super::responses::error(error, "message.the_settings_could_not_be_updated"),
    }
}

pub(super) async fn update(State(state): State<Shared>, request: Request) -> Response {
    execute(state, None, request).await
}

pub(super) async fn remove_token(
    State(state): State<Shared>,
    Path(service): Path<String>,
    request: Request,
) -> Response {
    execute(state, Some(service), request).await
}
