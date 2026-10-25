use super::{Shared, failure};
use crate::infrastructure::brainstorm::{self, Convert};
use axum::{
    extract::{Path, Request, State},
    http::StatusCode,
    response::Response,
};

pub(super) async fn convert(
    State(state): State<Shared>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    let input: Convert = match super::jira_transitions::input(request).await {
        Ok(input) => input,
        Err(response) => return response,
    };
    if id != input.expected.id {
        return failure(StatusCode::BAD_REQUEST, "Session incohérente.");
    }
    super::responses::response(
        state
            .database
            .call(move |store| brainstorm::convert(store, input))
            .await,
    )
}
