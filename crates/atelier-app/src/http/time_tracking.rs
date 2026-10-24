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
    if action == "export" || action == "reconcile" {
        return failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "L’envoi de temps n’est pas encore disponible dans le prototype Rust.",
        );
    }
    if action == "route" {
        return failure(StatusCode::BAD_REQUEST, "Route de temps inconnue.");
    }
    if action == "method" {
        return failure(StatusCode::BAD_REQUEST, "Méthode non autorisée.");
    }
    let action = action.to_owned();
    // shortcut: wire the export reservation guard when M05 enables export/reconcile; both reject above.
    match state
        .database
        .call(move |store| {
            store.transaction(|store| {
                time_tracking::execute(
                    store,
                    &Clock,
                    &action,
                    &id,
                    &body,
                    || uuid::Uuid::new_v4().to_string(),
                    |_| false,
                )
            })
        })
        .await
    {
        Ok(entry) => Json(entry).into_response(),
        Err(error) => failure(StatusCode::BAD_REQUEST, &error.to_string()),
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

pub(super) async fn action(
    State(state): State<Shared>,
    Path((id, action)): Path<(String, String)>,
    request: Request,
) -> Response {
    let action = if !["stop", "export", "reconcile", "reset-export"].contains(&action.as_str()) {
        "route"
    } else if request.method() != Method::POST {
        "method"
    } else {
        &action
    };
    execute(state, id, action, request).await
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
