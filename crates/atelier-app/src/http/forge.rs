use super::{Shared, failure};
use crate::infrastructure::forge;
use atelier_core::contracts::{ForgeKind, ForgeUpdate};
use axum::{
    extract::{Query, Request, State},
    http::StatusCode,
    response::Response,
};
use serde::Deserialize;

pub(super) async fn configurations(State(state): State<Shared>) -> Response {
    let vault = state.vault.clone();
    super::responses::response(
        state
            .database
            .call(move |store| {
                forge::public(store, &mut |value, decrypt| vault.protect(value, decrypt))
            })
            .await,
    )
}
pub(super) async fn configure(State(state): State<Shared>, request: Request) -> Response {
    let input: ForgeUpdate = match super::jira_transitions::input(request).await {
        Ok(input) => input,
        Err(response) => return response,
    };
    let vault = state.vault.clone();
    super::responses::response(
        state
            .database
            .call(move |store| {
                forge::configure(store, input, &mut |value, decrypt| {
                    vault.protect(value, decrypt)
                })
            })
            .await,
    )
}
#[derive(Deserialize)]
pub(super) struct PullQuery {
    revision: u64,
    kind: ForgeKind,
    repository: String,
    number: u64,
}
pub(super) async fn pull_request(
    State(state): State<Shared>,
    Query(query): Query<PullQuery>,
) -> Response {
    let Ok(_permit) = state.integration_reads.try_acquire() else {
        return failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "Les recherches distantes sont occupées. Réessaie dans un instant.",
        );
    };
    let mut stopped = state.shutdown.subscribe();
    if *stopped.borrow() {
        return failure(StatusCode::CONFLICT, "Atelier est en cours d’arrêt.");
    }
    let operation = async {
        let vault = state.vault.clone();
        let revision = query.revision;
        let kind = query.kind;
        let (configuration, token) = state
            .database
            .call(move |store| {
                forge::credentials(store, kind, revision, &mut |value, decrypt| {
                    vault.protect(value, decrypt)
                })
            })
            .await?;
        let result = forge::Forge::new(configuration, token, revision)?
            .pull_request(&query.repository, query.number)
            .await?;
        state
            .database
            .call(move |store| forge::require_revision(store, revision))
            .await?;
        Ok(result)
    };
    tokio::select! {result=operation=>super::responses::response(result),_=stopped.changed()=>failure(StatusCode::CONFLICT,"Atelier est en cours d’arrêt.")}
}
