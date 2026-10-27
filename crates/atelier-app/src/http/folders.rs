use super::{Shared, failure, responses::response};
use axum::{
    extract::{Request, State},
    http::StatusCode,
    response::Response,
};
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    confirmed: bool,
}
pub(super) async fn choose(State(state): State<Shared>, request: Request) -> Response {
    if !state.native {
        return failure(
            StatusCode::CONFLICT,
            "Le sélecteur est disponible depuis le lanceur natif.",
        );
    }
    let input: Input = match super::jira_transitions::input(request).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    if !input.confirmed {
        return failure(
            StatusCode::BAD_REQUEST,
            "Confirme l’ouverture du sélecteur de dossiers.",
        );
    }
    let mut shutdown = state.shutdown.subscribe();
    if *shutdown.borrow() {
        return failure(StatusCode::CONFLICT, "Atelier est en cours d’arrêt.");
    }
    let result = tokio::select! {
        result=crate::infrastructure::folders::choose()=>result,
        _=shutdown.changed()=>Err("Sélecteur interrompu par l’arrêt d’Atelier.".into())
    };
    response(result.map(|path| serde_json::json!({"path":path})))
}
