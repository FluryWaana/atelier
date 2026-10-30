use super::{Shared, failure, object};
use crate::infrastructure::agent_providers;

use axum::{
    extract::{Path, Request, State},
    http::StatusCode,
    response::Response,
};

fn response<T: serde::Serialize>(result: crate::Result<T>) -> Response {
    super::responses::with_message(result, "message.could_not_update_profiles")
}
pub(super) async fn cli_status(State(state): State<Shared>) -> Response {
    response(
        state
            .database
            .call(|store| {
                Ok(atelier_core::application::agent_cli::operation(
                    &store.settings::<atelier_core::document::Document>()?,
                )?)
            })
            .await,
    )
}

pub(super) async fn diagnose(
    State(state): State<Shared>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    let body = match object(request.into_body()).await {
        Ok(body) => body,
        Err((status, message)) => return failure(status, message),
    };
    if body.raw("expected").is_none() {
        return failure(
            StatusCode::BAD_REQUEST,
            "message.reload_the_profiles_before_editing_them",
        );
    }
    let probe_model = match body.get::<String>("probeModel") {
        Ok(value) => value,
        Err(_) => return failure(StatusCode::BAD_REQUEST, "message.invalid_model"),
    };
    let status_only = match body.get::<bool>("statusOnly") {
        Ok(value) => value.unwrap_or(false),
        Err(_) => return failure(StatusCode::BAD_REQUEST, "message.invalid_server_response"),
    };
    let vault = state.vault.clone();
    #[cfg(windows)]
    let provider = id.clone();
    let profile = state
        .database
        .call(move |store| {
            agent_providers::diagnostic_profile(store, &body, &id, &mut |value, decrypt| {
                vault.protect(value, decrypt)
            })
        })
        .await;
    let result = match profile {
        Ok(profile) => {
            crate::infrastructure::agent_diagnostics::diagnose(
                profile,
                probe_model.as_deref(),
                status_only,
            )
            .await
        }
        Err(error) => Err(error),
    };
    #[cfg(windows)]
    if let Ok(diagnostic) = &result
        && let Some(info) = &diagnostic.update_info
        && let Some(actual) = info
            .installed_version
            .clone()
            .filter(|version| crate::infrastructure::package::version_parts(version).is_ok())
    {
        let root = info.managed_root.clone();
        let observed = state
            .database
            .call(move |store| {
                if let Some(operation) =
                    crate::infrastructure::agent_cli::read(store)?.filter(|operation| {
                        operation.provider == provider
                            && operation.phase
                                == atelier_core::application::agent_cli::Phase::Uncertain
                            && operation.info.managed_root == root
                            && root.is_some()
                    })
                {
                    crate::infrastructure::agent_cli::change(
                        store,
                        &operation.id,
                        atelier_core::application::agent_cli::Phase::Observed,
                        Some(actual),
                        "message.version_read_after_interruption_the_previous_command_s_outcome_h",
                    )?;
                }
                Ok(())
            })
            .await;
        if let Err(error) = observed {
            return response::<atelier_core::contracts::AgentDiagnostic>(Err(error));
        }
    }
    response(result)
}

pub(super) async fn describe(State(state): State<Shared>) -> Response {
    let vault = state.vault.clone();
    response(
        state
            .database
            .call(move |store| {
                agent_providers::describe(store, &mut |value, decrypt| {
                    vault.protect(value, decrypt)
                })
            })
            .await,
    )
}

pub(super) async fn discover() -> Response {
    response(crate::infrastructure::agent_diagnostics::discover().await)
}

pub(super) async fn save(State(state): State<Shared>, request: Request) -> Response {
    let vault = state.vault.clone();
    let body = match object(request.into_body()).await {
        Ok(body) => body,
        Err((status, message)) => return failure(status, message),
    };
    if body.raw("expected").is_none() {
        return failure(
            StatusCode::BAD_REQUEST,
            "message.reload_the_profiles_before_editing_them",
        );
    }
    response(
        state
            .database
            .call(move |store| {
                agent_providers::save(store, &body, &mut |value, decrypt| {
                    vault.protect(value, decrypt)
                })
            })
            .await,
    )
}

pub(super) async fn remove(
    State(state): State<Shared>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    let vault = state.vault.clone();
    let body = match object(request.into_body()).await {
        Ok(body) => body,
        Err((status, message)) => return failure(status, message),
    };
    if body.raw("expected").is_none() {
        return failure(
            StatusCode::BAD_REQUEST,
            "message.reload_the_profiles_before_editing_them",
        );
    }
    response(
        state
            .database
            .call(move |store| {
                agent_providers::remove(store, &id, &body, &mut |value, decrypt| {
                    vault.protect(value, decrypt)
                })
            })
            .await,
    )
}
