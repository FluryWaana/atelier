use super::{Shared, failure, object};
use crate::{Result, infrastructure::time_tracking::Clock};
use atelier_core::{application::time_tracking, document::Document};
use axum::{
    Json,
    extract::{Path, Request, State},
    http::{Method, StatusCode},
    response::{IntoResponse, Response},
};
use std::time::Duration;

async fn execute(state: Shared, id: String, action: &str, request: Request) -> Response {
    let body = if request.method() == Method::GET {
        Document::default()
    } else {
        match object(request.into_body()).await {
            Ok(body) => body,
            Err((status, message)) => return failure(status, message),
        }
    };
    if matches!(action, "export-preview" | "export" | "reconcile") {
        return super::tempo_export::action(state, id, action, body).await;
    }
    if action == "reset-export" {
        return super::responses::response(
            state
                .database
                .call(move |store| crate::infrastructure::tempo_export::reset(store, &id, &body))
                .await,
        );
    }
    if action == "route" {
        return failure(StatusCode::BAD_REQUEST, "message.unknown_time_route");
    }
    if action == "method" {
        return failure(StatusCode::BAD_REQUEST, "message.method_not_allowed");
    }
    if matches!(action, "update" | "remove")
        && body.get::<Document>("expected").ok().flatten().is_none()
    {
        return failure(
            StatusCode::BAD_REQUEST,
            "message.read_the_time_entry_again_before_changing_it",
        );
    }
    if matches!(action, "create" | "update")
        && !body
            .get::<u32>("seconds")
            .ok()
            .flatten()
            .is_some_and(|seconds| (1..=86400).contains(&seconds))
    {
        return failure(
            StatusCode::BAD_REQUEST,
            "message.duration_must_be_an_integer_between_1_second_and_24_hours",
        );
    }
    let action = action.to_owned();
    match state
        .database
        .call(move |store| {
            store.transaction(|store| {
                let sending = action != "create"
                    && store
                        .document::<Document>(crate::infrastructure::store::Documents::Times, &id)?
                        .is_some_and(|entry| {
                            entry
                                .string_equals("exportState", "sending")
                                .unwrap_or(true)
                        });
                time_tracking::execute(
                    store,
                    &Clock,
                    &action,
                    &id,
                    &body,
                    || uuid::Uuid::new_v4().to_string(),
                    |candidate| candidate == id && sending,
                )
            })
        })
        .await
    {
        Ok(entry) => Json(entry).into_response(),
        Err(error) => failure(
            if error.is::<time_tracking::TimeConflict>() {
                StatusCode::CONFLICT
            } else {
                StatusCode::BAD_REQUEST
            },
            &error.to_string(),
        ),
    }
}

pub(super) async fn create(State(state): State<Shared>, request: Request) -> Response {
    let action = if request.method() == Method::POST {
        "create"
    } else {
        "route"
    };
    execute(state, String::new(), action, request).await
}

pub(super) async fn entry(
    State(state): State<Shared>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    let action = match *request.method() {
        Method::PATCH => "update",
        Method::DELETE => "remove",
        _ => "method",
    };
    execute(state, id, action, request).await
}

pub(super) async fn known_action(
    State(state): State<Shared>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    let action = if request.method() == Method::POST {
        request
            .uri()
            .path()
            .rsplit('/')
            .next()
            .unwrap_or("")
            .to_owned()
    } else {
        "method".to_owned()
    };
    execute(state, id, &action, request).await
}

pub(super) async fn action() -> Response {
    failure(StatusCode::BAD_REQUEST, "message.unknown_time_route")
}
pub(super) async fn run(state: Shared) -> Result<()> {
    let mut closing = state.shutdown.subscribe();
    let interval = Duration::from_secs(5);
    let mut clock = tokio::time::interval_at(tokio::time::Instant::now() + interval, interval);
    clock.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        if *closing.borrow() {
            return Ok(());
        }
        tokio::select! {
            biased;
            _ = closing.changed() => return Ok(()),
            _ = clock.tick() => {}
        }
        match state
            .database
            .call(|store| store.transaction(|store| time_tracking::tick(store, &Clock)))
            .await
        {
            Ok(true) => {
                let _ = state.changed.send(());
            }
            Ok(false) => {}
            Err(error) => {
                let _ = state.shutdown.send(true);
                return Err(error);
            }
        }
    }
}
