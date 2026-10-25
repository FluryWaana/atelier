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
    blocked.then(|| failure(StatusCode::CONFLICT, "Arrête les exécutions avant de modifier cette tâche, les projets, les profils ou les sauvegardes."))
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
    let result: Result<Document> = async {
        let mode = body.get::<String>("mode")?.ok_or("Mode requis.")?;
        if !["analyze", "edit"].contains(&mode.as_str()) {
            return Err("Mode invalide.".into());
        }
        let agent = body.get::<String>("agent")?.ok_or("Profil requis.")?;
        let vault=state.vault.clone();
        let requested=body.clone();
        let conversation_body=body.clone();
        let (card, profile, api_key, rules, conversation) = state
            .database
            .call(move |store| {
                let card = store
                    .document::<Card>(Documents::Cards, &id)?
                    .ok_or("Carte introuvable.")?;
                let (mut profile,api_key)=agent_providers::execution_profile(store,&agent,&mut |value,decrypt| vault.protect(value,decrypt))?;
                let settings:serde_json::Value=serde_json::from_str(&serde_json::to_string(&store.settings::<Document>()?)?)?;
                let card_value:serde_json::Value=serde_json::from_str(&serde_json::to_string(&card)?)?;
                let requested:serde_json::Value=serde_json::from_str(&serde_json::to_string(&requested)?)?;
                let options=atelier_core::domain::agent_options::resolve_agent_options(&settings,&card_value,&requested)?;
                profile.set("model",&options["model"])?; profile.set("effort",&options["effort"])?;
                if card.fields.truthy("archived")? {
                    return Err("Restaure cette tâche avant de lancer un agent.".into());
                }
                let project = card.fields.get::<String>("projectId")?.map(|id| store.document::<Document>(Documents::Projects, &id)).transpose()?.flatten();
                let rules = project.map(|project| project.get::<String>("projectRules")).transpose()?.flatten().unwrap_or_default();
                let conversation=if card.fields.truthy("brainstormSession")? {
                    if conversation_body.get::<String>("mode")?.as_deref()!=Some("analyze"){return Err("Un brainstorm utilise uniquement le mode Analyse.".into());}
                    let expected=conversation_body.get::<Card>("expected")?.ok_or("Instantané de session requis.")?;
                    let reply=conversation_body.get::<String>("message")?.ok_or("Message de brainstorm requis.")?;
                    Some(crate::infrastructure::brainstorm::conversation(store,&card,&expected,&reply)?)
                }else{None};
                Ok((card, profile, api_key, rules, conversation))
            })
            .await?;
        let no_repository=card.fields.truthy("brainstormSession")? && card.fields.get::<String>("repo")?.unwrap_or_default().is_empty();
        let (directory,branch)=if no_repository {
            if card.fields.raw("worktree").is_some(){return Err("Retire le worktree avant de poursuivre sans dépôt.".into());}
            (RunDirectory::conversation()?,String::new())
        }else{
            let worktree=card.fields.get::<Worktree>("worktree")?.ok_or("Prépare le worktree avant de lancer une commande.")?;
            if worktree.state!=WorktreeState::Ready {return Err("Vérifie le worktree avant de lancer une commande.".into());}
            let source=card.fields.get::<String>("repo")?.ok_or("Dépôt absent.")?;
            if !same_repository(&source,&worktree.source,true){return Err("Le dépôt source ne correspond plus au worktree.".into());}
            let repository=worktrees::inspect(&state.prototype_directory,&state.prototype_paths,&worktree).await?;
            (RunDirectory::Worktree(repository),worktree.branch)
        };
        let working_path=crate::infrastructure::task_creation::path_string(directory.path()).utf8()?;
        let kind=profile.get::<String>("type")?.ok_or("Agent invalide.")?;
        let (mut command,secrets)=if kind=="command" { (agent_diagnostics::command(&profile).await?,Vec::new()) } else { agent_diagnostics::agent_command(&profile,&api_key).await? };
        let model = profile.get::<String>("model")?.unwrap_or_default();
        let effort = profile.get::<String>("effort")?.unwrap_or_default();
        let args=match kind.as_str() {
            "command"=>profile.get::<Vec<String>>(&format!("{mode}Args"))?.ok_or("Arguments de commande absents.")?,
            "codex"=> { let mut args=vec!["app-server".to_owned()]; if !api_key.is_empty() { args.extend(["-c".into(),"cli_auth_credentials_store=\"ephemeral\"".into()]); } args },
            "claude"=> {
                let mut args=vec!["-p".into(),"--output-format".into(),"stream-json".into(),"--input-format".into(),"stream-json".into(),"--verbose".into(),"--include-partial-messages".into(),"--permission-mode".into(),if mode=="edit"{"default".into()}else{"plan".into()}];
                if !model.is_empty() { args.extend(["--model".into(),model.clone()]); }
                if !effort.is_empty() { args.extend(["--effort".into(),effort.clone()]); }
                args
            },
            _=>return Err("Agent invalide.".into()),
        };
        if args.len()>128 || args.iter().any(|arg| arg.len()>16*1024 || arg.contains('\0')) { return Err("Arguments de commande invalides.".into()); }
        command
            .args(
                args.iter()
                    .map(|arg| arg.replace("{model}", &model).replace("{effort}", &effort)),
            )
            .current_dir(&working_path);
        let criteria = card.fields.get::<Vec<Document>>("checklist")?.unwrap_or_default().iter().map(|criterion| criterion.get::<String>("text").map(|text| text.unwrap_or_default())).collect::<std::result::Result<Vec<_>,_>>()?.join("\n- ");
        let instruction = if mode == "analyze" { "Analyse seulement, sans modifier les fichiers." } else { "Effectue les modifications nécessaires et indique les vérifications et limites." };
        let prompt = if let Some(conversation)=conversation {if no_repository{format!("Conversation sans dépôt, dans un dossier temporaire vide. Aucun fichier utilisateur ni projet à inspecter. Ne crée aucun fichier.\n{conversation}")}else{format!("Branche : {}. Ne change pas de branche. Règles du projet :\n{rules}\n\n{conversation}",branch)}} else {format!(
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
            return Err("Instructions trop longues.".into());
        }
        let mut run = Document::default();
        run.set("id", &uuid::Uuid::new_v4().to_string())?;
        run.set("cardId", &card.id)?;
        run.set("agent", &profile.get::<String>("id")?)?;
        run.set("agentType", &kind)?;
        run.set("mode", &mode)?;
        if card.fields.truthy("brainstormSession")? {run.set("purpose",&"brainstorm")?;run.set("userMessage",&body.get::<String>("message")?.ok_or("Message de brainstorm requis.")?)?;}
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
        let input=if kind=="command" { RunInput::Command(prompt) } else { RunInput::Agent(Box::new(Protocol::new(ProtocolConfig { kind:if kind=="codex"{Kind::Codex}else{Kind::Claude},edit:mode=="edit",repo:working_path,prompt,model,effort,api_key,secrets }))) };
        state
            .runs
            .start(state.database.clone(), run, command, input, directory)
            .await
    }
    .await;
    match result {
        Ok(run) => Json(run).into_response(),
        Err(error) => failure(StatusCode::BAD_REQUEST, &error.to_string()),
    }
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
            Err(_) => return failure(StatusCode::BAD_REQUEST, "Réponse interactive invalide."),
        };
    match state.runs.reply(&id, answer).await {
        Ok(()) => Json(serde_json::json!({"ok":true})).into_response(),
        Err(error) => failure(StatusCode::CONFLICT, &error.to_string()),
    }
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
        return failure(StatusCode::BAD_REQUEST, "Curseur de journal invalide.");
    }
    match state
        .database
        .call(move |store| {
            if store.document::<Document>(Documents::Runs, &id)?.is_none() {
                return Err("Exécution introuvable.".into());
            }
            store.logs(&id, cursor.after)
        })
        .await
    {
        Ok(logs) => Json(logs).into_response(),
        Err(_) => failure(StatusCode::NOT_FOUND, "Journal indisponible."),
    }
}
