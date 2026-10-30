use super::{Shared, failure, responses::response};
use crate::infrastructure::repository_access;
use axum::{
    extract::{Request, State},
    http::StatusCode,
    response::Response,
};
use serde::Deserialize;
fn public(registry: repository_access::Registry, available: bool) -> serde_json::Value {
    serde_json::json!({"revision":registry.revision,"repositories":registry.repositories.into_iter().map(|entry|entry.path).collect::<Vec<_>>(),"available":available})
}
pub(super) async fn read(State(state): State<Shared>) -> Response {
    let root = state.prototype_directory.clone();
    let native = state.native;
    response(
        state
            .database
            .call(move |_| Ok(public(repository_access::read(&root)?, native)))
            .await,
    )
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Input {
    path: String,
    expected_revision: u64,
    confirmed: bool,
}
pub(super) async fn register(State(state): State<Shared>, request: Request) -> Response {
    if !state.native {
        return failure(
            StatusCode::CONFLICT,
            "message.authorize_repositories_from_the_native_launcher",
        );
    }
    let input: Input = match super::input::parse(request).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    let root = state.prototype_directory.clone();
    response(
        state
            .database
            .call(move |_| {
                Ok(public(
                    repository_access::register(
                        &root,
                        &input.path,
                        input.expected_revision,
                        input.confirmed,
                    )?,
                    true,
                ))
            })
            .await,
    )
}
pub(super) async fn revoke(State(state): State<Shared>, request: Request) -> Response {
    if !state.native {
        return failure(
            StatusCode::CONFLICT,
            "message.authorize_repositories_from_the_native_launcher",
        );
    }
    let input: Input = match super::input::parse(request).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    let root = state.prototype_directory.clone();
    response(
        state
            .database
            .call(move |store| {
                let mut in_use = repository_access::references(
                    &store.settings::<serde_json::Value>()?,
                    &input.path,
                );
                for table in [
                    crate::infrastructure::store::Documents::Projects,
                    crate::infrastructure::store::Documents::Cards,
                ] {
                    for value in store.documents::<serde_json::Value>(table)? {
                        in_use |= repository_access::references(&value, &input.path);
                    }
                }
                Ok(public(
                    repository_access::revoke(
                        &root,
                        &input.path,
                        input.expected_revision,
                        input.confirmed,
                        in_use,
                    )?,
                    true,
                ))
            })
            .await,
    )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BranchInput {
    path: String,
}

pub(super) async fn default_branch(State(state): State<Shared>, request: Request) -> Response {
    let input: BranchInput = match super::input::parse(request).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    if atelier_core::domain::text(&input.path, 2000).is_err() || input.path.trim().is_empty() {
        return failure(StatusCode::BAD_REQUEST, "message.invalid_request");
    }
    response(
        crate::infrastructure::files::Repository::default_branch(
            &state.prototype_directory,
            &state.prototype_paths,
            &input.path,
        )
        .await
        .map(|branch| serde_json::json!({"branch":branch})),
    )
}
