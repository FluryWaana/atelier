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
            "message.the_folder_picker_is_available_through_the_native_launcher",
        );
    }
    let input: Input = match super::input::parse(request).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    if !input.confirmed {
        return failure(
            StatusCode::BAD_REQUEST,
            "message.confirm_opening_the_folder_picker",
        );
    }
    let mut shutdown = state.shutdown.subscribe();
    if *shutdown.borrow() {
        return failure(StatusCode::CONFLICT, "message.atelier_is_shutting_down");
    }
    let result = tokio::select! {
        result=crate::infrastructure::folders::choose()=>result,
        _=shutdown.changed()=>Err("message.folder_picker_interrupted_by_atelier_shutdown".into())
    };
    response(result.map(|path| serde_json::json!({"path":path})))
}
