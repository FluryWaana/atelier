use super::{Shared, failure, responses::response};
use crate::infrastructure::project_clone::{self, Destination};
use atelier_core::application::project_clone::CloneRequest;
use axum::{
    extract::{Path, Request, State},
    http::StatusCode,
    response::Response,
};

pub(super) async fn clone(State(state): State<Shared>, request: Request) -> Response {
    let input: CloneRequest = match super::input::parse(request).await {
        Ok(input) => input,
        Err(response) => return *response,
    };
    if let Err(error) = input
        .validate()
        .and_then(|_| project_clone::remote(&input.url).map(|_| ()))
    {
        return response(Err::<(), _>(error));
    }
    if !state.native
        && !state
            .prototype_paths
            .iter()
            .any(|root| std::path::Path::new(&input.parent).starts_with(root.join("repositories")))
    {
        return failure(
            StatusCode::FORBIDDEN,
            "message.clone_native_destination_required",
        );
    }
    let mut shutdown = state.shutdown.subscribe();
    let mut cancellation = state.clone_cancellation.subscribe();
    if *shutdown.borrow() {
        return failure(StatusCode::CONFLICT, "message.atelier_is_shutting_down");
    }
    let credentials = input.clone();
    let vault = state.vault.clone();
    let authentication = match state
        .database
        .call(move |store| {
            project_clone::authentication(store, &credentials, &mut |value, decrypt| {
                vault.protect(value, decrypt)
            })
        })
        .await
    {
        Ok(value) => value,
        Err(error) => return response(Err::<(), _>(error)),
    };
    let destination = match Destination::reserve(&input) {
        Ok(value) => value,
        Err(error) => return response(Err::<(), _>(error)),
    };
    let operation = input.operation.clone();
    let stopped = async move {
        tokio::select! {
            _ = shutdown.wait_for(|stopped| *stopped) => {},
            _ = cancellation.wait_for(|cancelled| cancelled == &operation) => {},
        }
        "message.clone_cancelled".into()
    };
    if let Err(error) = destination
        .download(&input, authentication.as_ref(), stopped)
        .await
    {
        return response(
            destination
                .discard()
                .map_err(|_| crate::Error::from("message.clone_cleanup_failed"))
                .and(Err::<(), _>(error)),
        );
    }
    let path = match destination.publish() {
        Ok(path) => path,
        Err(error) => {
            return response(
                destination
                    .discard()
                    .map_err(|_| crate::Error::from("message.clone_cleanup_failed"))
                    .and(Err::<(), _>(error)),
            );
        }
    };
    // A complete clone belongs to the user even when registration or project creation fails.
    let selected = path.clone();
    let root = state.prototype_directory.clone();
    let authorized = state
        .database
        .call(move |_| destination.authorize(&root, &selected))
        .await
        .is_ok();
    response(Ok(
        serde_json::json!({"path":path, "authorized":authorized}),
    ))
}

pub(super) async fn cancel(State(state): State<Shared>, Path(operation): Path<String>) -> Response {
    if operation.is_empty()
        || operation.len() > 100
        || !operation
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-')
    {
        return failure(StatusCode::BAD_REQUEST, "message.clone_invalid_request");
    }
    state.clone_cancellation.send_replace(operation);
    response(Ok(serde_json::json!({"ok":true})))
}
