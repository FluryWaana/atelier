use super::{Shared, failure, object};
use crate::{
    Result,
    infrastructure::{agent_diagnostics, command_runs::CommandRuns, store::Documents, worktrees},
};
use atelier_core::{
    contracts::{Card, Worktree, WorktreeState},
    document::Document,
    domain::{agent_options::providers, projects::same_repository},
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
        let (card, profile, rules) = state
            .database
            .call(move |store| {
                let card = store
                    .document::<Card>(Documents::Cards, &id)?
                    .ok_or("Carte introuvable.")?;
                let profile = providers(&store.settings()?)?
                    .into_iter()
                    .find(|profile| profile.string_equals("id", &agent).unwrap_or(false))
                    .ok_or("Agent invalide.")?;
                if !profile.string_equals("type", "command")? {
                    return Err("L’exécution interactive Codex/Claude reste à raccorder.".into());
                }
                if card.fields.truthy("archived")? {
                    return Err("Restaure cette tâche avant de lancer un agent.".into());
                }
                let project = card.fields.get::<String>("projectId")?.map(|id| store.document::<Document>(Documents::Projects, &id)).transpose()?.flatten();
                let rules = project.map(|project| project.get::<String>("projectRules")).transpose()?.flatten().unwrap_or_default();
                Ok((card, profile, rules))
            })
            .await?;
        let worktree = card
            .fields
            .get::<Worktree>("worktree")?
            .ok_or("Prépare le worktree avant de lancer une commande.")?;
        if worktree.state != WorktreeState::Ready {
            return Err("Vérifie le worktree avant de lancer une commande.".into());
        }
        let source = card.fields.get::<String>("repo")?.ok_or("Dépôt absent.")?;
        if !same_repository(&source, &worktree.source, true) {
            return Err("Le dépôt source ne correspond plus au worktree.".into());
        }
        let directory = worktrees::inspect(
            &state.prototype_directory,
            &state.prototype_paths,
            &worktree,
        )
        .await?;
        let mut command = agent_diagnostics::command(&profile).await?;
        let args = profile
            .get::<Vec<String>>(&format!("{mode}Args"))?
            .ok_or("Arguments de commande absents.")?;
        if args.len() > 128
            || args
                .iter()
                .any(|arg| arg.len() > 16 * 1024 || arg.contains('\0'))
        {
            return Err("Arguments de commande invalides.".into());
        }
        let model = profile.get::<String>("model")?.unwrap_or_default();
        let effort = profile.get::<String>("effort")?.unwrap_or_default();
        command
            .args(
                args.iter()
                    .map(|arg| arg.replace("{model}", &model).replace("{effort}", &effort)),
            )
            .current_dir(&worktree.path);
        let criteria = card.fields.get::<Vec<Document>>("checklist")?.unwrap_or_default().iter().map(|criterion| criterion.get::<String>("text").map(|text| text.unwrap_or_default())).collect::<std::result::Result<Vec<_>,_>>()?.join("\n- ");
        let instruction = if mode == "analyze" { "Analyse seulement, sans modifier les fichiers." } else { "Effectue les modifications nécessaires et indique les vérifications et limites." };
        let prompt = format!(
            "{instruction} Branche : {}. Ne change pas de branche. Ne crée aucun commit, ne pousse rien et ne publie rien.\nRègles du projet :\n{rules}\n\n{}\n\n{}\n\n{}\n\nCritères :\n- {criteria}",
            worktree.branch,
            card.title.utf8()?,
            card.fields
                .get::<String>("description")?
                .unwrap_or_default(),
            card.fields
                .get::<String>("instructions")?
                .unwrap_or_default()
        );
        if prompt.len() > 256 * 1024 {
            return Err("Instructions trop longues.".into());
        }
        let mut run = Document::default();
        run.set("id", &uuid::Uuid::new_v4().to_string())?;
        run.set("cardId", &card.id)?;
        run.set("agent", &profile.get::<String>("id")?)?;
        run.set("agentType", &"command")?;
        run.set("mode", &mode)?;
        run.set("repo", &worktree.path)?;
        run.set("branch", &worktree.branch)?;
        run.set("status", &"starting")?;
        run.set(
            "startedAt",
            &time::OffsetDateTime::now_utc()
                .format(&time::format_description::well_known::Rfc3339)?,
        )?;
        state
            .runs
            .start(state.database.clone(), run, command, prompt, directory)
            .await
    }
    .await;
    match result {
        Ok(run) => Json(run).into_response(),
        Err(error) => failure(StatusCode::BAD_REQUEST, &error.to_string()),
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
