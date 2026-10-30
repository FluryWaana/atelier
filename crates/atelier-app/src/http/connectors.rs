use super::{Shared, responses::response};
#[path = "connector_repositories.rs"]
pub(super) mod repositories;
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

pub(super) async fn import_tickets(
    State(state): State<Shared>,
    axum::extract::Path(id): axum::extract::Path<String>,
    request: axum::extract::Request,
) -> Response {
    let input: atelier_core::application::extensions::TicketImport =
        match super::input::parse(request).await {
            Ok(input) => input,
            Err(response) => return *response,
        };
    let result = state
        .database
        .call(move |store| crate::infrastructure::connector_tickets::import(store, &id, &input))
        .await;
    if result.is_ok() {
        let _ = state.changed.send(());
    }
    response(result)
}

pub(super) async fn refresh_ticket(
    State(state): State<Shared>,
    axum::extract::Path(id): axum::extract::Path<String>,
    request: axum::extract::Request,
) -> Response {
    let input: atelier_core::application::extensions::TicketRefresh =
        match super::input::parse(request).await {
            Ok(input) => input,
            Err(response) => return *response,
        };
    response(
        state
            .database
            .call(move |store| {
                if input.expected.id != id {
                    return Err("message.the_synchronization_card_does_not_match".into());
                }
                let link = input
                    .expected
                    .fields
                    .get::<atelier_core::document::Document>("ticket")?
                    .ok_or("message.no_connector_ticket_is_linked")?;
                let connector = link
                    .get::<String>("connector")?
                    .ok_or("message.no_connector_ticket_is_linked")?;
                let module = connector
                    .strip_prefix("extension:")
                    .ok_or("message.no_connector_ticket_is_linked")?;
                crate::infrastructure::connector_tickets::refresh(store, module, &input)
            })
            .await,
    )
}

use {
    crate::{Result, infrastructure::extensions::Executable},
    atelier_core::application::extensions::{Configuration, Invocation, Report, Status},
    axum::extract::{Path, Request},
    std::{path::PathBuf, time::Duration},
};
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Removal {
    expected_revision: u64,
    confirmed: bool,
}

pub(super) async fn configure(
    State(state): State<Shared>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    let input: Configuration = match super::input::parse(request).await {
        Ok(input) => input,
        Err(response) => return *response,
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
pub(super) async fn remove(
    State(state): State<Shared>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    let input: Removal = match super::input::parse(request).await {
        Ok(input) => input,
        Err(response) => return *response,
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

pub(super) async fn invoke(
    State(state): State<Shared>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    let input: Invocation = match super::input::parse(request).await {
        Ok(input) => input,
        Err(response) => return *response,
    };
    if let Err(error) = repositories::guard_invocation(&state, input.method) {
        return response::<Report>(Err(error));
    }
    let operation = async {
        let vault = state.vault.clone();
        let module = id.clone();
        let (profile, mut request) = state
            .database
            .call(move |store| {
                let (profile, mut request) =
                    connectors::invocation(store, &module, &input, &mut |value, decrypt| {
                        vault.protect(value, decrypt)
                    })?;
                if request.method == atelier_core::application::extensions::Method::Action {
                    let count =
                        crate::infrastructure::connector_tickets::imported_count(store, &profile)?;
                    if request.input.is_null() {
                        request.input = serde_json::json!({});
                    }
                    let object = request
                        .input
                        .as_object_mut()
                        .ok_or("message.connector_action_input_object")?;
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
        let repository_guard = repositories::prepare_read(&state, &profile, &mut request).await?;
        let repository_review =
            request.method == atelier_core::application::extensions::Method::RepositoriesReview;
        let action = request.action.clone();
        let transitions_source = if request.method
            == atelier_core::application::extensions::Method::TicketsTransitions
        {
            Some(request.input.clone())
        } else {
            None
        };
        let time_source =
            if request.method == atelier_core::application::extensions::Method::TimePreview {
                Some(request.input.clone())
            } else {
                None
            };
        let mut reply = executable
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
        if let Some(guard) = repository_guard {
            reply.output =
                serde_json::to_value(guard.observe(&reply.output, repository_review).await?)?;
        }
        if let Some(source) = time_source
            && serde_json::to_value(
                crate::infrastructure::connector_time::proposal(&reply.output)?.source,
            )? != source
        {
            return Err(
                "message.the_module_changed_the_requested_entry_in_its_time_preview".into(),
            );
        }
        if let Some(source) = transitions_source
            && serde_json::to_value(
                crate::infrastructure::connector_ticket_transitions::report(&reply.output)?.ticket,
            )? != source
        {
            return Err(
                "message.the_module_answered_for_another_ticket_while_reading_transitions".into(),
            );
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
        let _ = state.changed.send(());
        Ok::<_, crate::Error>(report)
    };
    let mut closing = state.shutdown.subscribe();
    let stopping = *closing.borrow();
    let result: Result<Report> = if stopping {
        drop(operation);
        Err("message.atelier_is_shutting_down".into())
    } else {
        tokio::select! {
            result=tokio::time::timeout(Duration::from_secs(45),operation)=>result.unwrap_or_else(|_|Err("message.connector_call_timeout".into())),
            _=closing.changed()=>Err("message.atelier_is_shutting_down".into()),
        }
    };
    response(result)
}
