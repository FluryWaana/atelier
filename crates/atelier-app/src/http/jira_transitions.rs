use super::{Shared, failure, object, responses::response};
use crate::{
    Result,
    infrastructure::{jira, jira_transition as workflow},
};
use atelier_core::{
    application::jira::JiraFailure,
    contracts::{
        Card, JiraReconciliation, JiraTransitionIntent, JiraTransitionRequest,
        JiraWriteState as Phase,
    },
};
use axum::{
    extract::{Path, Query, Request, State},
    http::StatusCode,
    response::Response,
};
use serde::{Deserialize, de::DeserializeOwned};
use std::time::Duration;
#[derive(Deserialize)]
pub(super) struct Revision {
    revision: u64,
}
pub(super) async fn input<T: DeserializeOwned>(
    request: Request,
) -> std::result::Result<T, Response> {
    let body = object(request.into_body())
        .await
        .map_err(|(status, message)| failure(status, message))?;
    serde_json::from_str(
        &serde_json::to_string(&body)
            .map_err(|_| failure(StatusCode::BAD_REQUEST, "Paramètres Jira invalides."))?,
    )
    .map_err(|_| failure(StatusCode::BAD_REQUEST, "Paramètres Jira invalides."))
}
pub(super) async fn client(state: &Shared, revision: u64) -> Result<jira::Jira> {
    let vault = state.vault.clone();
    let (configuration, token) = state
        .database
        .call(move |store| {
            jira::credentials(store, revision, &mut |value, decrypt| {
                vault.protect(value, decrypt)
            })
        })
        .await?;
    jira::Jira::new(configuration, token)
}
pub(super) async fn choices(
    State(state): State<Shared>,
    Path(id): Path<String>,
    Query(query): Query<Revision>,
) -> Response {
    let mut stopped = state.shutdown.subscribe();
    if *stopped.borrow() {
        return failure(StatusCode::CONFLICT, "Atelier est en cours d’arrêt.");
    }
    let Ok(_permit) = state.integration_reads.try_acquire() else {
        return failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "Les recherches distantes sont occupées. Réessaie dans un instant.",
        );
    };
    let operation = async {
        let (expected, issue) = state
            .database
            .call(move |store| workflow::snapshot(store, &id, query.revision))
            .await?;
        let client = client(&state, query.revision).await?;
        let result = client.transitions(&issue).await?;
        state
            .database
            .call(move |store| workflow::revalidate(store, &expected, query.revision))
            .await?;
        Ok(result)
    };
    tokio::select! {
        result=tokio::time::timeout(Duration::from_secs(60),operation)=>response(result.unwrap_or_else(|_|Err("Lecture des transitions Jira trop longue.".into()))),
        _=stopped.changed()=>failure(StatusCode::CONFLICT,"Atelier est en cours d’arrêt."),
    }
}
pub(super) async fn send(
    State(state): State<Shared>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    let input: JiraTransitionRequest = match input(request).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    if input.expected.id != id {
        return failure(StatusCode::BAD_REQUEST, "Carte Jira incohérente.");
    }
    let Ok(_permit) = state.integration_reads.try_acquire() else {
        return failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "Les recherches distantes sont occupées. Réessaie dans un instant.",
        );
    };
    let operation = uuid::Uuid::new_v4().to_string();
    let reserved = operation.clone();
    let prepared = input.clone();
    let issue = match state
        .database
        .call(move |store| {
            let now = time::OffsetDateTime::now_utc()
                .format(&time::format_description::well_known::Rfc3339)?;
            workflow::reserve(store, &prepared, &reserved, &now)
        })
        .await
    {
        Ok(issue) => issue,
        Err(error) => return response::<Card>(Err(error)),
    };
    let mut stopped = state.shutdown.subscribe();
    let ready = {
        let preparing = async {
            let client = client(&state, input.revision).await?;
            let choices = client.transitions(&issue).await?;
            let card = id.clone();
            let reservation = operation.clone();
            state
                .database
                .call(move |store| workflow::sending(store, &card, &reservation, &choices))
                .await?;
            Ok::<_, crate::Error>(client)
        };
        let stopping = *stopped.borrow();
        if stopping {
            Err("Atelier est en cours d’arrêt.".into())
        } else {
            tokio::select! {result=tokio::time::timeout(Duration::from_secs(60),preparing)=>result.unwrap_or_else(|_|Err("Lecture des transitions Jira trop longue.".into())),_=stopped.changed()=>Err("Atelier est en cours d’arrêt.".into())}
        }
    };
    let client = match ready {
        Ok(client) => client,
        Err(error) => {
            let result = state
                .database
                .call(move |store| {
                    // A cancelled SQLite job may already have recorded Sending; preserve that uncertainty.
                    let card = store
                        .document::<Card>(crate::infrastructure::store::Documents::Cards, &id)?
                        .ok_or("Carte introuvable.")?;
                    let intent = card
                        .fields
                        .get::<JiraTransitionIntent>("jiraTransition")?
                        .ok_or("Opération Jira absente.")?;
                    let phase = if intent.state == Phase::Preparing {
                        Phase::Failed
                    } else {
                        Phase::Uncertain
                    };
                    workflow::complete(store, &id, &operation, phase, None)
                })
                .await;
            let _ = state.changed.send(());
            return match result {
                Ok(_) => response::<Card>(Err(error)),
                Err(error) => response::<Card>(Err(error)),
            };
        }
    };
    let sending = async {
        client.send_transition(&issue, &input.transition).await?;
        client.status(&issue).await
    };
    let stopping = *stopped.borrow();
    let observed = if stopping {
        None
    } else {
        tokio::select! {result=tokio::time::timeout(Duration::from_secs(60),sending)=>result.ok().and_then(std::result::Result::ok),_=stopped.changed()=>None}
    };
    let phase = if observed
        .as_ref()
        .is_some_and(|status| status.id == input.transition.target.id)
    {
        Phase::Confirmed
    } else {
        Phase::Uncertain
    };
    let result = state
        .database
        .call(move |store| workflow::complete(store, &id, &operation, phase, observed))
        .await;
    let _ = state.changed.send(());
    response(result)
}
pub(super) async fn reconcile(
    State(state): State<Shared>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    let input: JiraReconciliation = match input(request).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    if input.expected.id != id {
        return failure(StatusCode::BAD_REQUEST, "Carte Jira incohérente.");
    }
    let Ok(_permit) = state.integration_reads.try_acquire() else {
        return failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "Les recherches distantes sont occupées. Réessaie dans un instant.",
        );
    };
    let intent = match input
        .expected
        .fields
        .get::<JiraTransitionIntent>("jiraTransition")
    {
        Ok(Some(intent)) if intent.state == Phase::Uncertain => intent,
        _ => {
            return failure(
                StatusCode::CONFLICT,
                "Aucun envoi Jira incertain à vérifier.",
            );
        }
    };
    let mut stopped = state.shutdown.subscribe();
    if *stopped.borrow() {
        return failure(StatusCode::CONFLICT, "Atelier est en cours d’arrêt.");
    }
    let operation = async {
        let expected = input.expected.clone();
        let (card, issue) = state
            .database
            .call(move |store| {
                workflow::revalidate(store, &expected, input.revision)?;
                workflow::snapshot(store, &id, input.revision)
            })
            .await?;
        let observed = client(&state, input.revision).await?.status(&issue).await?;
        let phase = if intent.from.id != intent.transition.target.id
            && observed.id == intent.transition.target.id
        {
            Phase::Confirmed
        } else if input.dismiss {
            Phase::Dismissed
        } else {
            Phase::Uncertain
        };
        state
            .database
            .call(move |store| {
                workflow::revalidate(store, &card, input.revision)?;
                workflow::complete(store, &card.id, &intent.id, phase, Some(observed))
            })
            .await
    };
    let result = tokio::select! {result=tokio::time::timeout(Duration::from_secs(60),operation)=>result.unwrap_or_else(|_|Err("Vérification Jira trop longue.".into())),_=stopped.changed()=>Err(JiraFailure::OperationConflict("Atelier est en cours d’arrêt.").into())};
    let _ = state.changed.send(());
    response(result)
}
