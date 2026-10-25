use super::{Shared, failure, responses::response};
use crate::infrastructure::{
    forge, forge_association, git_push as workflow, git_transport::HttpsAuthentication,
    store::Documents,
};
use atelier_core::contracts::{Card, ForgeWriteState, GitPushIntent, GitPushRequest, PullRequest};
use axum::{
    extract::{Path, Request, State},
    http::StatusCode,
    response::Response,
};

pub(super) async fn preview(
    State(state): State<Shared>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    let input: GitPushRequest = match super::jira_transitions::input(request).await {
        Ok(input) => input,
        Err(response) => return response,
    };
    if id != input.expected.id {
        return failure(StatusCode::BAD_REQUEST, "Carte de push incohérente.");
    }
    let Ok(_permit) = state.integration_reads.try_acquire() else {
        return failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "Les recherches distantes sont occupées. Réessaie dans un instant.",
        );
    };
    let mut stopped = state.shutdown.subscribe();
    let operation = async {
        let vault = state.vault.clone();
        let expected = input.expected.clone();
        let revision = input.revision;
        let (card, configuration, token) = state
            .database
            .call(move |store| {
                let card = forge_association::snapshot(store, &expected)?;
                let linked = card
                    .fields
                    .get::<PullRequest>("pullRequest")?
                    .ok_or("Associe une PR/MR avant de préparer le push.")?;
                if linked.revision != revision {
                    return Err(
                        "Relis la liaison de PR/MR après le changement de connexion.".into(),
                    );
                }
                let (configuration, token) =
                    forge::credentials(store, linked.kind, revision, &mut |value, decrypt| {
                        vault.protect(value, decrypt)
                    })?;
                Ok((card, configuration, token))
            })
            .await?;
        let preview = workflow::preview(
            &state.prototype_directory,
            &state.prototype_paths,
            &card,
            configuration,
            token,
        )
        .await?;
        state
            .database
            .call(move |store| workflow::save_preview(store, &card, input.revision, preview))
            .await
    };
    let stopping = *stopped.borrow();
    if stopping {
        return failure(StatusCode::CONFLICT, "Atelier est en cours d’arrêt.");
    }
    tokio::select! {result=tokio::time::timeout(std::time::Duration::from_secs(120),operation)=>response(result.unwrap_or_else(|_|Err("Aperçu du push trop long.".into()))),_=stopped.changed()=>failure(StatusCode::CONFLICT,"Atelier est en cours d’arrêt.")}
}
pub(super) async fn push(
    State(state): State<Shared>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    execute(state, id, request, false).await
}
pub(super) async fn reconcile(
    State(state): State<Shared>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    execute(state, id, request, true).await
}
async fn execute(state: Shared, id: String, request: Request, reconcile: bool) -> Response {
    let input: GitPushRequest = match super::jira_transitions::input(request).await {
        Ok(input) => input,
        Err(response) => return response,
    };
    if id != input.expected.id || !input.confirmed {
        return failure(
            StatusCode::BAD_REQUEST,
            "Confirme le commit et la branche à pousser.",
        );
    }
    let Ok(_permit) = state.integration_reads.try_acquire() else {
        return failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "Les recherches distantes sont occupées. Réessaie dans un instant.",
        );
    };
    let requested = input.clone();
    let operation = uuid::Uuid::new_v4().to_string();
    let card = state
        .database
        .call(move |store| {
            if !reconcile {
                return workflow::reserve(store, &requested, &operation);
            }
            let card = store
                .document::<Card>(Documents::Cards, &requested.expected.id)?
                .ok_or("Carte introuvable.")?;
            if serde_json::to_value(atelier_core::application::jira::card_snapshot(&card))?
                != serde_json::to_value(atelier_core::application::jira::card_snapshot(
                    &requested.expected,
                ))?
            {
                return Err("La tâche a changé. Recharge avant de vérifier le push.".into());
            }
            let intent = card
                .fields
                .get::<GitPushIntent>("gitPush")?
                .filter(|intent| intent.state == ForgeWriteState::Uncertain)
                .ok_or("Aucun push incertain à vérifier.")?;
            let configuration =
                forge::configuration(store, intent.preview.pull_request.kind, requested.revision)?;
            if configuration.base != intent.preview.pull_request.base_url {
                return Err("Reconnecte le site de forge du push incertain.".into());
            }
            Ok(card)
        })
        .await;
    let card = match card {
        Ok(card) => card,
        Err(error) => return response::<Card>(Err(error)),
    };
    let intent = match card.fields.get::<GitPushIntent>("gitPush") {
        Ok(Some(intent)) => intent,
        _ => {
            return failure(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Intention de push absente.",
            );
        }
    };
    let _ = state.changed.send(());
    let mut stopped = state.shutdown.subscribe();
    let work = async {
        let vault = state.vault.clone();
        let revision = input.revision;
        let kind = intent.preview.pull_request.kind;
        let (configuration, token) = state
            .database
            .call(move |store| {
                forge::credentials(store, kind, revision, &mut |value, decrypt| {
                    vault.protect(value, decrypt)
                })
            })
            .await?;
        let authentication = HttpsAuthentication::new(
            configuration.clone(),
            intent.preview.pull_request.repository.clone(),
            token,
        )?;
        if !reconcile {
            let prepared = workflow::prepare(
                &state.prototype_directory,
                &state.prototype_paths,
                &card,
                &configuration,
                &authentication,
            )
            .await?;
            let expected = card.clone();
            let operation = intent.id.clone();
            state
                .database
                .call(move |store| workflow::sending(store, &expected, &operation))
                .await?;
            let _ = state.changed.send(());
            authentication.push(prepared).await?;
        }
        let observed = authentication
            .remote_head(
                &intent.preview.origin,
                &format!("refs/heads/{}", intent.preview.pull_request.source_branch),
            )
            .await?;
        let card_id = id.clone();
        let operation = intent.id.clone();
        state
            .database
            .call(move |store| {
                workflow::record(
                    store,
                    &card_id,
                    &operation,
                    observed,
                    input.revision,
                    reconcile,
                )
            })
            .await
    };
    let stopping = *stopped.borrow();
    let result = if stopping {
        drop(work);
        Err("Atelier est en cours d’arrêt.".into())
    } else {
        tokio::select! {result=tokio::time::timeout(std::time::Duration::from_secs(300),work)=>result.unwrap_or_else(|_|Err("Push interrompu ou trop long. Relis la branche distante.".into())),_=stopped.changed()=>Err("Atelier est en cours d’arrêt.".into())}
    };
    let result = match result {
        Err(error) if !reconcile => {
            let operation = intent.id;
            match state
                .database
                .call(move |store| workflow::interrupted(store, &id, &operation))
                .await
            {
                Ok(_) => Err(error),
                Err(recording) => Err(recording),
            }
        }
        result => result,
    };
    let _ = state.changed.send(());
    response(result)
}
