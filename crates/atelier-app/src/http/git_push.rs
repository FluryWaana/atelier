use super::{Shared, failure, responses::response};
use crate::infrastructure::{
    forge, forge_association, git_push as workflow, git_transport::HttpsAuthentication,
    store::Documents,
};
use atelier_core::contracts::{
    Card, ForgePublicationRequest, ForgeWriteState, GitPushIntent, GitPushRequest, PullRequest,
};
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
        Err(response) => return *response,
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
    request_input(state, id, request, false).await
}
pub(super) async fn reconcile(
    State(state): State<Shared>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    request_input(state, id, request, true).await
}
pub(super) async fn publication_branch(
    State(state): State<Shared>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    let publication: ForgePublicationRequest = match super::jira_transitions::input(request).await {
        Ok(input) => input,
        Err(response) => return *response,
    };
    let input = GitPushRequest {
        expected: publication.expected.clone(),
        revision: publication.revision,
        confirmed: publication.confirmed,
    };
    execute(state, id, input, false, Some(publication)).await
}
async fn request_input(state: Shared, id: String, request: Request, reconcile: bool) -> Response {
    let input: GitPushRequest = match super::jira_transitions::input(request).await {
        Ok(input) => input,
        Err(response) => return *response,
    };
    execute(state, id, input, reconcile, None).await
}
async fn execute(
    state: Shared,
    id: String,
    input: GitPushRequest,
    reconcile: bool,
    publication: Option<ForgePublicationRequest>,
) -> Response {
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
    response(perform(&state, id, input, reconcile, publication).await)
}

pub(super) async fn perform(
    state: &Shared,
    id: String,
    input: GitPushRequest,
    reconcile: bool,
    publication: Option<ForgePublicationRequest>,
) -> crate::Result<Card> {
    if id != input.expected.id || !input.confirmed {
        return Err("Confirme le commit et la branche à pousser.".into());
    }
    let requested = input.clone();
    let operation = uuid::Uuid::new_v4().to_string();
    let card = state
        .database
        .call(move |store| {
            if let Some(publication) = publication {
                return crate::infrastructure::forge_publication::reserve_branch(
                    store,
                    &publication,
                    &operation,
                );
            }
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
                forge::configuration(store, intent.preview.configuration.kind, requested.revision)?;
            if configuration.base != intent.preview.configuration.base {
                return Err("Reconnecte le site de forge du push incertain.".into());
            }
            Ok(card)
        })
        .await?;
    let intent = card
        .fields
        .get::<GitPushIntent>("gitPush")?
        .ok_or("Intention de push absente.")?;
    let _ = state.changed.send(());
    let mut stopped = state.shutdown.subscribe();
    let work = async {
        let vault = state.vault.clone();
        let revision = input.revision;
        let kind = intent.preview.configuration.kind;
        let (configuration, token) = state
            .database
            .call(move |store| {
                forge::credentials(store, kind, revision, &mut |value, decrypt| {
                    vault.protect(value, decrypt)
                })
            })
            .await?;
        if !reconcile && let Some(linked) = intent.preview.pull_request.as_ref() {
            let remote = forge::Forge::new(configuration.clone(), token.clone(), revision)?
                .pull_request(&intent.preview.repository, linked.number)
                .await?;
            workflow::require_pull_request(&intent.preview, &remote)?;
        }
        let authentication = HttpsAuthentication::new(
            configuration.clone(),
            intent.preview.repository.clone(),
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
                &format!("refs/heads/{}", intent.preview.worktree.branch),
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
    result
}
