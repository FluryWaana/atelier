use crate::{
    Result,
    infrastructure::{database::Database, prototype::PrototypeDirectory, session},
};
use atelier_core::{
    application::duplicate_card,
    contracts::{Board, Card},
    document::Document,
};
use axum::{
    Json,
    body::{Body, to_bytes},
    extract::{Path, Request, State},
    http::StatusCode,
    middleware,
    response::{IntoResponse, Response},
};

use serde_json::json;
use std::{path::PathBuf, sync::Arc};
use tokio::{net::TcpListener, sync::watch};

#[cfg(windows)]
mod agent_cli;
mod agent_providers;
mod assets;
mod automation;
pub use assets::wasm_asset_name;
mod backups;
mod brainstorm;
mod card_lifecycle;
mod commits;
mod connector_effects;
mod connector_ticket_reset;
mod connectors;
mod events;
mod files;
mod folders;
mod forge;
mod forge_draft;
mod forge_publication;
mod git_push;
mod integrations;
mod jira;
mod jira_creation;
mod jira_refresh;
mod jira_transitions;
mod kanban;
mod mutations;
mod preparation;
mod projects;
mod queue;
mod repository_access;
mod responses;
mod reviews;
mod routes;
mod runs;
mod security;
mod services;
mod settings;
mod task_creation;
mod task_editing;
mod task_templates;
mod tempo;
mod tempo_export;
mod time_tracking;
mod updates;
mod workflow_publication;
mod worktrees;

use security::{health, security};

pub const CSP: &str = "default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self'; connect-src 'self'; img-src 'self' data:; frame-ancestors 'none'; base-uri 'none'; form-action 'self'";

struct AppState {
    native: bool,
    installation_root: Option<PathBuf>,
    database: Arc<Database>,
    prototype_paths: [PathBuf; 2],
    prototype_directory: Arc<cap_std::fs::Dir>,
    vault: crate::infrastructure::token_vault::PrototypeVault,
    backups: Arc<crate::infrastructure::backups::BackupDirectory>,
    token: String,
    event_token: String,
    port: u16,
    shutdown: watch::Sender<bool>,
    changed: tokio::sync::broadcast::Sender<()>,
    file_reads: tokio::sync::Semaphore,
    integration_reads: tokio::sync::Semaphore,
    mutations: Arc<tokio::sync::RwLock<()>>,
    services: Arc<crate::infrastructure::dev_servers::DevServers>,
    runs: Arc<crate::infrastructure::command_runs::CommandRuns>,
}
type Shared = Arc<AppState>;

fn failure(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({"error":message}))).into_response()
}

async fn board(State(state): State<Shared>) -> Response {
    let vault = state.vault.clone();
    match state
        .database
        .call(move |store| {
            use crate::infrastructure::store::Documents;
            let mut cards = Vec::new();
            let mut brainstorms = Vec::new();
            for card in store.documents::<Card>(Documents::Cards)? {
                if card.fields.flag("brainstormSession")? {
                    brainstorms.push(card);
                } else {
                    cards.push(card);
                }
            }
            Ok(Board {
                settings: crate::infrastructure::settings::public_config(
                    store,
                    &mut |value, decrypt| vault.protect(value, decrypt),
                )?,
                cards,
                brainstorms,
                projects: store.documents(Documents::Projects)?,
                runs: store.documents(Documents::Runs)?,
                times: store.documents(Documents::Times)?,
                revision: store.revision(),
            })
        })
        .await
    {
        Ok(board) => Json(board).into_response(),
        Err(_) => failure(
            StatusCode::INTERNAL_SERVER_ERROR,
            "message.storage_read_failed",
        ),
    }
}

async fn object(body: Body) -> std::result::Result<Document, (StatusCode, &'static str)> {
    // Bound the encoded request before parsing or allocating its fields.
    let bytes = to_bytes(body, 400_000)
        .await
        .map_err(|_| (StatusCode::PAYLOAD_TOO_LARGE, "message.request_too_large"))?;
    let raw = std::str::from_utf8(&bytes)
        .map_err(|_| (StatusCode::BAD_REQUEST, "message.invalid_json"))?;
    let value: &serde_json::value::RawValue =
        serde_json::from_str(if raw.is_empty() { "{}" } else { raw })
            .map_err(|_| (StatusCode::BAD_REQUEST, "message.invalid_json"))?;
    if !value.get().starts_with('{') {
        return Err((StatusCode::BAD_REQUEST, "message.a_json_object_is_required"));
    }
    serde_json::from_str(value.get()).map_err(|_| (StatusCode::BAD_REQUEST, "message.invalid_json"))
}

