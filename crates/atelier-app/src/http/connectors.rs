use super::{Shared, responses::response};
use crate::infrastructure::connectors;
use axum::{extract::State, response::Response};

pub(super) async fn get(State(state): State<Shared>) -> Response {
    let vault = state.vault.clone();
    response(
        state
            .database
            .call(move |store| {
                connectors::describe(store, &mut |value, decrypt| vault.protect(value, decrypt))
            })
            .await,
    )
}

#[cfg(windows)]
use {
    crate::{
        Result,
        infrastructure::{extensions::Executable, store::Documents},
    },
    atelier_core::{
        application::extensions::{
            Candidate, Configuration, Installation, Invocation, Report, Status,
        },
        document::Document,
    },
    axum::{
        extract::{Path, Request},
        http::StatusCode,
    },
    std::{path::PathBuf, time::Duration},
};
#[cfg(windows)]
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Probe {
    module: String,
    path: String,
    expected_revision: u64,
    confirmed: bool,
}
#[cfg(windows)]
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Removal {
    expected_revision: u64,
    confirmed: bool,
}

#[cfg(windows)]
pub(super) async fn probe(State(state): State<Shared>, request: Request) -> Response {
    let input: Probe = match super::jira_transitions::input(request).await {
        Ok(input) => input,
        Err(response) => return response,
    };
    if !input.confirmed
        || !atelier_core::application::extensions::identifier(&input.module)
        || input.path.chars().any(char::is_control)
        || input.path.len() > 32_000
    {
        return super::failure(
            StatusCode::BAD_REQUEST,
            "Confirme l’exécution de ce programme de confiance avant de lire son manifeste.",
        );
    }
    let operation = async {
        state
            .database
            .call(move |store| connectors::require_registry(store, input.expected_revision))
            .await?;
        let executable = Executable::open(PathBuf::from(input.path), None).await?;
        let (binary, manifest) = executable.describe(&input.module).await?;
        state
            .database
            .call(move |store| connectors::require_registry(store, input.expected_revision))
            .await?;
        Ok::<_, crate::Error>(Candidate { binary, manifest })
    };
    response(
        tokio::time::timeout(Duration::from_secs(30), operation)
            .await
            .unwrap_or_else(|_| Err("Découverte du connecteur trop longue.".into())),
    )
}

#[cfg(windows)]
pub(super) async fn install(State(state): State<Shared>, request: Request) -> Response {
    let input: Installation = match super::jira_transitions::input(request).await {
        Ok(input) => input,
        Err(response) => return response,
    };
    if !input.confirmed {
        return super::failure(
            StatusCode::BAD_REQUEST,
            "Confirme le binaire et le manifeste exacts avant d’installer le connecteur.",
        );
    }
    let operation = async {
        let revision = input.expected_revision;
        state
            .database
            .call(move |store| connectors::require_registry(store, revision))
            .await?;
        let executable = Executable::open(
            PathBuf::from(&input.candidate.binary.path),
            Some(input.candidate.binary.sha256.clone()),
        )
        .await?;
        let (binary, manifest) = executable.describe(&input.candidate.manifest.id).await?;
        let vault = state.vault.clone();
        state
            .database
            .call(move |store| {
                connectors::install(
                    store,
                    &input,
                    Candidate { binary, manifest },
                    &mut |value, decrypt| vault.protect(value, decrypt),
                )
            })
            .await
    };
    response(
        tokio::time::timeout(Duration::from_secs(30), operation)
            .await
            .unwrap_or_else(|_| Err("Installation du connecteur trop longue.".into())),
    )
}

#[cfg(windows)]
pub(super) async fn configure(
    State(state): State<Shared>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    let input: Configuration = match super::jira_transitions::input(request).await {
        Ok(input) => input,
        Err(response) => return response,
    };
    let vault = state.vault.clone();
    response(
        state
            .database
            .call(move |store| {
                connectors::configure(store, &id, &input, &mut |value, decrypt| {
                    vault.protect(value, decrypt)
                })
            })
            .await,
    )
}
#[cfg(windows)]
pub(super) async fn remove(
    State(state): State<Shared>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    let input: Removal = match super::jira_transitions::input(request).await {
        Ok(input) => input,
        Err(response) => return response,
    };
    let vault = state.vault.clone();
    response(
        state
            .database
            .call(move |store| {
                connectors::remove(
                    store,
                    &id,
                    input.expected_revision,
                    input.confirmed,
                    &mut |value, decrypt| vault.protect(value, decrypt),
                )
            })
            .await,
    )
}

#[cfg(windows)]
pub(super) async fn invoke(
    State(state): State<Shared>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    let input: Invocation = match super::jira_transitions::input(request).await {
        Ok(input) => input,
        Err(response) => return response,
    };
    let operation = async {
        let vault = state.vault.clone();
        let module = id.clone();
        let (profile, request) = state
            .database
            .call(move |store| {
                let (profile, mut request) =
                    connectors::invocation(store, &module, &input, &mut |value, decrypt| {
                        vault.protect(value, decrypt)
                    })?;
                if request.method == atelier_core::application::extensions::Method::Action {
                    let count = store
                        .documents::<atelier_core::contracts::Card>(Documents::Cards)?
                        .iter()
                        .filter(|card| {
                            card.fields
                                .get::<Document>("ticket")
                                .ok()
                                .flatten()
                                .is_some_and(|ticket| {
                                    ticket
                                        .string_equals("connector", &format!("extension:{module}"))
                                        .unwrap_or(false)
                                })
                        })
                        .count();
                    if request.input.is_null() {
                        request.input = serde_json::json!({});
                    }
                    let object = request
                        .input
                        .as_object_mut()
                        .ok_or("L’entrée d’une action de connecteur doit être un objet.")?;
                    object.insert("importedCount".into(), serde_json::json!(count));
                }
                Ok((profile, request))
            })
            .await?;
        let executable = Executable::open(
            PathBuf::from(&profile.binary.path),
            Some(profile.binary.sha256.clone()),
        )
        .await?;
        let action = request.action.clone();
        let reply = executable
            .prepare(&profile.manifest, request, Duration::from_secs(30))?
            .dispatch()
            .await?;
        if reply.status != Status::Ok {
            return Err(if reply.message.is_empty() {
                "Le connecteur n’a pas fourni de résultat utilisable.".into()
            } else {
                reply.message.into()
            });
        }
        let report = Report {
            id: uuid::Uuid::new_v4().to_string(),
            method: reply.method,
            action,
            message: reply.message,
            output: reply.output,
            observed_at: time::OffsetDateTime::now_utc()
                .format(&time::format_description::well_known::Rfc3339)?,
        };
        let saved = report.clone();
        state
            .database
            .call(move |store| connectors::record(store, &id, profile.revision, saved))
            .await?;
        Ok::<_, crate::Error>(report)
    };
    let mut closing = state.shutdown.subscribe();
    let stopping = *closing.borrow();
    let result: Result<Report> = if stopping {
        drop(operation);
        Err("Atelier est en cours d’arrêt.".into())
    } else {
        tokio::select! {
            result=tokio::time::timeout(Duration::from_secs(45),operation)=>result.unwrap_or_else(|_|Err("Appel de connecteur trop long.".into())),
            _=closing.changed()=>Err("Atelier est en cours d’arrêt.".into()),
        }
    };
    response(result)
}
