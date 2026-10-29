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

async fn authorize(state: Shared, request: Request, next: Next) -> Response {
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
        let event_session = (path == "/api/events")
            .then(|| Query::<events::Credentials>::try_from_uri(request.uri()).ok())
            .flatten()
            .and_then(|query| query.0.session);
        let token = header_value(&request, "authorization")
            .map(|value| value.strip_prefix("Bearer ").unwrap_or(value))
            .filter(|value| !value.is_empty())
            .or(event_session.as_deref())
            .unwrap_or("");
        if !session::same_secret(token, &state.token) {
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
    if request.method() != Method::GET && path.starts_with("/api/") {
        let exclusive = path.ends_with("/jira/create")
            || path.ends_with("/connector-refresh")
            || path.starts_with("/api/updates/")
            || path.starts_with("/api/extensions")
            || (path.starts_with("/api/time/")
                && ["/export-preview", "/export", "/reconcile", "/reset-export"]
                    .iter()
                    .any(|suffix| path.ends_with(suffix)))
            || path.ends_with("/commit/preview")
            || path.ends_with("/commit/create")
            || path.ends_with("/commit/reconcile")
            || path.ends_with("/jira/creation-reconcile")
            || path.ends_with("/jira/refresh-preview")
            || path.ends_with("/jira/refresh-apply")
            || path.ends_with("/jira/transition")
            || path.ends_with("/jira/reconcile")
            || path == "/api/jira/import"
            || path == "/api/jira/import-batch"
            || path.ends_with("/forge/attach")
            || path.ends_with("/forge/draft")
            || path.ends_with("/forge/draft-reconcile")
            || path.ends_with("/forge/push-preview")
            || path.ends_with("/forge/push")
            || path.ends_with("/forge/push-reconcile")
            || path.ends_with("/forge/publication-preview")
            || path.ends_with("/forge/publication")
            || path.ends_with("/forge/publication-reconcile")
            || path.ends_with("/forge/publication-push")
            || path.ends_with("/worktree")
            || path.ends_with("/worktree/remove")
            || path.ends_with("/services/start")
            || path.ends_with("/services/stop")
            || path.ends_with("/services/restart")
            || path.ends_with("/runs/start")
            || path.ends_with("/verification/start")
            || path.ends_with("/automation")
            || path == "/api/queue"
            || path == "/api/repositories"
            || path == "/api/folders/choose"
            || (path.starts_with("/api/extensions/") && path.contains("/operations"))
            || path.ends_with("/review/validate")
            || (path.starts_with("/api/runs/") && path.ends_with("/stop"));
        let exclusive = exclusive
            || (path.starts_with("/api/agent-providers/") && path.ends_with("/cli-update"));
        let cancellation = request.method() == Method::POST && mutations::is_cancellation(path);
        let gate = state.mutations.clone();
        let changed = state.changed.clone();
        let services = state.services.clone();
        let runs = state.runs.clone();
        let operation = async move {
            if !cancellation {
                if let Some(response) = updates::guard(state.installation_root.as_deref()) {
                    return response;
                }
                let guard_path = if request.uri().path() == "/api/repositories"
                    || (request.uri().path().starts_with("/api/agent-providers/")
                        && request.uri().path().ends_with("/cli-update"))
                    || (request.uri().path().starts_with("/api/extensions/")
                        && (request.uri().path().contains("/operations")
                            || request.uri().path().contains("/tickets/reset/")))
                {
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
            mutations::run(gate, exclusive, operation).await
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
    // Node counts UTF-16 code units; the byte ceiling bounds allocation before the exact check.
    let bytes = to_bytes(body, 400_000)
        .await
        .map_err(|_| (StatusCode::PAYLOAD_TOO_LARGE, "message.request_too_large"))?;
    let raw = std::str::from_utf8(&bytes)
        .map_err(|_| (StatusCode::BAD_REQUEST, "message.invalid_json"))?;
    if raw.encode_utf16().count() > 100_000 {
        return Err((StatusCode::PAYLOAD_TOO_LARGE, "message.request_too_large"));
    }
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
    let app = app.route(
        "/api/agent-providers/{id}/cli-update",
        post(agent_cli::update),
    );
    let app = app
        .route(
            "/api/extensions/{module}/operations/{id}/send",
            post(connector_effects::send),
        )
        .route(
            "/api/extensions/{module}/operations/{id}/reconcile",
            post(connector_effects::reconcile),
        )
        .route("/api/cards/{id}/runs/start", post(runs::start))
        .route("/api/extensions/probe", post(connectors::probe))
        .route("/api/extensions/install", post(connectors::install))
        .route(
            "/api/extensions/{id}",
            axum::routing::put(connectors::configure).delete(connectors::remove),
        )
        .route("/api/extensions/{id}/invoke", post(connectors::invoke))
        .route(
            "/api/extensions/{id}/tickets/reset/preview",
            post(connector_ticket_reset::preview),
        )
        .route(
            "/api/extensions/{id}/tickets/reset/apply",
            post(connector_ticket_reset::apply),
        )
        .route("/api/queue", get(queue::get).post(queue::change))
        .route("/api/cards/{id}/automation", post(automation::change))
        .route("/api/cards/{id}/automation/stop", post(automation::stop))
        .route("/api/queue/pause", post(queue::pause))
        .route("/api/cards/{id}/verification/start", post(runs::verify))
        .route(
            "/api/cards/{id}/runs/{run}/review/validate",
            post(reviews::validate),
        )
        .route("/api/runs/{id}/stop", post(runs::stop))
        .route("/api/runs/{id}/reply", post(runs::reply))
        .route("/api/runs/{id}/logs", get(runs::logs))
        .route("/api/cards/{id}/services", get(services::get))
        .route("/api/cards/{id}/services/start", post(services::start))
        .route("/api/cards/{id}/services/restart", post(services::restart))
        .route("/api/cards/{id}/services/stop", post(services::stop));
    let app = app
        .route("/api/health", get(health))
        .route(
            "/api/cards/{id}/forge/publication-push",
            post(git_push::publication_branch),
        )
        .route(
            "/api/cards/{id}/forge/publication-preview",
            post(forge_publication::preview),
        )
        .route(
            "/api/cards/{id}/forge/publication",
            post(forge_publication::create),
        )
        .route(
            "/api/cards/{id}/forge/publication-reconcile",
            post(forge_publication::reconcile),
        )
        .route(
            "/api/cards/{id}/forge/push-preview",
            post(git_push::preview),
        )
        .route("/api/cards/{id}/forge/push", post(git_push::push))
        .route(
            "/api/cards/{id}/forge/push-reconcile",
            post(git_push::reconcile),
        )
        .route("/api/cards/{id}/commit/preview", post(commits::preview))
        .route("/api/cards/{id}/commit/changes", get(commits::changes))
        .route("/api/cards/{id}/commit/create", post(commits::create))
        .route("/api/cards/{id}/commit/reconcile", post(commits::reconcile))
        .route("/api/events", get(events))
        .route("/api/time", any(time_tracking::create))
        .route("/api/time/{id}", any(time_tracking::entry))
        .route("/api/time/{id}/{action}", any(time_tracking::action))
        .route("/api/state", get(board))
        .route("/api/jira/config", get(jira::get).put(jira::save))
        .route("/api/tempo/config", get(tempo::get).put(tempo::save))
        .route("/api/extensions", get(connectors::get))
        .route(
            "/api/agent-providers/cli-update",
            get(agent_providers::cli_status),
        )
        .route("/api/folders/choose", post(folders::choose))
        .route("/api/extensions/pending", get(connector_effects::pending))
        .route(
            "/api/extensions/{module}/operations",
            get(connector_effects::list).post(connector_effects::preview),
        )
        .route(
            "/api/extensions/{module}/operations/{id}/cancel",
            post(connector_effects::cancel),
        )
        .route("/api/updates", get(updates::get))
        .route("/api/updates/preview", post(updates::action))
        .route("/api/updates/apply", post(updates::action))
        .route("/api/updates/cancel", post(updates::action))
        .route("/api/updates/recover", post(updates::action))
        .route(
            "/api/extensions/{id}/tickets/import",
            post(connectors::import_tickets),
        )
        .route(
            "/api/cards/{id}/connector-refresh",
            post(connectors::refresh_ticket),
        )
        .route(
            "/api/cards/{id}/jira/transitions",
            get(jira_transitions::choices),
        )
        .route(
            "/api/cards/{id}/jira/transition",
            post(jira_transitions::send),
        )
        .route(
            "/api/cards/{id}/jira/reconcile",
            post(jira_transitions::reconcile),
        )
        .route(
            "/api/cards/{id}/jira/refresh-preview",
            post(jira_refresh::preview),
        )
        .route(
            "/api/cards/{id}/jira/refresh-apply",
            post(jira_refresh::apply),
        )
        .route("/api/cards/{id}/jira/create", post(jira_creation::create))
        .route(
            "/api/cards/{id}/jira/creation-reconcile",
            post(jira_creation::reconcile),
        )
        .route("/api/jira/creation-metadata", get(jira::creation_metadata))
        .route("/api/jira/search", get(jira::search))
        .route(
            "/api/jira/boards/{id}/configuration",
            get(jira::board_configuration),
        )
        .route("/api/jira/import", post(jira::import))
        .route(
            "/api/forges",
            get(forge::configurations).put(forge::configure),
        )
        .route("/api/forges/pull-request", get(forge::pull_request))
        .route("/api/forges/checks", get(forge::checks))
        .route("/api/cards/{id}/forge/draft", post(forge_draft::change))
        .route(
            "/api/cards/{id}/forge/draft-reconcile",
            post(forge_draft::reconcile),
        )
        .route("/api/forges/pull-requests", get(forge::pull_requests))
        .route("/api/cards/{id}/forge/attach", post(forge::attach))
        .route("/api/cards/{id}/forge/detach", post(forge::detach))
        .route("/api/jira/import-batch", post(jira::import_batch))
        .route("/api/jira/issues/{id}", get(jira::detail))
        .route("/api/backups", get(backups::list).post(backups::create))
        .route("/api/backups/restore", post(backups::restore))
        .route(
            "/api/agent-providers",
            get(agent_providers::describe).post(agent_providers::save),
        )
        .route(
            "/api/agent-providers/discover",
            get(agent_providers::discover),
        )
        .route(
            "/api/agent-providers/{id}",
            axum::routing::delete(agent_providers::remove),
        )
        .route(
            "/api/agent-providers/{id}/diagnose",
            post(agent_providers::diagnose),
        )
        .route("/api/settings", axum::routing::put(settings::update))
        .route(
            "/api/integrations/defaults",
            axum::routing::put(integrations::choose),
        )
        .route(
            "/api/settings/tokens/{service}",
            axum::routing::delete(settings::remove_token),
        )
        .route("/api/projects", post(projects::create))
        .route(
            "/api/projects/{id}/kanban",
            axum::routing::put(kanban::save),
        )
        .route(
            "/api/repositories",
            get(repository_access::read)
                .post(repository_access::register)
                .delete(repository_access::revoke),
        )
        .route("/api/projects/archive-tasks", post(projects::archive))
        .route(
            "/api/projects/{id}",
            axum::routing::delete(projects::remove).patch(projects::update),
        )
        .route(
            "/api/projects/{id}/detect-launch",
            get(projects::detect).post(projects::detect),
        )
        .route("/api/cards", post(task_creation::create))
        .route("/api/brainstorms", post(task_creation::create))
        .route(
            "/api/cards/{id}/preparation/apply",
            post(preparation::apply),
        )
        .route(
            "/api/cards/{id}/brainstorm/convert",
            post(brainstorm::convert),
        )
        .route("/api/templates", post(task_templates::create))
        .route(
            "/api/templates/{id}",
            axum::routing::delete(task_templates::remove),
        )
        .route(
            "/api/cards/{id}",
            axum::routing::delete(card_lifecycle::remove).patch(task_editing::update),
        )
        .route("/api/cards/{id}/archive", post(card_lifecycle::archive))
        .route("/api/cards/{id}/duplicate", post(duplicate))
        .route("/api/cards/{id}/files", get(files::list))
        .route("/api/cards/{id}/file", get(files::preview))
        .route("/api/cards/{id}/changes", get(files::changes))
        .route("/api/cards/{id}/file-diff", get(files::diff))
        .route("/api/cards/{id}/worktree", post(worktrees::create))
        .route(
            "/api/cards/{id}/worktree/inspection",
            get(worktrees::inspect_status),
        )
        .route("/api/cards/{id}/worktree/remove", post(worktrees::remove))
        .route("/api/shutdown", post(stop))
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