fn timestamp() -> Result<String> {
    Ok(
        time::OffsetDateTime::now_utc().format(time::macros::format_description!(
            "[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z"
        ))?,
    )
}

async fn duplicate(
    State(state): State<Shared>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    if let Err((status, message)) = object(request.into_body()).await {
        return failure(status, message);
    }
    let timestamp = match timestamp() {
        Ok(value) => value,
        Err(_) => {
            return failure(
                StatusCode::INTERNAL_SERVER_ERROR,
                "message.clock_unavailable",
            );
        }
    };
    let copied = state
        .database
        .call(move |store| {
            use crate::infrastructure::store::Documents;
            let Some(card) = store.document::<Card>(Documents::Cards, &id)? else {
                return Ok(None);
            };
            let copy = duplicate_card(&card, uuid::Uuid::new_v4().to_string(), timestamp)?;
            store.put(Documents::Cards, &copy)?;
            Ok(Some(copy))
        })
        .await;
    match copied {
        Ok(Some(card)) => Json(card).into_response(),
        Ok(None) => failure(StatusCode::NOT_FOUND, "message.card_not_found"),
        Err(_) => failure(
            StatusCode::INTERNAL_SERVER_ERROR,
            "message.card_duplicate_failed",
        ),
    }
}

async fn stop(State(state): State<Shared>, request: Request) -> Response {
    if let Err((status, message)) = object(request.into_body()).await {
        return failure(status, message);
    }
    let _ = state.shutdown.send(true);
    Json(json!({"ok":true})).into_response()
}

async fn events(State(state): State<Shared>) -> Response {
    events::response(state.changed.subscribe(), state.shutdown.subscribe())
}

async fn unavailable() -> Response {
    failure(StatusCode::NOT_FOUND, "message.route_not_found")
}

// Headless HTTP journeys keep the same Rust assets and guards without opening desktop controls.
pub async fn serve(listener: TcpListener, directory: PathBuf, assets: PathBuf) -> Result<()> {
    serve_mode(listener, directory, assets, false, None, false).await
}

pub async fn serve_native(
    listener: TcpListener,
    directory: PathBuf,
    assets: PathBuf,
) -> Result<()> {
    serve_mode(listener, directory, assets, true, None, false).await
}

/// Local development only: keep the tab authenticated across supervised restarts.
pub async fn serve_development(
    listener: TcpListener,
    directory: PathBuf,
    assets: PathBuf,
) -> Result<()> {
    serve_mode(listener, directory, assets, true, None, true).await
}

pub async fn serve_installed(
    listener: TcpListener,
    directory: PathBuf,
    assets: PathBuf,
    root: PathBuf,
) -> Result<()> {
    serve_mode(listener, directory, assets, true, Some(root), false).await
}

