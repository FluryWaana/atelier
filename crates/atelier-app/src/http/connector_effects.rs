use super::{Shared, failure, responses::response};
use crate::infrastructure::connector_effects as workflow;
use crate::infrastructure::extensions::Executable;
use atelier_core::{application::extensions::RemoteOperation, contracts::ForgeWriteState as Phase};
use atelier_core::{
    application::extensions::{RemoteCommand, RemotePreview},
    document::Document,
};
use axum::{
    extract::{Path, Request, State},
    http::StatusCode,
    response::Response,
};
pub(super) async fn list(State(state): State<Shared>, Path(module): Path<String>) -> Response {
    response(
        state
            .database
            .call(move |store| {
                Ok(workflow::read(&store.settings::<Document>()?)?
                    .into_iter()
                    .filter(|operation| operation.module == module)
                    .collect::<Vec<_>>())
            })
            .await,
    )
}
pub(super) async fn pending(State(state): State<Shared>) -> Response {
    response(state.database.call(|store|Ok(serde_json::json!({"pending":workflow::read(&store.settings::<Document>()?)?.iter().any(workflow::pending)||atelier_core::application::agent_cli::busy(&store.settings::<Document>()?)?}))).await)
}
pub(super) async fn preview(
    State(state): State<Shared>,
    Path(module): Path<String>,
    request: Request,
) -> Response {
    let input: RemotePreview = match super::jira_transitions::input(request).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    let result = state
        .database
        .call(move |store| workflow::preview(store, &module, &input))
        .await;
    let _ = state.changed.send(());
    response(result)
}
pub(super) async fn cancel(
    State(state): State<Shared>,
    Path((module, id)): Path<(String, String)>,
    request: Request,
) -> Response {
    let input: RemoteCommand = match super::jira_transitions::input(request).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    if !input.confirmed {
        return failure(
            StatusCode::BAD_REQUEST,
            "Confirme l’annulation de cet aperçu non envoyé.",
        );
    }
    let result = state
        .database
        .call(move |store| workflow::cancel(store, &module, &id, input.expected_version))
        .await;
    let _ = state.changed.send(());
    response(result)
}
pub(super) async fn send(
    State(state): State<Shared>,
    Path((module, id)): Path<(String, String)>,
    request: Request,
) -> Response {
    execute(state, module, id, request, false).await
}
pub(super) async fn reconcile(
    State(state): State<Shared>,
    Path((module, id)): Path<(String, String)>,
    request: Request,
) -> Response {
    execute(state, module, id, request, true).await
}
async fn execute(
    state: Shared,
    module: String,
    id: String,
    request: Request,
    reconcile: bool,
) -> Response {
    let input: RemoteCommand = match super::jira_transitions::input(request).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    if !input.confirmed {
        return failure(
            StatusCode::BAD_REQUEST,
            "Confirme l’opération de connecteur affichée.",
        );
    }
    let vault = state.vault.clone();
    let selected = module.clone();
    let operation = id.clone();
    let (profile, request) = match state
        .database
        .call(move |store| {
            workflow::prepare(
                store,
                &selected,
                &operation,
                input.expected_version,
                input.expected_revision,
                reconcile,
                &mut |value, decrypt| vault.protect(value, decrypt),
            )
        })
        .await
    {
        Ok(value) => value,
        Err(error) => return response::<RemoteOperation>(Err(error)),
    };
    let revision = profile.revision;
    let mut version = input.expected_version;
    let mut stopped = state.shutdown.subscribe();
    let work = async {
        if let Some(read) =
            crate::infrastructure::connector_ticket_transitions::preflight(&request)?
        {
            let executable = Executable::open(
                std::path::PathBuf::from(&profile.binary.path),
                Some(profile.binary.sha256.clone()),
            )
            .await?;
            let reply = executable
                .prepare(&profile.manifest, read, std::time::Duration::from_secs(20))?
                .dispatch()
                .await?;
            crate::infrastructure::connector_ticket_transitions::validate_preflight(
                &request.input,
                &reply,
            )?;
        }
        let executable = Executable::open(
            std::path::PathBuf::from(&profile.binary.path),
            Some(profile.binary.sha256.clone()),
        )
        .await?;
        let request_id = request.request_id.clone();
        let prepared = executable.prepare(
            &profile.manifest,
            request,
            std::time::Duration::from_secs(30),
        )?;
        {
            let selected = module.clone();
            let operation = id.clone();
            let sending = state
                .database
                .call(move |store| {
                    if reconcile {
                        workflow::observing(
                            store,
                            &selected,
                            &operation,
                            version,
                            revision,
                            &request_id,
                        )
                    } else {
                        workflow::sending(
                            store,
                            &selected,
                            &operation,
                            version,
                            revision,
                            &request_id,
                        )
                    }
                })
                .await?;
            version = sending.version;
            let _ = state.changed.send(());
        }
        let reply = prepared.dispatch().await?;
        let selected = module.clone();
        let operation = id.clone();
        state
            .database
            .call(move |store| {
                workflow::record(
                    store, &selected, &operation, version, revision, reply, reconcile,
                )
            })
            .await
    };
    let stopping = *stopped.borrow();
    let result = if stopping {
        drop(work);
        Err("Atelier est en cours d’arrêt.".into())
    } else {
        tokio::select! {result=tokio::time::timeout(std::time::Duration::from_secs(60),work)=>result.unwrap_or_else(|_|Err("Écriture de connecteur interrompue ; rapproche son état sans renvoyer la demande.".into())),_=stopped.changed()=>Err("Atelier est en cours d’arrêt.".into())}
    };
    let result = match result {
        Err(error) if !reconcile => {
            let selected = module.clone();
            let operation = id.clone();
            match state
                .database
                .call(move |store| {
                    let current = workflow::current(store, &selected, &operation)?;
                    if matches!(current.state, Phase::Preparing | Phase::Sending) {
                        workflow::interrupted(store, &operation, current.version)?;
                    }
                    Ok(())
                })
                .await
            {
                Ok(()) => Err(error),
                Err(recording) => Err(recording),
            }
        }
        result => result,
    };
    let _ = state.changed.send(());
    response(result)
}
