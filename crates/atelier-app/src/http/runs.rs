use super::{Shared, failure, object};
use crate::{
    Result,
    infrastructure::{
        agent_diagnostics, agent_providers,
        command_runs::{CommandRuns, RunDirectory, RunInput},
        store::Documents,
        worktrees,
    },
};
use atelier_core::{
    application::agent_protocol::{Kind, Protocol, ProtocolConfig},
    contracts::{AgentAnswer, Card, Worktree, WorktreeState},
    document::Document,
    domain::projects::same_repository,
};
use axum::{
    Json,
    extract::{Path, Query, Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Deserialize;

pub(super) fn guard(manager: &CommandRuns, path: &str) -> Option<Response> {
    let cards = manager.active_cards();
    if cards.is_empty() {
        return None;
    }
    let blocked = path.starts_with("/api/backups")
        || path.starts_with("/api/projects")
        || path.starts_with("/api/settings")
        || path.starts_with("/api/agent-providers")
        || path.strip_prefix("/api/cards/").is_some_and(|tail| {
            let (id, action) = tail.split_once('/').unwrap_or((tail, ""));
            cards.iter().any(|card| card == id) && !action.starts_with("services")
        });
    blocked.then(|| {
        failure(
            StatusCode::CONFLICT,
            "message.stop_runs_before_changing_this_task_projects_profiles_or_backups",
        )
    })
}

pub(super) async fn start(
    State(state): State<Shared>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    let body = match object(request.into_body()).await {
        Ok(body) => body,
        Err((status, message)) => return failure(status, message),
    };
    let result = launch(&state, id, body, uuid::Uuid::new_v4().to_string(), false).await;
    match result {
        Ok(run) => Json(run).into_response(),
        Err(error) => failure(StatusCode::BAD_REQUEST, &error.to_string()),
    }
}

pub(super) async fn launch(
    state: &Shared,
    id: String,
    body: Document,
    run_id: String,
    queued: bool,
) -> Result<Document> {
    let result: Result<Document> = async {
        let mode = body.get::<String>("mode")?.ok_or("message.mode_required")?;
        if !["analyze", "edit"].contains(&mode.as_str()) {
            return Err("message.invalid_mode".into());
        }
        let purpose=body.get::<String>("purpose")?.unwrap_or_else(||"work".into());
        if !["work","preparation","review","correction"].contains(&purpose.as_str()) {return Err("message.invalid_run_purpose".into());}
        if purpose=="review" && mode!="analyze" {return Err("message.review_only_uses_analyze_mode".into());}
        if purpose=="correction" && (mode!="edit" || body.get::<bool>("confirmed")?!=Some(true)) {return Err("message.confirm_the_selected_correction_in_edit_mode".into());}
        let agent = body.get::<String>("agent")?.ok_or("message.profile_required")?;
        let vault=state.vault.clone();
        let requested=body.clone();
        let conversation_body=body.clone();
        let (card, profile, api_key, rules, conversation) = state
            .database
            .call(move |store| {
                let card = store
                    .document::<Card>(Documents::Cards, &id)?
                    .ok_or("message.card_not_found")?;
                if let Some(expected)=requested.get::<Card>("expected")?
                    && serde_json::to_value(atelier_core::application::jira::card_snapshot(&expected))?!=serde_json::to_value(atelier_core::application::jira::card_snapshot(&card))? {return Err("message.the_task_changed_reload_before_starting_the_run".into());}
                let (mut profile,api_key)=agent_providers::execution_profile(store,&agent,&mut |value,decrypt| vault.protect(value,decrypt))?;
                let settings:serde_json::Value=serde_json::from_str(&serde_json::to_string(&store.settings::<Document>()?)?)?;
                let settings=atelier_core::domain::projects::settings_for_project(&settings,&store.documents::<serde_json::Value>(Documents::Projects)?,&card.fields.get::<String>("repo")?.unwrap_or_default(),cfg!(windows));
                let card_value:serde_json::Value=serde_json::from_str(&serde_json::to_string(&card)?)?;
                let requested:serde_json::Value=serde_json::from_str(&serde_json::to_string(&requested)?)?;
                let options=atelier_core::domain::agent_options::resolve_agent_options(&settings,&card_value,&requested)?;
                profile.set("model",&options["model"])?; profile.set("effort",&options["effort"])?;
                if card.fields.truthy("archived")? {
                    return Err("message.restore_task_before_agent".into());
                }
                let rules=settings["projectRules"].as_str().unwrap_or_default().to_owned();
                let conversation=if card.fields.truthy("brainstormSession")? {
                    if conversation_body.get::<String>("mode")?.as_deref()!=Some("analyze"){return Err("message.a_brainstorm_only_uses_analyze_mode".into());}
                    let expected=conversation_body.get::<Card>("expected")?.ok_or("message.session_snapshot_required")?;
                    let reply=conversation_body.get::<String>("message")?.ok_or("message.brainstorm_message_required")?;
                    Some(crate::infrastructure::brainstorm::conversation(store,&card,&expected,&reply)?)
                }else if conversation_body.get::<String>("purpose")?.as_deref()==Some("preparation") {
                    if conversation_body.get::<String>("mode")?.as_deref()!=Some("analyze"){return Err("message.preparation_only_uses_analyze_mode".into());}
                    let expected=conversation_body.get::<Card>("expected")?.ok_or("message.task_snapshot_required")?;
                    if serde_json::to_value(atelier_core::application::jira::card_snapshot(&card))?!=serde_json::to_value(atelier_core::application::jira::card_snapshot(&expected))? {return Err("message.the_task_changed_reload_before_preparing_the_issue".into());}
                    Some(atelier_core::application::preparation::prompt(&card)?)
                }else{None};
                Ok((card, profile, api_key, rules, conversation))
            })
            .await?;
        let connector_review=card.fields.raw("connectorPullRequest").is_some();
        let connector_correction=if purpose=="correction"&&connector_review {
            let input=card.clone();let review=body.get::<String>("reviewId")?.ok_or("message.review_not_found")?;let indexes=body.get::<Vec<usize>>("indexes")?.ok_or("message.invalid_finding_selection")?;
            Some(state.database.call(move|store|crate::infrastructure::connector_repositories::correction(store,&input,&review,&indexes)).await?)
        }else{None};
        let connector_context=if connector_review&&["review","correction"].contains(&purpose.as_str()) {Some(super::connectors::repositories::prepare_review(state,&card).await?)}else{None};
        let correction=if purpose=="correction"&&!connector_review {
            let input=card.clone();let review=body.get::<String>("reviewId")?.ok_or("message.review_not_found")?;let indexes=body.get::<Vec<usize>>("indexes")?.ok_or("message.invalid_finding_selection")?;
            Some(state.database.call(move|store|crate::infrastructure::reviews::correction(store,&input,&review,&indexes)).await?)
        }else{None};
        let review_context=if !connector_review&&["review","correction"].contains(&purpose.as_str()) {
            let expected=body.get::<Card>("expected")?.ok_or("message.task_snapshot_required")?;
            if card.fields.truthy("brainstormSession")? || serde_json::to_value(atelier_core::application::jira::card_snapshot(&card))?!=serde_json::to_value(atelier_core::application::jira::card_snapshot(&expected))? {return Err("message.the_task_changed_reload_before_reviewing".into());}
            let linked=card.fields.get::<atelier_core::contracts::PullRequest>("pullRequest")?.ok_or("message.a_pr_mr_and_its_worktree_are_required_for_review")?;
            let vault=state.vault.clone();let kind=linked.kind;let revision=linked.revision;
            let (configuration,token)=state.database.call(move|store|crate::infrastructure::forge::credentials(store,kind,revision,&mut|value,decrypt|vault.protect(value,decrypt))).await?;
            Some(crate::infrastructure::reviews::prepare(&state.prototype_directory,&state.prototype_paths,&card,configuration,token).await?)
        }else{None};
        let conversation=if let Some((expected,prompt))=connector_correction {
            if connector_context.as_ref()!=Some(&expected){return Err("message.connector_review_changed".into());}Some(prompt)
        }else if let Some(context)=&connector_context {Some(atelier_core::application::extensions::repository_prompt(context)?)}else if let Some((expected,prompt))=correction {
            if review_context.as_ref()!=Some(&expected) {return Err("message.commits_or_requirements_changed_run_the_review_again".into());}
            Some(prompt)
        }else if let Some(context)=&review_context {Some(atelier_core::application::reviews::prompt(context)?)}else{conversation};
        let no_repository=(card.fields.truthy("brainstormSession")? || body.get::<String>("purpose")?.as_deref()==Some("preparation")) && card.fields.get::<String>("repo")?.unwrap_or_default().is_empty();
        let (directory,branch)=if no_repository {
            if card.fields.raw("worktree").is_some(){return Err("message.remove_the_worktree_before_continuing_without_a_repository".into());}
            (RunDirectory::conversation()?,String::new())
        }else{
            let worktree=card.fields.get::<Worktree>("worktree")?.ok_or("message.prepare_the_worktree_before_starting_a_command")?;
            if worktree.state!=WorktreeState::Ready {return Err("message.verify_worktree_before_command".into());}
            let source=card.fields.get::<String>("repo")?.ok_or("message.repository_missing")?;
            if !same_repository(&source,&worktree.source,true){return Err("message.the_source_repository_no_longer_matches_the_worktree".into());}
            let repository=worktrees::inspect(&state.prototype_directory,&state.prototype_paths,&worktree).await?;
            (RunDirectory::Worktree(repository),worktree.branch)
        };
        let working_path=crate::infrastructure::task_creation::path_string(directory.path()).utf8()?;
        let kind=profile.get::<String>("type")?.ok_or("message.invalid_agent")?;
        let model = profile.get::<String>("model")?.unwrap_or_default();
        let effort = profile.get::<String>("effort")?.unwrap_or_default();
        let protocol_kind=Kind::from_id(&kind);
        let (mut command,secrets)=if kind=="command" { (agent_diagnostics::command(&profile).await?,Vec::new()) } else { agent_diagnostics::agent_command(&profile,&api_key).await? };
        let args=match kind.as_str() {
            "command"=>profile.get::<Vec<String>>(&format!("{mode}Args"))?.ok_or("message.command_arguments_missing")?,
            "acp"=>profile.get::<Vec<String>>("protocolArgs")?.unwrap_or_default(),
            _=>protocol_kind.ok_or("message.invalid_agent")?.launch_arguments(&kind,mode=="edit",&model,&effort,!api_key.is_empty()),
        };
        if args.len()>128 || args.iter().any(|arg| arg.len()>16*1024 || arg.contains('\0')) { return Err("message.command_arguments_invalid".into()); }
        command
            .args(
                args.iter()
                    .map(|arg| arg.replace("{model}", &model).replace("{effort}", &effort)),
            )
            .current_dir(&working_path);
        let criteria = card.fields.get::<Vec<Document>>("checklist")?.unwrap_or_default().iter().map(|criterion| criterion.get::<String>("text").map(|text| text.unwrap_or_default())).collect::<std::result::Result<Vec<_>,_>>()?.join("\n- ");
        let instruction = if mode == "analyze" { "Analyse seulement, sans modifier les fichiers." } else { "Effectue les modifications nécessaires et indique les vérifications et limites." };
        let prompt = if let Some(conversation)=conversation {if no_repository{let context=if card.fields.truthy("brainstormSession")?{"Conversation sans dépôt"}else{"Préparation sans dépôt"};format!("{context}, dans un dossier temporaire vide. Aucun fichier utilisateur ni projet à inspecter. Ne crée aucun fichier.\n{conversation}")}else{format!("Branche : {}. Ne change pas de branche. Règles du projet :\n{rules}\n\n{conversation}",branch)}} else {format!(
            "{instruction} Branche : {}. Ne change pas de branche. Ne crée aucun commit, ne pousse rien et ne publie rien.\nRègles du projet :\n{rules}\n\n{}\n\n{}\n\n{}\n\nCritères :\n- {criteria}",
            branch,
            card.title.utf8()?,
            card.fields
                .get::<String>("description")?
                .unwrap_or_default(),
            card.fields
                .get::<String>("instructions")?
                .unwrap_or_default()
        )};
        if prompt.len() > 256 * 1024 {
            return Err("message.instructions_too_long".into());
        }
        let mut run = Document::default();
        run.set("id", &run_id)?; run.set("queued",&queued)?;
        run.set("purpose", &purpose)?;
        if let Some(operation)=body.get::<String>("automationId")? { run.set("automationId",&operation)?; }
        run.set("cardId", &card.id)?;
        run.set("agent", &profile.get::<String>("id")?)?;
        run.set("agentType", &kind)?;
        run.set("mode", &mode)?;
        if let Some(context)=connector_context {
            run.set("connectorReviewContext",&context)?;
            if purpose=="review"{run.set("kind",&"pr-review")?;run.set("reviewStaleReason",&crate::infrastructure::reviews::UNCHECKED)?;}
            else{run.set("correctionOf",&body.get::<String>("reviewId")?)?;run.set("correctionIndexes",&body.get::<Vec<usize>>("indexes")?)?;}
        }
        if let Some(context)=review_context {
            run.set("purpose",&purpose)?;run.set("reviewContext",&context)?;
            if purpose=="review" {run.set("kind",&"pr-review")?;run.set("reviewStaleReason",&crate::infrastructure::reviews::UNCHECKED)?;}
            else {run.set("correctionOf",&body.get::<String>("reviewId")?)?;run.set("correctionIndexes",&body.get::<Vec<usize>>("indexes")?)?;}
        }
        if card.fields.truthy("brainstormSession")? {run.set("purpose",&"brainstorm")?;run.set("userMessage",&body.get::<String>("message")?.ok_or("message.brainstorm_message_required")?)?;}
        if !card.fields.truthy("brainstormSession")? && body.get::<String>("purpose")?.as_deref()==Some("preparation"){run.set("purpose",&"preparation")?;run.set("requirements",&atelier_core::application::preparation::requirements(&card)?)?;}
        run.set("model", &model)?;
        run.set("effort", &effort)?;
        run.set("repo", &working_path)?;
        run.set("branch", &branch)?;
        run.set("status", &"starting")?;
        run.set(
            "startedAt",
            &time::OffsetDateTime::now_utc()
                .format(&time::format_description::well_known::Rfc3339)?,
        )?;
        let input=if kind=="command" { RunInput::Command(prompt) } else { RunInput::Agent(Box::new(Protocol::new(ProtocolConfig { kind:protocol_kind.ok_or("message.invalid_agent")?,edit:mode=="edit",repo:working_path,prompt,model,effort,api_key,secrets }))) };
        if *state.shutdown.borrow(){return Err("message.atelier_is_shutting_down".into());}
        state
            .runs
            .start(state.database.clone(), run, command, input, directory)
            .await
    }
    .await;
    result
}

pub(super) async fn reply(
    State(state): State<Shared>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    let body = match object(request.into_body()).await {
        Ok(body) => body,
        Err((status, message)) => return failure(status, message),
    };
    let answer: AgentAnswer =
        match serde_json::from_str(&serde_json::to_string(&body).unwrap_or_default()) {
            Ok(answer) => answer,
            Err(_) => return failure(StatusCode::BAD_REQUEST, "message.invalid_interactive_reply"),
        };
    match state.runs.reply(&id, answer).await {
        Ok(()) => Json(serde_json::json!({"ok":true})).into_response(),
        Err(error) => failure(StatusCode::CONFLICT, &error.to_string()),
    }
}

pub(super) async fn verify(
    State(state): State<Shared>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    let body = match object(request.into_body()).await {
        Ok(body) => body,
        Err((status, message)) => return failure(status, message),
    };
    super::responses::response(
        launch_verification(&state, id, body, uuid::Uuid::new_v4().to_string()).await,
    )
}

pub(super) async fn launch_verification(
    state: &Shared,
    id: String,
    body: Document,
    run_id: String,
) -> Result<Document> {
    let result: Result<Document> = async {
        if body.get::<bool>("confirmed")? != Some(true) {
            return Err("message.confirm_the_verification_command_saved_in_the_project".into());
        }
        let expected = body
            .get::<Card>("expected")?
            .ok_or("message.task_snapshot_required")?;
        if expected.id != id {
            return Err("message.inconsistent_card".into());
        }
        let command = body
            .get::<String>("command")?
            .ok_or("message.invalid_verification_command")?;
        let selected = command.clone();
        let card = state
            .database
            .call(move |store| {
                let card = crate::infrastructure::forge_association::snapshot(store, &expected)?;
                crate::infrastructure::verification::require_command(store, &card, &selected)?;
                Ok(card)
            })
            .await?;
        let worktree = card
            .fields
            .get::<Worktree>("worktree")?
            .filter(|worktree| worktree.state == WorktreeState::Ready)
            .ok_or("message.prepare_the_worktree_before_verification")?;
        if !same_repository(
            &card.fields.get::<String>("repo")?.unwrap_or_default(),
            &worktree.source,
            true,
        ) {
            return Err("message.the_source_repository_no_longer_matches_the_worktree".into());
        }
        let repository = worktrees::inspect(
            &state.prototype_directory,
            &state.prototype_paths,
            &worktree,
        )
        .await?;
        let fingerprint = repository.fingerprint().await?;
        let directory = RunDirectory::Worktree(repository);
        let mut process = crate::infrastructure::service_launch::shell(&command).await?;
        process.current_dir(directory.path());
        let mut run = Document::default();
        run.set("id", &run_id)?;
        if let Some(operation) = body.get::<String>("automationId")? {
            run.set("automationId", &operation)?;
        }
        run.set("cardId", &card.id)?;
        run.set("status", &"starting")?;
        run.set("agentType", &"verification")?;
        run.set("purpose", &"verification")?;
        run.set(
            "repo",
            &crate::infrastructure::task_creation::path_string(directory.path()),
        )?;
        run.set("branch", &worktree.branch)?;
        run.set("verificationCommand", &command)?;
        run.set("verificationWorktree", &worktree)?;
        run.set("sourceFingerprint", &fingerprint)?;
        run.set(
            "requirements",
            &atelier_core::application::preparation::requirements(&card)?,
        )?;
        run.set("verificationState", &"running")?;
        run.set(
            "startedAt",
            &time::OffsetDateTime::now_utc()
                .format(&time::format_description::well_known::Rfc3339)?,
        )?;
        state
            .runs
            .start(
                state.database.clone(),
                run,
                process,
                RunInput::Verification,
                directory,
            )
            .await
    }
    .await;
    result
}

pub(super) async fn stop(
    State(state): State<Shared>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    if let Err((status, message)) = object(request.into_body()).await {
        return failure(status, message);
    }
    match state.runs.stop(&id).await {
        Ok(()) => Json(serde_json::json!({"ok":true})).into_response(),
        Err(error) => failure(StatusCode::CONFLICT, &error.to_string()),
    }
}

#[derive(Deserialize)]
pub(super) struct Cursor {
    #[serde(default)]
    after: i64,
}

pub(super) async fn logs(
    State(state): State<Shared>,
    Path(id): Path<String>,
    Query(cursor): Query<Cursor>,
) -> Response {
    if cursor.after < 0 {
        return failure(StatusCode::BAD_REQUEST, "message.log_cursor_invalid");
    }
    match state
        .database
        .call(move |store| {
            if store.document::<Document>(Documents::Runs, &id)?.is_none() {
                return Err("message.run_not_found".into());
            }
            store.logs(&id, cursor.after)
        })
        .await
    {
        Ok(logs) => Json(logs).into_response(),
        Err(_) => failure(StatusCode::NOT_FOUND, "message.log_unavailable"),
    }
}
