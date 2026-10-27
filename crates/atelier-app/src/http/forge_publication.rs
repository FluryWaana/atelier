use super::{Shared, failure, responses::response};
use crate::infrastructure::{
    forge, forge_association, forge_publication as workflow, git_transport::HttpsAuthentication,
    store::Documents,
};
use atelier_core::contracts::{
    Card, ForgePublicationIntent, ForgePublicationRequest, ForgeWriteState,
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
    let input: ForgePublicationRequest = match super::jira_transitions::input(request).await {
        Ok(input) => input,
        Err(response) => return *response,
    };
    if input.expected.id != id {
        return failure(StatusCode::BAD_REQUEST, "Carte de publication incohérente.");
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
        let kind = input.kind;
        let revision = input.revision;
        let (configuration, token) = state
            .database
            .call(move |store| {
                forge_association::snapshot(store, &expected)?;
                forge::credentials(store, kind, revision, &mut |value, decrypt| {
                    vault.protect(value, decrypt)
                })
            })
            .await?;
        let preview = workflow::preview(
            &state.prototype_directory,
            &state.prototype_paths,
            &input,
            configuration,
            token,
        )
        .await?;
        state
            .database
            .call(move |store| workflow::save_preview(store, &input.expected, preview))
            .await
    };
    let stopping = *stopped.borrow();
    if stopping {
        return failure(StatusCode::CONFLICT, "Atelier est en cours d’arrêt.");
    }
    tokio::select! {result=tokio::time::timeout(std::time::Duration::from_secs(180),operation)=>response(result.unwrap_or_else(|_|Err("Aperçu de publication trop long.".into()))),_=stopped.changed()=>failure(StatusCode::CONFLICT,"Atelier est en cours d’arrêt.")}
}
pub(super) async fn create(
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
    let input: ForgePublicationRequest = match super::jira_transitions::input(request).await {
        Ok(input) => input,
        Err(response) => return *response,
    };
    if input.expected.id != id || !input.confirmed {
        return failure(
            StatusCode::BAD_REQUEST,
            "Confirme les branches et le contenu de la PR/MR à créer.",
        );
    }
    let Ok(_permit) = state.integration_reads.try_acquire() else {
        return failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "Les recherches distantes sont occupées. Réessaie dans un instant.",
        );
    };
    response(perform(&state, id, input, reconcile).await)
}

pub(super) async fn perform(
    state: &Shared,
    id: String,
    input: ForgePublicationRequest,
    reconcile: bool,
) -> crate::Result<Card> {
    if id != input.expected.id || !input.confirmed {
        return Err("Confirme les branches et le contenu de la PR/MR à créer.".into());
    }
    let prepared = input.clone();
    let operation = uuid::Uuid::new_v4().to_string();
    let card = state
        .database
        .call(move |store| {
            if !reconcile {
                return workflow::reserve(store, &prepared, &operation);
            }
            let card = store
                .document::<Card>(Documents::Cards, &prepared.expected.id)?
                .ok_or("Carte introuvable.")?;
            if serde_json::to_value(atelier_core::application::jira::card_snapshot(&card))?
                != serde_json::to_value(atelier_core::application::jira::card_snapshot(
                    &prepared.expected,
                ))?
            {
                return Err("La tâche a changé. Recharge avant de vérifier la publication.".into());
            }
            let intent = card
                .fields
                .get::<ForgePublicationIntent>("forgePublication")?
                .filter(|intent| intent.state == ForgeWriteState::Uncertain)
                .ok_or("Aucune publication incertaine à vérifier.")?;
            let configuration =
                forge::configuration(store, intent.preview.configuration.kind, prepared.revision)?;
            if configuration.base != intent.preview.configuration.base {
                return Err("Reconnecte le site de forge de la publication incertaine.".into());
            }
            Ok(card)
        })
        .await?;
    let intent = card
        .fields
        .get::<ForgePublicationIntent>("forgePublication")?
        .ok_or("Intention de publication absente.")?;
    let _ = state.changed.send(());
    let mut stopped = state.shutdown.subscribe();
    let work = async {
        let vault = state.vault.clone();
        let kind = intent.preview.configuration.kind;
        let revision = input.revision;
        let (configuration, token) = state
            .database
            .call(move |store| {
                forge::credentials(store, kind, revision, &mut |value, decrypt| {
                    vault.protect(value, decrypt)
                })
            })
            .await?;
        let client = forge::Forge::new(configuration.clone(), token.clone(), revision)?;
        let mut known = intent.number;
        if !reconcile {
            let authentication = HttpsAuthentication::new(
                configuration.clone(),
                intent.preview.repository.clone(),
                token,
            )?;
            workflow::prepare(
                &state.prototype_directory,
                &state.prototype_paths,
                &card,
                &configuration,
                &authentication,
            )
            .await?;
            let prepared = client
                .prepare_publication(&intent.preview, &intent.id)
                .await?;
            let expected = card.clone();
            let operation = intent.id.clone();
            state
                .database
                .call(move |store| workflow::sending(store, &expected, &operation))
                .await?;
            let _ = state.changed.send(());
            let number = client.create_publication(prepared).await?;
            let card_id = id.clone();
            let operation = intent.id.clone();
            state
                .database
                .call(move |store| workflow::remember_number(store, &card_id, &operation, number))
                .await?;
            known = Some(number);
        }
        let found = client
            .reconcile_publication(&intent.preview, &intent.id, known)
            .await?;
        let card_id = id.clone();
        let operation = intent.id.clone();
        match found{
            Some(pr)=>state.database.call(move|store|workflow::record(store,&card_id,&operation,pr)).await,
            None=>Err("Aucune PR/MR portant le marqueur retrouvée. La publication reste incertaine ; aucun renvoi automatique.".into()),
        }
    };
    let stopping = *stopped.borrow();
    let result = if stopping {
        drop(work);
        Err("Atelier est en cours d’arrêt.".into())
    } else {
        tokio::select! {result=tokio::time::timeout(std::time::Duration::from_secs(300),work)=>result.unwrap_or_else(|_|Err("Publication interrompue ou trop longue. Relis la forge sans renvoyer.".into())),_=stopped.changed()=>Err("Atelier est en cours d’arrêt.".into())}
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
