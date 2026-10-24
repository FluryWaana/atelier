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

mod agent_providers;
mod assets;
pub use assets::LEGACY_ASSETS;
mod backups;
mod card_lifecycle;
mod events;
mod files;
mod mutations;
mod projects;
#[cfg(windows)]
mod services;
mod settings;
mod task_creation;
mod task_editing;
mod task_templates;
mod time_tracking;
mod worktrees;

pub const CSP: &str = "default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self'; connect-src 'self'; img-src 'self' data:; frame-ancestors 'none'; base-uri 'none'; form-action 'self'";

struct AppState {
    database: Database,
    prototype_paths: [PathBuf; 2],
    prototype_directory: Arc<cap_std::fs::Dir>,
    vault: crate::infrastructure::token_vault::PrototypeVault,
    backups: Arc<crate::infrastructure::backups::BackupDirectory>,
    token: String,
    port: u16,
    shutdown: watch::Sender<bool>,
    changed: tokio::sync::broadcast::Sender<()>,
    file_reads: tokio::sync::Semaphore,
    mutations: Arc<tokio::sync::RwLock<()>>,
    #[cfg(windows)]
    services: Arc<crate::infrastructure::dev_servers::DevServers>,
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
    let mut response = authorize(&state, request, next).await;
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

async fn authorize(state: &AppState, request: Request, next: Next) -> Response {
    let host = header_value(&request, "host").unwrap_or("");
    if host != format!("127.0.0.1:{}", state.port) && host != format!("localhost:{}", state.port) {
        return failure(StatusCode::FORBIDDEN, "Hôte interdit.");
    }
    if request.headers().contains_key("origin")
        && header_value(&request, "origin") != Some(format!("http://{host}").as_str())
    {
        return failure(StatusCode::FORBIDDEN, "Origine interdite.");
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
                "Session expirée ou absente. Rouvre Atelier avec son lanceur.",
            );
        }
        if request.method() != Method::GET
            && (header_value(&request, "x-local-request") != Some("1")
                || header_value(&request, "content-type") != Some("application/json"))
        {
            return failure(StatusCode::FORBIDDEN, "Requête locale JSON requise.");
        }
        if request.method() != Method::GET && *state.shutdown.borrow() {
            return failure(StatusCode::CONFLICT, "Atelier est en cours d’arrêt.");
        }
    }
    if request.method() != Method::GET && path.starts_with("/api/") {
        let exclusive = path.ends_with("/worktree")
            || path.ends_with("/worktree/remove")
            || path.ends_with("/services/start")
            || path.ends_with("/services/stop");
        let changed = state.changed.clone();
        #[cfg(windows)]
        let services = state.services.clone();
        mutations::run(state.mutations.clone(), exclusive, async move {
            #[cfg(windows)]
            if let Some(response) = services::guard(&services, request.uri().path()) {
                return response;
            }
            let response = next.run(request).await;
            if response.status().is_success() {
                let _ = changed.send(());
            }
            response
        })
        .await
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
                "Identité du serveur indisponible.",
            ),
        };
    }
    Json(json!({"app":"atelier-kanban", "version":env!("CARGO_PKG_VERSION"), "pid":std::process::id(), "prototype":true})).into_response()
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
            "Lecture du stockage impossible.",
        ),
    }
}

