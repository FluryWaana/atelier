use super::{Request, Response, Shared, State, responses::response};
use crate::infrastructure::tempo;
use atelier_core::contracts::TempoConfigurationUpdate;

pub(super) async fn get(State(state): State<Shared>) -> Response {
    let vault = state.vault.clone();
    response(
        state
            .database
            .call(move |store| {
                tempo::describe(store, &mut |value, decrypt| vault.protect(value, decrypt))
            })
            .await,
    )
}
pub(super) async fn save(State(state): State<Shared>, request: Request) -> Response {
    let input: TempoConfigurationUpdate = match super::jira_transitions::input(request).await {
        Ok(input) => input,
        Err(response) => return *response,
    };
    let vault = state.vault.clone();
    response(
        state
            .database
            .call(move |store| {
                tempo::configure(store, input, &mut |value, decrypt| {
                    vault.protect(value, decrypt)
                })
            })
            .await,
    )
}