async fn serve_mode(
    listener: TcpListener,
    directory: PathBuf,
    assets: PathBuf,
    desktop: bool,
    installation_root: Option<PathBuf>,
    development: bool,
) -> Result<()> {
    if !listener.local_addr()?.ip().is_loopback() {
        return Err("Écoute distante interdite.".into());
    }
    let app = if development {
        assets::development_router(&assets)?
    } else {
        assets::router(&assets)?
    };
    let _package_guard =
        crate::infrastructure::package_maintenance::acquire(installation_root.as_deref())?;
    let expected_identity = installation_root
        .as_ref()
        .map(|root| crate::infrastructure::updates::expected_data_identity(root, &directory))
        .transpose()?
        .flatten();
    let original_directory = std::path::absolute(&directory)?;
    let directory = PrototypeDirectory::open(&directory)?;
    let service = directory.vault_service()?;
    if expected_identity.is_some_and(|expected| {
        service
            != format!(
                "{}{expected}",
                crate::infrastructure::token_vault::PROTOTYPE_SERVICE_PREFIX
            )
    }) {
        return Err("Le dossier de données a changé avant le démarrage de la version.".into());
    }
    let vault = crate::infrastructure::token_vault::PrototypeVault::new(service)?;
    let token = if development {
        match session::read_optional_in(&directory.opened_directory()?)? {
            Some(token) => token,
            None => session::new_token()?,
        }
    } else {
        session::new_token()?
    };
    directory.write_session(&token)?;
    let database = directory.database()?;
    let cleanup_vault = vault.clone();
    let retired = database
        .call(move |store| {
            crate::infrastructure::connectors::retire_local_connectors(
                store,
                &mut |value, decrypt| cleanup_vault.protect(value, decrypt),
            )
        })
        .await;
    // A locked vault must not prevent local work or discard credentials during cleanup.
    if let Err(error) = retired
        && !error.is::<crate::infrastructure::settings::VaultFailure>()
    {
        return Err(error);
    }
    database
        .call(|store| {
            let now = time::OffsetDateTime::now_utc()
                .format(&time::format_description::well_known::Rfc3339)?;
            crate::infrastructure::jira_import::recover(store)?;
            crate::infrastructure::jira_creation::recover(store)?;
            crate::infrastructure::jira_refresh::recover(store)?;
            crate::infrastructure::jira_transition::recover(store)?;
            crate::infrastructure::forge_draft::recover(store)?;
            crate::infrastructure::connector_effects::recover(store)?;
            let mut settings = store.settings::<atelier_core::document::Document>()?;
            if atelier_core::application::agent_cli::recover(&mut settings, &now)? {
                store.save_settings(&settings)?;
            }
            crate::infrastructure::runs::recover(store, &now)?;
            crate::infrastructure::automation::recover(store)?;
            crate::infrastructure::queue::recover(store)
        })
        .await?;
    database
        .call(|store| store.transaction(atelier_core::application::time_tracking::initialize))
        .await?;
    let (shutdown, mut stopped) = watch::channel(false);
    let changed = tokio::sync::broadcast::channel(16).0;
    let state = Arc::new(AppState {
        native: desktop,
        installation_root,
        database: Arc::new(database),
        prototype_paths: [directory.path.clone(), original_directory],
        prototype_directory: Arc::new(directory.opened_directory()?),
        vault,
        backups: Arc::new(directory.backups()?),
        token,
        event_token: session::new_token()?,
        port: listener.local_addr()?.port(),
        shutdown,
        services: crate::infrastructure::dev_servers::DevServers::new(changed.clone()),
        runs: crate::infrastructure::command_runs::CommandRuns::new(changed.clone()),
        changed,
        file_reads: tokio::sync::Semaphore::new(2),
        integration_reads: tokio::sync::Semaphore::new(4),
        mutations: Arc::new(tokio::sync::RwLock::new(())),
    });
    let app = routes::router(app)
        .layer(middleware::from_fn_with_state(state.clone(), security))
        .with_state(state.clone());
    let stop_events = state.shutdown.clone();
    let clock = tokio::spawn(time_tracking::run(state.clone()));
    let queue_worker = tokio::spawn(queue::run(state.clone()));
    let tray = if !desktop {
        None
    } else {
        match crate::infrastructure::tray::Owner::start(&state.prototype_paths[0], state.port).await
        {
            Ok(tray) => tray,
            Err(_) => {
                eprintln!(
                    "Icône système indisponible ; Atelier reste accessible dans le navigateur."
                );
                None
            }
        }
    };
    let result = axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            tokio::select! { _ = stopped.changed() => {}, _ = tokio::signal::ctrl_c() => {} }
            let _ = stop_events.send(true);
        })
        .await;
    let _ = state.shutdown.send(true);
    let tray_result = match tray {
        Some(tray) => tray.close().await,
        None => Ok(()),
    };
    let clock_result = clock.await;
    let queue_result = queue_worker.await;
    let _mutations = state.mutations.write().await;
    let services_result = state.services.close().await;
    let runs_result = state.runs.close().await;
    let timers = state
        .database
        .call(|store| {
            store.transaction(|store| {
                atelier_core::application::time_tracking::close(
                    store,
                    &crate::infrastructure::time_tracking::Clock,
                )
            })
        })
        .await;
    state.database.close().await?;
    drop(directory);
    services_result?;
    runs_result?;
    queue_result??;
    clock_result??;
    timers?;
    tray_result?;
    result?;
    Ok(())
}
