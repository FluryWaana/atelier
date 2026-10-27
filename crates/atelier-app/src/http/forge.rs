use super::{Shared, failure};
use crate::infrastructure::forge;
use atelier_core::contracts::{ForgeKind, ForgeUpdate};
use axum::{
    extract::{Query, Request, State},
    http::StatusCode,
    response::Response,
};
use serde::Deserialize;

pub(super) async fn attach(
    State(state): State<Shared>,
    axum::extract::Path(id): axum::extract::Path<String>,
    request: Request,
) -> Response {
    use crate::infrastructure::forge_association as workflow;
    use atelier_core::contracts::PullRequestAttach;
    let input: PullRequestAttach = match super::jira_transitions::input(request).await {
        Ok(input) => input,
        Err(response) => return *response,
    };
    if id != input.expected.id {
        return failure(StatusCode::BAD_REQUEST, "Carte incohérente.");
    }
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
        let expected = input.clone();
        let (card, configuration) = state
            .database
            .call(move |store| workflow::prepare(store, &expected))
            .await?;
        let _directory = workflow::verify(
            &state.prototype_directory,
            &state.prototype_paths,
            &card,
            &configuration,
            &input.repository,
        )
        .await?;
        let vault = state.vault.clone();
        let kind = input.kind;
        let revision = input.revision;
        let (configuration, token) = state
            .database
            .call(move |store| {
                forge::credentials(store, kind, revision, &mut |value, decrypt| {
                    vault.protect(value, decrypt)
                })
            })
            .await?;
        let pr = forge::Forge::new(configuration.clone(), token, revision)?
            .pull_request(&input.repository, input.number)
            .await?;
        let _checked = workflow::verify(
            &state.prototype_directory,
            &state.prototype_paths,
            &card,
            &configuration,
            &input.repository,
        )
        .await?;
        state
            .database
            .call(move |store| workflow::attach(store, &input, pr))
            .await
    };
    tokio::select! {result=tokio::time::timeout(std::time::Duration::from_secs(120),operation)=>super::responses::response(result.unwrap_or_else(|_|Err("Association de PR/MR trop longue. Recharge la tâche avant de réessayer.".into()))),_=stopped.changed()=>failure(StatusCode::CONFLICT,"Atelier est en cours d’arrêt.")}
}
pub(super) async fn detach(
    State(state): State<Shared>,
    axum::extract::Path(id): axum::extract::Path<String>,
    request: Request,
) -> Response {
    let input: atelier_core::contracts::PullRequestDetach =
        match super::jira_transitions::input(request).await {
            Ok(input) => input,
            Err(response) => return *response,
        };
    if id != input.expected.id {
        return failure(StatusCode::BAD_REQUEST, "Carte incohérente.");
    }
    super::responses::response(
        state
            .database
            .call(move |store| crate::infrastructure::forge_association::detach(store, input))
            .await,
    )
}

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
        Err(response) => return *response,
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
    #[serde(default)]
    number: u64,
}
enum ReadOperation {
    PullRequest,
    List,
    Checks,
}
pub(super) async fn pull_request(
    State(state): State<Shared>,
    Query(query): Query<PullQuery>,
) -> Response {
    read_forge(state, query, ReadOperation::PullRequest).await
}
pub(super) async fn pull_requests(
    State(state): State<Shared>,
    Query(query): Query<PullQuery>,
) -> Response {
    read_forge(state, query, ReadOperation::List).await
}
pub(super) async fn checks(
    State(state): State<Shared>,
    Query(query): Query<PullQuery>,
) -> Response {
    read_forge(state, query, ReadOperation::Checks).await
}
async fn read_forge(state: Shared, query: PullQuery, operation: ReadOperation) -> Response {
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
    let reading = async {
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
        let client = forge::Forge::new(configuration, token, revision)?;
        let result = match operation {
            ReadOperation::List => {
                serde_json::to_value(client.list_pull_requests(&query.repository).await?)?
            }
            ReadOperation::Checks => {
                serde_json::to_value(client.checks(&query.repository, query.number).await?)?
            }
            ReadOperation::PullRequest => {
                serde_json::to_value(client.pull_request(&query.repository, query.number).await?)?
            }
        };
        state
            .database
            .call(move |store| forge::require_revision(store, revision))
            .await?;
        Ok(result)
    };
    tokio::select! {result=reading=>super::responses::response(result),_=stopped.changed()=>failure(StatusCode::CONFLICT,"Atelier est en cours d’arrêt.")}
}
