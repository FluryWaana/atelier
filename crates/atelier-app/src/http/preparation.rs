use super::{Shared, failure};
use atelier_core::application::preparation::Apply;
use axum::{
    extract::{Path, Request, State},
    http::StatusCode,
    response::Response,
};
pub(super) async fn apply(
    State(state): State<Shared>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    let input: Apply = match super::jira_transitions::input(request).await {
        Ok(input) => input,
        Err(response) => return response,
    };
    if id != input.expected.id {
        return failure(StatusCode::BAD_REQUEST, "Carte incohérente.");
    }
    super::jira::response(
        state
            .database
            .call(move |store| crate::infrastructure::preparation::apply(store, input))
            .await,
    )
}
