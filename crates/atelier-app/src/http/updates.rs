use super::{Shared, failure, responses::response};
use crate::{Result, infrastructure::updates};
use axum::{
    extract::{Request, State},
    http::StatusCode,
    response::Response,
};
use serde::Deserialize;
use std::path::PathBuf;

pub(super) fn guard(root: Option<&std::path::Path>) -> Option<Response> {
    match root.map(updates::maintenance).transpose() {
        Ok(Some(true)) => Some(failure(
            StatusCode::CONFLICT,
            "message.update_in_progress_changes_are_paused_until_verification_or_reco",
        )),
        Err(_) => Some(failure(
            StatusCode::CONFLICT,
            "message.the_update_journal_needs_attention_before_data_can_be_changed",
        )),
        _ => None,
    }
}

#[cfg(test)]
mod guard_tests {
    #[test]
    fn install_selection_rejects_unknown_fields_and_missing_versions() {
        assert!(serde_json::from_str::<super::ReleaseSelection>(r#"{"version":"1.2.3"}"#).is_ok());
        for body in [
            r#"{}"#,
            r#"{"version":"1.2.3","archive":"/tmp/untrusted"}"#,
            r#"{"version":123}"#,
        ] {
            assert!(serde_json::from_str::<super::ReleaseSelection>(body).is_err());
        }
    }

    #[test]
    fn absent_installation_is_allowed_but_unreadable_or_partial_journal_blocks_writes() {
        assert!(super::guard(None).is_none());
        let root = tempfile::tempdir().unwrap();
        assert!(super::guard(Some(root.path())).is_none());
        std::fs::write(
            root.path().join("update-operation.json"),
            b"partial synthetic journal",
        )
        .unwrap();
        assert_eq!(
            super::guard(Some(root.path())).unwrap().status(),
            axum::http::StatusCode::CONFLICT
        );
        let blocked = super::guard(Some(&root.path().join("missing-installation"))).unwrap();
        assert_eq!(blocked.status(), axum::http::StatusCode::CONFLICT);
    }
}

pub(super) async fn get(State(state): State<Shared>) -> Response {
    response(updates::status(state.installation_root.as_deref()))
}
pub(super) async fn check(State(state): State<Shared>) -> Response {
    let status = match updates::status(state.installation_root.as_deref()) {
        Ok(status) => status,
        Err(error) => return response::<()>(Err(error)),
    };
    response(updates::check_release(&status.current_version, &status.target).await)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReleaseSelection {
    version: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Source {
    archive: String,
    signature: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Confirmation {
    id: String,
    confirmed: bool,
}

pub(super) async fn action(State(state): State<Shared>, request: Request) -> Response {
    let Some(root) = state.installation_root.clone() else {
        return failure(
            StatusCode::CONFLICT,
            "message.this_command_requires_a_versioned_rust_installation",
        );
    };
    let path = request.uri().path().to_owned();
    if path.ends_with("/install") {
        let selection: ReleaseSelection = match super::jira_transitions::input(request).await {
            Ok(value) => value,
            Err(response) => return *response,
        };
        if crate::infrastructure::package::version_parts(&selection.version).is_err() {
            return failure(StatusCode::BAD_REQUEST, "message.github_update_invalid");
        }
        let data = state.prototype_paths[0].clone();
        let port = state.port;
        let result = tokio::task::spawn_blocking(move || {
            tokio::runtime::Handle::current().block_on(updates::install_release(
                &root,
                &selection.version,
                &data,
                port,
            ))
        })
        .await;
        return response(
            result.unwrap_or_else(|_| Err("message.github_update_unavailable".into())),
        );
    }
    if path.ends_with("/preview") {
        let source: Source = match super::jira_transitions::input(request).await {
            Ok(value) => value,
            Err(response) => return *response,
        };
        if [&source.archive, &source.signature].iter().any(|path| {
            path.is_empty()
                || path.len() > 2000
                || path.chars().any(char::is_control)
                || !std::path::Path::new(path).is_absolute()
        }) {
            return failure(
                StatusCode::BAD_REQUEST,
                "message.choose_an_archive_and_its_signature_on_this_computer",
            );
        }
        let data = state.prototype_paths[0].clone();
        let port = state.port;
        let operation = tokio::task::spawn_blocking(move || {
            tokio::runtime::Handle::current().block_on(updates::preview(
                &root,
                &PathBuf::from(source.archive),
                &PathBuf::from(source.signature),
                &data,
                port,
            ))
        })
        .await;
        response(operation.unwrap_or_else(|_| {
            Err("message.update_preparation_was_interrupted_read_its_status_again".into())
        }))
    } else {
        let input: Confirmation = match super::jira_transitions::input(request).await {
            Ok(value) => value,
            Err(response) => return *response,
        };
        if uuid::Uuid::parse_str(&input.id).is_err() {
            return failure(StatusCode::BAD_REQUEST, "message.update_id_invalid");
        }
        let operation = tokio::task::spawn_blocking(move || {
            let runtime = tokio::runtime::Handle::current();
            match path.as_str() {
                "/api/updates/apply" => {
                    runtime.block_on(updates::apply(&root, &input.id, input.confirmed))
                }
                "/api/updates/cancel" => updates::cancel(&root, &input.id, input.confirmed),
                "/api/updates/recover" => {
                    runtime.block_on(updates::request_recovery(&root, &input.id, input.confirmed))
                }
                _ => Err("message.update_action_unknown".into()),
            }
        })
        .await;
        let result: Result<_> = operation.unwrap_or_else(|_| {
            Err("message.the_update_command_was_interrupted_read_its_status_before_taking".into())
        });
        response(result)
    }
}
