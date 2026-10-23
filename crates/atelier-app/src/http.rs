use crate::{
    Result,
    infrastructure::{database::Database, prototype::PrototypeDirectory, session},
};
use atelier_core::{
    application::{create_card, duplicate_card},
    contracts::{Board, Card},
    document::Document,
};
use axum::{
    Json, Router,
    body::{Body, to_bytes},
    extract::{Path, Query, Request, State},
    http::{Method, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, get_service, post},
};
use serde::Deserialize;
use serde_json::json;
use std::{path::PathBuf, sync::Arc};
use tokio::{net::TcpListener, sync::watch};
use tower_http::services::ServeFile;

pub const CSP: &str = "default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self'; connect-src 'self'; img-src 'self' data:; frame-ancestors 'none'; base-uri 'none'; form-action 'self'";

struct AppState {
    database: Database,
    token: String,
    port: u16,
    shutdown: watch::Sender<bool>,
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
        let token = header_value(&request, "authorization")
            .and_then(|value| value.strip_prefix("Bearer "))
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
    next.run(request).await
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
    match state.database.cards().await {
        Ok(cards) => Json(Board { cards }).into_response(),
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

async fn create(State(state): State<Shared>, request: Request) -> Response {
    let body = match object(request.into_body()).await {
        Ok(body) => body,
        Err((status, message)) => return failure(status, message),
    };
    let timestamp = match timestamp() {
        Ok(value) => value,
        Err(_) => return failure(StatusCode::INTERNAL_SERVER_ERROR, "Horloge indisponible."),
    };
    let card = match create_card(&body, uuid::Uuid::new_v4().to_string(), timestamp) {
        Ok(card) => card,
        Err(message) => return failure(StatusCode::BAD_REQUEST, message),
    };
    match state.database.put_card(card).await {
        Ok(card) => Json(card).into_response(),
        Err(_) => failure(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Écriture du stockage impossible.",
        ),
    }
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

pub async fn serve(listener: TcpListener, directory: PathBuf, assets: PathBuf) -> Result<()> {
    if !listener.local_addr()?.ip().is_loopback() {
        return Err("Écoute distante interdite.".into());
    }
    let mut app = Router::new();
    for entry in std::fs::read_dir(&assets)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let allowed = name == "index.html"
            || name == "atelier-loader.js"
            || (name.starts_with("atelier-ui-")
                && (name.ends_with(".js") || name.ends_with(".wasm")))
            || (name.starts_with("style-") && name.ends_with(".css"));
        if !allowed {
            continue;
        }
        if !entry.file_type()?.is_file() {
            return Err("Asset non régulier interdit.".into());
        }
        let path = if name == "index.html" {
            "/".to_owned()
        } else {
            format!("/{name}")
        };
        app = app.route(&path, get_service(ServeFile::new(entry.path())));
    }
    let directory = PrototypeDirectory::open(&directory)?;
    let token = session::new_token()?;
    session::write(&directory.path, &token)?;
    let database = Database::open(&directory.path.join("kanban.sqlite"))?;
    let (shutdown, mut stopped) = watch::channel(false);
    let state = Arc::new(AppState {
        database,
        token,
        port: listener.local_addr()?.port(),
        shutdown,
    });
    let app = app
        .route("/api/health", get(health))
        .route("/api/state", get(board))
        .route("/api/cards", post(create))
        .route("/api/cards/{id}/duplicate", post(duplicate))
        .route("/api/prototype/stop", post(stop))
        .layer(middleware::from_fn_with_state(state.clone(), security))
        .with_state(state.clone());
    let result = axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            tokio::select! { _ = stopped.changed() => {}, _ = tokio::signal::ctrl_c() => {} }
        })
        .await;
    state.database.close().await?;
    drop(directory);
    result?;
    Ok(())
}