async fn object(body: Body) -> std::result::Result<Document, (StatusCode, &'static str)> {
    // Node counts UTF-16 code units; the byte ceiling bounds allocation before the exact check.
    let bytes = to_bytes(body, 400_000)
        .await
        .map_err(|_| (StatusCode::PAYLOAD_TOO_LARGE, "Requête trop volumineuse."))?;
    let raw =
        std::str::from_utf8(&bytes).map_err(|_| (StatusCode::BAD_REQUEST, "JSON invalide."))?;
    if raw.encode_utf16().count() > 100_000 {
        return Err((StatusCode::PAYLOAD_TOO_LARGE, "Requête trop volumineuse."));
    }
    let value: &serde_json::value::RawValue =
        serde_json::from_str(if raw.is_empty() { "{}" } else { raw })
            .map_err(|_| (StatusCode::BAD_REQUEST, "JSON invalide."))?;
    if !value.get().starts_with('{') {
        return Err((StatusCode::BAD_REQUEST, "Objet JSON requis."));
    }
    serde_json::from_str(value.get()).map_err(|_| (StatusCode::BAD_REQUEST, "JSON invalide."))
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
        Err(_) => return failure(StatusCode::INTERNAL_SERVER_ERROR, "Horloge indisponible."),
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
        Ok(None) => failure(StatusCode::NOT_FOUND, "Carte introuvable."),
        Err(_) => failure(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Duplication de la carte impossible.",
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

async fn unavailable(request: Request) -> Response {
    let namespace = request
        .uri()
        .path()
        .strip_prefix("/api/")
        .and_then(|path| path.split('/').next());
    if matches!(
        namespace,
        Some(
            "agents"
                | "agent-providers"
                | "connectors"
                | "updates"
                | "cards"
                | "brainstorms"
                | "runs"
                | "queue"
                | "diagnostics"
                | "tickets"
                | "worktrees"
                | "dev-servers"
                | "folders"
                | "shutdown"
        )
    ) {
        failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "Cette fonction n’est pas encore disponible dans le prototype Rust.",
        )
    } else {
        failure(StatusCode::NOT_FOUND, "Route introuvable.")
    }
}

pub async fn serve(listener: TcpListener, directory: PathBuf, assets: PathBuf) -> Result<()> {
    if !listener.local_addr()?.ip().is_loopback() {
        return Err("Écoute distante interdite.".into());
    }
    let app = assets::router(&assets)?;
    let original_directory = std::path::absolute(&directory)?;
    let directory = PrototypeDirectory::open(&directory)?;
    let vault =
        crate::infrastructure::token_vault::PrototypeVault::new(directory.vault_service()?)?;
    let token = session::new_token()?;
    directory.write_session(&token)?;
    let database = directory.database()?;
    database
        .call(|store| store.transaction(atelier_core::application::time_tracking::initialize))
        .await?;
    let (shutdown, mut stopped) = watch::channel(false);
    let changed = tokio::sync::broadcast::channel(16).0;
    let state = Arc::new(AppState {
        database,
        prototype_paths: [directory.path.clone(), original_directory],
        prototype_directory: Arc::new(directory.opened_directory()?),
        vault,
        backups: Arc::new(directory.backups()?),
        token,
        port: listener.local_addr()?.port(),
        shutdown,
        #[cfg(windows)]
        services: crate::infrastructure::dev_servers::DevServers::new(changed.clone()),
        changed,
        file_reads: tokio::sync::Semaphore::new(2),
        mutations: Arc::new(tokio::sync::RwLock::new(())),
    });
    #[cfg(windows)]
    let app = app
        .route("/api/cards/{id}/services", get(services::get))
        .route("/api/cards/{id}/services/start", post(services::start))
        .route("/api/cards/{id}/services/stop", post(services::stop));
    let app = app
        .route("/api/health", get(health))
        .route("/api/events", get(events))
        .route("/api/time", any(time_tracking::create))
        .route("/api/time/{id}", any(time_tracking::entry))
        .route("/api/time/{id}/{action}", any(time_tracking::action))
        .route("/api/state", get(board))
        .route("/api/backups", get(backups::list).post(backups::create))
        .route("/api/backups/restore", post(backups::restore))
        .route(
            "/api/agent-providers",
            get(agent_providers::describe).post(agent_providers::save),
        )
        .route(
            "/api/agent-providers/{id}",
            axum::routing::delete(agent_providers::remove),
        )
        .route("/api/settings", axum::routing::put(settings::update))
        .route(
            "/api/settings/tokens/{service}",
            axum::routing::delete(settings::remove_token),
        )
        .route("/api/projects", post(projects::create))
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
        .route("/api/cards/{id}/worktree/remove", post(worktrees::remove))
        .route("/api/prototype/stop", post(stop))
        .fallback(unavailable)
        .layer(middleware::from_fn_with_state(state.clone(), security))
        .with_state(state.clone());
    let stop_events = state.shutdown.clone();
    let clock = tokio::spawn(time_tracking::run(state.clone()));
    let result = axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            tokio::select! { _ = stopped.changed() => {}, _ = tokio::signal::ctrl_c() => {} }
            let _ = stop_events.send(true);
        })
        .await;
    let _ = state.shutdown.send(true);
    let clock_result = clock.await;
    let _mutations = state.mutations.write().await;
    #[cfg(windows)]
    let services_result = state.services.close().await;
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
    #[cfg(windows)]
    services_result?;
    clock_result??;
    timers?;
    result?;
    Ok(())
}
