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
    extract::{Path, Query, Request, State},
    http::{Method, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{any, get, post},
};
use serde::Deserialize;
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
mod runs;
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

use mutations::{ApiRoutes, Policy};

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

fn header_value<'a>(request: &'a Request, name: &str) -> Option<&'a str> {
    request
        .headers()
        .get(name)
        .and_then(|value| value.to_str().ok())
}

async fn security(State(state): State<Shared>, request: Request, next: Next) -> Response {
    let mut response = authorize(state, request, next).await;
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        CSP.parse().expect("constant header"),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        "nosniff".parse().expect("constant header"),
    );
    headers.insert(
        header::CACHE_CONTROL,
        "no-store".parse().expect("constant header"),
    );
    response
}

async fn authorize(state: Shared, mut request: Request, next: Next) -> Response {
    let host = header_value(&request, "host").unwrap_or("");
    if host != format!("127.0.0.1:{}", state.port) && host != format!("localhost:{}", state.port) {
        return failure(StatusCode::FORBIDDEN, "message.host_not_allowed");
    }
    if request.headers().contains_key("origin")
        && header_value(&request, "origin") != Some(format!("http://{host}").as_str())
    {
        return failure(StatusCode::FORBIDDEN, "message.origin_not_allowed");
    }
    let path = request.uri().path();
    let challenge = Query::<HealthQuery>::try_from_uri(request.uri())
        .ok()
        .and_then(|query| query.0.challenge);
    let public_proof = request.method() == Method::GET
        && path == "/api/health"
        && challenge.as_deref().is_some_and(valid_token);
    if path.starts_with("/api/") && !public_proof {
        let bearer = header_value(&request, "authorization")
            .map(|value| value.strip_prefix("Bearer ").unwrap_or(value))
            .filter(|value| !value.is_empty());
        let authenticated = if let Some(token) = bearer {
            session::same_secret(token, &state.token)
        } else {
            path == "/api/events"
                && request.method() == Method::GET
                && events::authenticated(&request, state.port, &state.event_token)
        };
        if !authenticated {
            return failure(
                StatusCode::UNAUTHORIZED,
                "message.session_expired_or_missing_reopen_atelier_using_its_launcher",
            );
        }
        if request.method() != Method::GET
            && (header_value(&request, "x-local-request") != Some("1")
                || header_value(&request, "content-type") != Some("application/json"))
        {
            return failure(
                StatusCode::FORBIDDEN,
                "message.a_local_json_request_is_required",
            );
        }
        if request.method() != Method::GET && *state.shutdown.borrow() {
            return failure(StatusCode::CONFLICT, "message.atelier_is_shutting_down");
        }
    }
    request.extensions_mut().insert(state);
    next.run(request).await
}

async fn mutate(
    State(policy): State<mutations::Policy>,
    axum::Extension(state): axum::Extension<Shared>,
    request: Request,
    next: Next,
) -> Response {
    if request.method() != Method::GET {
        let cancellation =
            request.method() == Method::POST && policy == mutations::Policy::Cancellation;
        let gate = state.mutations.clone();
        let changed = state.changed.clone();
        let services = state.services.clone();
        let runs = state.runs.clone();
        let operation = async move {
            if !cancellation {
                if let Some(response) = updates::guard(state.installation_root.as_deref()) {
                    return response;
                }
                let guard_path = if policy.project_guard() {
                    "/api/projects"
                } else {
                    request.uri().path()
                };
                if let Some(response) = services::guard(&services, guard_path) {
                    return response;
                }
                if let Some(response) = runs::guard(&runs, guard_path) {
                    return response;
                }
                let guarded_path = guard_path.to_owned();
                let operation_path = request.uri().path().to_owned();
                match state
                    .database
                    .call(move |store| {
                        let tempo_path =
                            if crate::infrastructure::connector_effects::reconciles_time(
                                store,
                                &operation_path,
                            )? {
                                &operation_path
                            } else {
                                &guarded_path
                            };
                        Ok(crate::infrastructure::connector_effects::blocks(
                            store,
                            &operation_path,
                        )? || atelier_core::application::agent_cli::busy(
                            &store.settings::<atelier_core::document::Document>()?,
                        )? || crate::infrastructure::jira_transition::blocks(
                            store,
                            &guarded_path,
                        )? || crate::infrastructure::tempo_export::blocks(store, tempo_path)?
                            || crate::infrastructure::forge_draft::blocks(store, &guarded_path)?
                            || crate::infrastructure::automation::blocks(store, &guarded_path)?)
                    })
                    .await
                {
                    Ok(true) => {
                        return failure(
                            StatusCode::CONFLICT,
                            "message.finish_or_inspect_the_current_operation_before_changing_this_tas",
                        );
                    }
                    Err(_) => {
                        return failure(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            "message.unable_to_read_jira_operations",
                        );
                    }
                    Ok(false) => {}
                }
            }
            let response = next.run(request).await;
            if response.status().is_success() {
                let _ = changed.send(());
            }
            response
        };
        if cancellation {
            mutations::detached(operation).await
        } else {
            mutations::run(gate, policy.exclusive(), operation).await
        }
    } else {
        next.run(request).await
    }
}

