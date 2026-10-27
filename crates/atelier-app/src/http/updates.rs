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
            "Mise à jour en cours. Les modifications sont suspendues jusqu’à sa validation ou sa récupération.",
        )),
        Err(_) => Some(failure(
            StatusCode::CONFLICT,
            "Le journal de mise à jour doit être examiné avant de modifier les données.",
        )),
        _ => None,
    }
}

#[cfg(test)]
mod guard_tests {
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
            "Cette commande exige une installation Rust versionnée.",
        );
    };
    let path = request.uri().path().to_owned();
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
                "Choisis une archive et sa signature sur ce poste.",
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
            Err("La préparation de mise à jour a été interrompue. Relis son état.".into())
        }))
    } else {
        let input: Confirmation = match super::jira_transitions::input(request).await {
            Ok(value) => value,
            Err(response) => return *response,
        };
        if uuid::Uuid::parse_str(&input.id).is_err() {
            return failure(
                StatusCode::BAD_REQUEST,
                "Identifiant de mise à jour invalide.",
            );
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
                _ => Err("Action de mise à jour inconnue.".into()),
            }
        })
        .await;
        let result:Result<_>=operation.unwrap_or_else(|_|Err("La commande de mise à jour a été interrompue. Relis son état avant toute nouvelle action.".into()));
        response(result)
    }
}
