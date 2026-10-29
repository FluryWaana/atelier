use super::{Shared, failure, responses::response};
use crate::infrastructure::{agent_cli as workflow, agent_providers};
use atelier_core::{
    application::agent_cli::{Operation, Phase, UpdateRequest},
    document::Document,
};
use axum::{
    extract::{Path, Request, State},
    http::StatusCode,
    response::Response,
};
pub(super) async fn update(
    State(state): State<Shared>,
    Path(provider): Path<String>,
    request: Request,
) -> Response {
    let input: UpdateRequest = match super::jira_transitions::input(request).await {
        Ok(value) => value,
        Err(response) => return *response,
    };
    if !input.confirmed {
        return failure(StatusCode::BAD_REQUEST, "message.confirm_cli_version");
    }
    let expected = input.expected;
    let info = input.info;
    let selected = provider.clone();
    let vault = state.vault.clone();
    let snapshot = info.clone();
    let (profile, operation) = match state
        .database
        .call(move |store| {
            let mut body = Document::default();
            body.set("expected", &expected)?;
            let profile = agent_providers::diagnostic_profile(
                store,
                &body,
                &selected,
                &mut |value, decrypt| vault.protect(value, decrypt),
            )?;
            let operation = workflow::reserve(store, &selected, snapshot)?;
            Ok((profile, operation))
        })
        .await
    {
        Ok(value) => value,
        Err(error) => return response::<Operation>(Err(error)),
    };
    let _ = state.changed.send(());
    let id = operation.id;
    let mut stopped = state.shutdown.subscribe();
    let result = async {
        let (mut command, pin, _lock) = workflow::prepare(&profile, &info).await?;
        if *stopped.borrow() {
            return Err("message.atelier_is_shutting_down".into());
        }
        let selected = id.clone();
        state
            .database
            .call(move |store| {
                workflow::change(
                    store,
                    &selected,
                    Phase::Running,
                    None,
                    "message.cli_update_command_started_outcome_not_yet_observed",
                )
            })
            .await?;
        let _ = state.changed.send(());
        let guard = async {
            let stopping = *stopped.borrow();
            if !stopping {
                let _ = stopped.changed().await;
            }
            crate::Error::from("message.cli_update_interrupted_by_shutdown")
        };
        let output = crate::infrastructure::process_tree::capture_pinned(
            &mut command,
            pin,
            std::time::Duration::from_secs(180),
            1024 * 1024,
            guard,
        )
        .await?;
        if !output.status.success() {
            return Err("message.cli_update_command_failed".into());
        }
        let version = workflow::observed_version(&profile, &info).await?;
        let message =
            if info.availability == atelier_core::application::agent_cli::Availability::Manual {
                "message.cli_version_read_after_the_command_no_target_version_had_been_ve"
            } else {
                "message.cli_version_read_after_the_command_check_the_target_and_actually"
            };
        let selected = id.clone();
        state
            .database
            .call(move |store| {
                workflow::change(store, &selected, Phase::Observed, Some(version), message)
            })
            .await
    }
    .await;
    let result = match result {
        Ok(value) => Ok(value),
        Err(error) => {
            let selected = id;
            match state.database.call(move|store|{
            if let Some(operation)=workflow::read(store)?.filter(|operation|operation.id==selected){match operation.phase {Phase::Preparing=>{workflow::change(store,&selected,Phase::Failed,None,"message.cli_preflight_refused_command_not_sent")?;},Phase::Running=>{workflow::change(store,&selected,Phase::Uncertain,None,"message.cli_update_outcome_uncertain_run_diagnostics_again_before_confir")?;},_=>{}}}Ok(())
        }).await {Ok(())=>Err(error),Err(recording)=>Err(recording)}
        }
    };
    let _ = state.changed.send(());
    response(result)
}