fn valid_token(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

#[derive(Deserialize)]
struct HealthQuery {
    challenge: Option<String>,
}

async fn health(State(state): State<Shared>, Query(query): Query<HealthQuery>) -> Response {
    if let Some(challenge) = query.challenge.filter(|value| valid_token(value)) {
        return match session::proof(&state.token, &challenge, state.port) {
            Ok(proof) => Json(json!({"proof":proof})).into_response(),
            Err(_) => failure(
                StatusCode::INTERNAL_SERVER_ERROR,
                "message.server_identity_unavailable",
            ),
        };
    }
    Json(json!({"app":"atelier-kanban", "protocol":"atelier-local/1", "version":env!("CARGO_PKG_VERSION"), "pid":std::process::id(), "dataDirectory":state.prototype_paths[0]})).into_response()
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
                if card.fields.truthy("brainstormSession")? {
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
    database
        .call(move |store| {
            crate::infrastructure::connectors::retire_local_connectors(
                store,
                &mut |value, decrypt| cleanup_vault.protect(value, decrypt),
            )
        })
        .await?;
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
    #[cfg(windows)]
    let app = app.api_route(
        "/api/agent-providers/{id}/cli-update",
        Policy::ProjectExternal,
        post(agent_cli::update),
    );
    let app = app
        .api_route(
            "/api/extensions/{module}/operations/{id}/send",
            Policy::ProjectExternal,
            post(connector_effects::send),
        )
        .api_route(
            "/api/extensions/{module}/operations/{id}/reconcile",
            Policy::ProjectExternal,
            post(connector_effects::reconcile),
        )
        .api_route(
            "/api/cards/{id}/runs/start",
            Policy::External,
            post(runs::start),
        )
        .api_route(
            "/api/extensions/{id}",
            Policy::External,
            axum::routing::put(connectors::configure).delete(connectors::remove),
        )
        .api_route(
            "/api/extensions/{id}/invoke",
            Policy::External,
            post(connectors::invoke),
        )
        .api_route(
            "/api/extensions/{id}/tickets/reset/preview",
            Policy::ProjectExternal,
            post(connector_ticket_reset::preview),
        )
        .api_route(
            "/api/extensions/{id}/tickets/reset/apply",
            Policy::ProjectExternal,
            post(connector_ticket_reset::apply),
        )
        .api_route(
            "/api/queue",
            Policy::External,
            get(queue::get).post(queue::change),
        )
        .api_route(
            "/api/cards/{id}/automation",
            Policy::External,
            post(automation::change),
        )
        .api_route(
            "/api/cards/{id}/automation/stop",
            Policy::Cancellation,
            post(automation::stop),
        )
        .api_route("/api/queue/pause", Policy::Cancellation, post(queue::pause))
        .api_route(
            "/api/cards/{id}/verification/start",
            Policy::External,
            post(runs::verify),
        )
        .api_route(
            "/api/cards/{id}/runs/{run}/review/validate",
            Policy::External,
            post(reviews::validate),
        )
        .api_route(
            "/api/runs/{id}/stop",
            Policy::Cancellation,
            post(runs::stop),
        )
        .api_route("/api/runs/{id}/reply", Policy::Ordinary, post(runs::reply))
        .api_route("/api/runs/{id}/logs", Policy::Ordinary, get(runs::logs))
        .api_route(
            "/api/cards/{id}/services",
            Policy::Ordinary,
            get(services::get),
        )
        .api_route(
            "/api/cards/{id}/services/start",
            Policy::External,
            post(services::start),
        )
        .api_route(
            "/api/cards/{id}/services/restart",
            Policy::External,
            post(services::restart),
        )
        .api_route(
            "/api/cards/{id}/services/stop",
            Policy::Cancellation,
            post(services::stop),
        );
    let app = app
        .api_route("/api/health", Policy::Ordinary, get(health))
        .api_route(
            "/api/cards/{id}/forge/publication-push",
            Policy::External,
            post(git_push::publication_branch),
        )
        .api_route(
            "/api/cards/{id}/forge/publication-preview",
            Policy::External,
            post(forge_publication::preview),
        )
        .api_route(
            "/api/cards/{id}/forge/publication",
            Policy::External,
            post(forge_publication::create),
        )
        .api_route(
            "/api/cards/{id}/forge/publication-reconcile",
            Policy::External,
            post(forge_publication::reconcile),
        )
        .api_route(
            "/api/cards/{id}/forge/push-preview",
            Policy::External,
            post(git_push::preview),
        )
        .api_route(
            "/api/cards/{id}/forge/push",
            Policy::External,
            post(git_push::push),
        )
        .api_route(
            "/api/cards/{id}/forge/push-reconcile",
            Policy::External,
            post(git_push::reconcile),
        )
        .api_route(
            "/api/cards/{id}/commit/preview",
            Policy::External,
            post(commits::preview),
        )
        .api_route(
            "/api/cards/{id}/commit/changes",
            Policy::Ordinary,
            get(commits::changes),
        )
        .api_route(
            "/api/cards/{id}/commit/create",
            Policy::External,
            post(commits::create),
        )
        .api_route(
            "/api/cards/{id}/commit/reconcile",
            Policy::External,
            post(commits::reconcile),
        )
        .api_route("/api/events", Policy::Ordinary, get(events))
        .api_route(
            "/api/events/session",
            Policy::Ordinary,
            get(events::session),
        )
        .api_route("/api/time", Policy::Ordinary, any(time_tracking::create))
        .api_route(
            "/api/time/{id}",
            Policy::Ordinary,
            any(time_tracking::entry),
        )
        .api_route(
            "/api/time/{id}/{action}",
            Policy::Ordinary,
            any(time_tracking::action),
        )
        .api_route(
            "/api/time/{id}/stop",
            Policy::Cancellation,
            any(time_tracking::known_action),
        )
        .api_route(
            "/api/time/{id}/export-preview",
            Policy::External,
            any(time_tracking::known_action),
        )
        .api_route(
            "/api/time/{id}/export",
            Policy::External,
            any(time_tracking::known_action),
        )
        .api_route(
            "/api/time/{id}/reconcile",
            Policy::External,
            any(time_tracking::known_action),
        )
        .api_route(
            "/api/time/{id}/reset-export",
            Policy::External,
            any(time_tracking::known_action),
        )
        .api_route("/api/state", Policy::Ordinary, get(board))
        .api_route(
            "/api/jira/config",
            Policy::Ordinary,
            get(jira::get).put(jira::save),
        )
        .api_route(
            "/api/tempo/config",
            Policy::Ordinary,
            get(tempo::get).put(tempo::save),
        )
        .api_route("/api/extensions", Policy::External, get(connectors::get))
        .api_route(
            "/api/agent-providers/cli-update",
            Policy::Ordinary,
            get(agent_providers::cli_status),
        )
        .api_route(
            "/api/folders/choose",
            Policy::External,
            post(folders::choose),
        )
        .api_route(
            "/api/extensions/pending",
            Policy::External,
            get(connector_effects::pending),
        )
        .api_route(
            "/api/extensions/{module}/operations",
            Policy::ProjectExternal,
            get(connector_effects::list).post(connector_effects::preview),
        )
        .api_route(
            "/api/extensions/{module}/operations/{id}/cancel",
            Policy::ProjectExternal,
            post(connector_effects::cancel),
        )
        .api_route("/api/updates", Policy::Ordinary, get(updates::get))
        .api_route("/api/updates/check", Policy::External, get(updates::check))
        .api_route(
            "/api/updates/install",
            Policy::External,
            post(updates::action),
        )
        .api_route(
            "/api/updates/preview",
            Policy::External,
            post(updates::action),
        )
        .api_route(
            "/api/updates/apply",
            Policy::External,
            post(updates::action),
        )
        .api_route(
            "/api/updates/cancel",
            Policy::External,
            post(updates::action),
        )
        .api_route(
            "/api/updates/recover",
            Policy::Cancellation,
            post(updates::action),
        )
        .api_route(
            "/api/extensions/{id}/tickets/import",
            Policy::External,
            post(connectors::import_tickets),
        )
        .api_route(
            "/api/cards/{id}/connector-refresh",
            Policy::External,
            post(connectors::refresh_ticket),
        )
        .api_route(
            "/api/cards/{id}/jira/transitions",
            Policy::Ordinary,
            get(jira_transitions::choices),
        )
        .api_route(
            "/api/cards/{id}/jira/transition",
            Policy::External,
            post(jira_transitions::send),
        )
        .api_route(
            "/api/cards/{id}/jira/reconcile",
            Policy::External,
            post(jira_transitions::reconcile),
        )
        .api_route(
            "/api/cards/{id}/jira/refresh-preview",
            Policy::External,
            post(jira_refresh::preview),
        )
        .api_route(
            "/api/cards/{id}/jira/refresh-apply",
            Policy::External,
            post(jira_refresh::apply),
        )
        .api_route(
            "/api/cards/{id}/jira/create",
            Policy::External,
            post(jira_creation::create),
        )
        .api_route(
            "/api/cards/{id}/jira/creation-reconcile",
            Policy::External,
            post(jira_creation::reconcile),
        )
        .api_route(
            "/api/jira/creation-metadata",
            Policy::Ordinary,
            get(jira::creation_metadata),
        )
        .api_route("/api/jira/search", Policy::Ordinary, get(jira::search))
        .api_route(
            "/api/jira/boards/{id}/configuration",
            Policy::Ordinary,
            get(jira::board_configuration),
        )
        .api_route("/api/jira/import", Policy::External, post(jira::import))
        .api_route(
            "/api/forges",
            Policy::Ordinary,
            get(forge::configurations).put(forge::configure),
        )
        .api_route(
            "/api/forges/pull-request",
            Policy::Ordinary,
            get(forge::pull_request),
        )
        .api_route("/api/forges/checks", Policy::Ordinary, get(forge::checks))
        .api_route(
            "/api/cards/{id}/forge/draft",
            Policy::External,
            post(forge_draft::change),
        )
        .api_route(
            "/api/cards/{id}/forge/draft-reconcile",
            Policy::External,
            post(forge_draft::reconcile),
        )
        .api_route(
            "/api/forges/pull-requests",
            Policy::Ordinary,
            get(forge::pull_requests),
        )
        .api_route(
            "/api/cards/{id}/forge/attach",
            Policy::External,
            post(forge::attach),
        )
        .api_route(
            "/api/cards/{id}/forge/detach",
            Policy::Ordinary,
            post(forge::detach),
        )
        .api_route(
            "/api/jira/import-batch",
            Policy::External,
            post(jira::import_batch),
        )
        .api_route("/api/jira/issues/{id}", Policy::Ordinary, get(jira::detail))
        .api_route(
            "/api/backups",
            Policy::Ordinary,
            get(backups::list).post(backups::create),
        )
        .api_route(
            "/api/backups/restore",
            Policy::Ordinary,
            post(backups::restore),
        )
        .api_route(
            "/api/agent-providers",
            Policy::Ordinary,
            get(agent_providers::describe).post(agent_providers::save),
        )
        .api_route(
            "/api/agent-providers/discover",
            Policy::Ordinary,
            get(agent_providers::discover),
        )
        .api_route(
            "/api/agent-providers/{id}",
            Policy::Ordinary,
            axum::routing::delete(agent_providers::remove),
        )
        .api_route(
            "/api/agent-providers/{id}/diagnose",
            Policy::Ordinary,
            post(agent_providers::diagnose),
        )
        .api_route(
            "/api/settings",
            Policy::Ordinary,
            axum::routing::put(settings::update),
        )
        .api_route(
            "/api/integrations/defaults",
            Policy::Ordinary,
            axum::routing::put(integrations::choose),
        )
        .api_route(
            "/api/settings/tokens/{service}",
            Policy::Ordinary,
            axum::routing::delete(settings::remove_token),
        )
        .api_route("/api/projects", Policy::Ordinary, post(projects::create))
        .api_route(
            "/api/projects/{id}/kanban",
            Policy::Ordinary,
            axum::routing::put(kanban::save),
        )
        .api_route(
            "/api/repositories",
            Policy::ProjectExternal,
            get(repository_access::read)
                .post(repository_access::register)
                .delete(repository_access::revoke),
        )
        .api_route(
            "/api/projects/archive-tasks",
            Policy::Ordinary,
            post(projects::archive),
        )
        .api_route(
            "/api/projects/{id}",
            Policy::Ordinary,
            axum::routing::delete(projects::remove).patch(projects::update),
        )
        .api_route(
            "/api/projects/{id}/detect-launch",
            Policy::Ordinary,
            get(projects::detect).post(projects::detect),
        )
        .api_route("/api/cards", Policy::Ordinary, post(task_creation::create))
        .api_route(
            "/api/brainstorms",
            Policy::Ordinary,
            post(task_creation::create),
        )
        .api_route(
            "/api/cards/{id}/preparation/apply",
            Policy::Ordinary,
            post(preparation::apply),
        )
        .api_route(
            "/api/cards/{id}/brainstorm/convert",
            Policy::Ordinary,
            post(brainstorm::convert),
        )
        .api_route(
            "/api/templates",
            Policy::Ordinary,
            post(task_templates::create),
        )
        .api_route(
            "/api/templates/{id}",
            Policy::Ordinary,
            axum::routing::delete(task_templates::remove),
        )
        .api_route(
            "/api/cards/{id}",
            Policy::Ordinary,
            axum::routing::delete(card_lifecycle::remove).patch(task_editing::update),
        )
        .api_route(
            "/api/cards/{id}/archive",
            Policy::Ordinary,
            post(card_lifecycle::archive),
        )
        .api_route(
            "/api/cards/{id}/duplicate",
            Policy::Ordinary,
            post(duplicate),
        )
        .api_route("/api/cards/{id}/files", Policy::Ordinary, get(files::list))
        .api_route(
            "/api/cards/{id}/file",
            Policy::Ordinary,
            get(files::preview),
        )
        .api_route(
            "/api/cards/{id}/changes",
            Policy::Ordinary,
            get(files::changes),
        )
        .api_route(
            "/api/cards/{id}/file-diff",
            Policy::Ordinary,
            get(files::diff),
        )
        .api_route(
            "/api/cards/{id}/worktree",
            Policy::External,
            post(worktrees::create),
        )
        .api_route(
            "/api/cards/{id}/worktree/inspection",
            Policy::Ordinary,
            get(worktrees::inspect_status),
        )
        .api_route(
            "/api/cards/{id}/worktree/remove",
            Policy::External,
            post(worktrees::remove),
        )
        .api_route("/api/shutdown", Policy::Cancellation, post(stop))
        .fallback(unavailable)
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
