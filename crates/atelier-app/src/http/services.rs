use super::{Shared, failure, object};
use crate::{
    Result,
    infrastructure::{dev_servers::DevServers, service_launch, store::Documents},
};
use atelier_core::{
    application::{card_lifecycle::CardFailure, services},
    contracts::Card,
    document::Document,
};
use axum::{
    Json,
    extract::{Path, Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};

pub(super) fn guard(manager: &DevServers, path: &str) -> Option<Response> {
    let cards = manager.active_cards();
    if cards.is_empty() {
        return None;
    }
    let projects = manager.active_projects();
    let blocked = path.starts_with("/api/backups")
        // Global mutations (repository selection, module writes/reset, CLI updates)
        // share this alias with the run and persistent-operation guards.
        || path == "/api/projects"
        || path == "/api/projects/archive-tasks"
        || path
            .strip_prefix("/api/projects/")
            .is_some_and(|tail| projects.iter().any(|id| tail == id))
        || path.strip_prefix("/api/cards/").is_some_and(|tail| {
            let (id, action) = tail.split_once('/').unwrap_or((tail, ""));
            cards.iter().any(|card| card == id)
                && !action.starts_with("services")
                && action != "runs/start"
                && !action.starts_with("jira/")
                && !["forge/attach", "forge/detach"].contains(&action)
        });
    blocked.then(|| {
        failure(
            StatusCode::CONFLICT,
            "message.stop_the_affected_services_before_changing_the_task_project_or_b",
        )
    })
}

pub(super) async fn get(State(state): State<Shared>, Path(id): Path<String>) -> Response {
    Json(state.services.snapshot(&id)).into_response()
}

pub(super) async fn start(
    State(state): State<Shared>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    let body = match object(request.into_body()).await {
        Ok(body) => body,
        Err((status, message)) => return failure(status, message),
    };
    let result = async {
        let standalone = body.get::<bool>("standalone")?.unwrap_or(false);
        let plan = state
            .database
            .call(move |store| {
                let card = store
                    .document::<Card>(Documents::Cards, &id)?
                    .ok_or(CardFailure::Missing)?;
                services::plan(
                    &card,
                    &store.documents::<Document>(Documents::Projects)?,
                    standalone,
                    cfg!(windows),
                )
            })
            .await?;
        let prepared =
            service_launch::prepare(&state.prototype_directory, &state.prototype_paths, plan)
                .await?;
        state.services.start(prepared).await
    }
    .await;
    respond(result)
}

pub(super) async fn stop(
    State(state): State<Shared>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    let body = match object(request.into_body()).await {
        Ok(body) => body,
        Err((status, message)) => return failure(status, message),
    };
    let result = async {
        let service = body.get::<String>("serviceId")?;
        if service.as_ref().is_some_and(String::is_empty) {
            return Err("message.service_not_found".into());
        }
        state.services.stop(&id, service.as_deref()).await?;
        state
            .services
            .snapshot(&id)
            .ok_or_else(|| "message.service_session_missing".into())
    }
    .await;
    respond(result)
}

pub(super) async fn restart(
    State(state): State<Shared>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    let body = match object(request.into_body()).await {
        Ok(body) => body,
        Err((status, message)) => return failure(status, message),
    };
    let result = async {
        let service = body
            .get::<String>("serviceId")?
            .filter(|value| !value.is_empty())
            .ok_or("message.service_not_found")?;
        let standalone = state
            .services
            .snapshot(&id)
            .ok_or("message.start_the_stack_from_this_task_first")?
            .project_ids
            .len()
            == 1;
        let mut plan = state
            .database
            .call(move |store| {
                let card = store
                    .document::<Card>(Documents::Cards, &id)?
                    .ok_or(CardFailure::Missing)?;
                services::plan(
                    &card,
                    &store.documents::<Document>(Documents::Projects)?,
                    standalone,
                    cfg!(windows),
                )
            })
            .await?;
        plan.services.retain(|launch| launch.id == service);
        if plan.services.len() != 1 {
            return Err(
                "message.this_service_is_no_longer_configured_start_the_stack_with_the_ne".into(),
            );
        }
        let prepared =
            service_launch::prepare(&state.prototype_directory, &state.prototype_paths, plan)
                .await?;
        state.services.restart(prepared).await
    }
    .await;
    respond(result)
}

fn respond(result: Result<atelier_core::contracts::ServiceSession>) -> Response {
    match result {
        Ok(session) => Json(session).into_response(),
        Err(error) => failure(
            match error.downcast_ref::<CardFailure>() {
                Some(CardFailure::Missing) => StatusCode::NOT_FOUND,
                Some(CardFailure::Conflict(_)) => StatusCode::CONFLICT,
                _ => StatusCode::BAD_REQUEST,
            },
            &error.to_string(),
        ),
    }
}
