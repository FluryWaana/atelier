use super::{Shared, failure, object, responses::response};
use crate::{
    Result,
    infrastructure::{automation as workflow, forge, reviews, store::Documents, worktrees},
};
use atelier_core::{
    application::{
        automation::{Flow, Step},
        reviews::Context,
    },
    contracts::Card,
    document::Document,
};
use axum::{
    extract::{Path, Request, State},
    http::StatusCode,
    response::Response,
};

pub(super) async fn change(
    State(state): State<Shared>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    let body = match object(request.into_body()).await {
        Ok(body) => body,
        Err((status, message)) => return failure(status, message),
    };
    response(
        state
            .database
            .call(move |store| workflow::change(store, &id, &body))
            .await,
    )
}
pub(super) async fn stop(
    State(state): State<Shared>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    let body = match object(request.into_body()).await {
        Ok(body) => body,
        Err((status, message)) => return failure(status, message),
    };
    if body.get::<bool>("confirmed").ok().flatten() != Some(true) {
        return failure(
            StatusCode::BAD_REQUEST,
            "Confirme l’arrêt du parcours automatique.",
        );
    }
    let Some(operation) = body.get::<String>("id").ok().flatten() else {
        return failure(StatusCode::BAD_REQUEST, "Parcours absent.");
    };
    let result = async {
        let card_id = id.clone();
        let run = state
            .database
            .call(move |store| workflow::cancel(store, &id, &operation))
            .await?;
        if let Some(run) = run
            && state.runs.active_cards().contains(&card_id)
        {
            state.runs.stop(&run).await?;
        }
        Ok::<_, crate::Error>(serde_json::json!({"ok":true}))
    }
    .await;
    response(result)
}

pub(super) async fn tested_sources(state: &Shared, card: &Card, flow: &Flow) -> Result<()> {
    let id = flow
        .verification_id
        .clone()
        .ok_or("Vérification du parcours absente.")?;
    let run = state
        .database
        .call(move |store| {
            store
                .document::<Document>(Documents::Runs, &id)?
                .ok_or_else(|| "Vérification du parcours absente.".into())
        })
        .await?;
    let repository = worktrees::inspect(
        &state.prototype_directory,
        &state.prototype_paths,
        &flow.worktree,
    )
    .await?;
    if !run.string_equals("cardId", &card.id)?
        || !run.string_equals("verificationState", "passed")?
        || run.get::<String>("sourceFingerprint")?.as_deref()
            != Some(repository.fingerprint().await?.as_str())
    {
        return Err("Les sources du parcours ont changé depuis la vérification. Reprends les vérifications avant la review.".into());
    }
    Ok(())
}
async fn validate_review(state: &Shared, card: &Card, flow: &Flow) -> Result<()> {
    let id = flow.run_id.clone().ok_or("Review du parcours absente.")?;
    let selected = id.clone();
    let run = state
        .database
        .call(move |store| {
            store
                .document::<Document>(Documents::Runs, &selected)?
                .ok_or_else(|| "Review du parcours absente.".into())
        })
        .await?;
    if !run.string_equals("status", "completed")? || run.get::<i32>("exitCode")? != Some(0) {
        return Err("La review du parcours n’a pas abouti. Consulte le journal.".into());
    }
    let recorded = run
        .get::<Context>("reviewContext")?
        .ok_or("Contexte de review absent.")?;
    let vault = state.vault.clone();
    let kind = recorded.kind;
    let revision = recorded.revision;
    let (configuration, token) = state
        .database
        .call(move |store| {
            forge::credentials(store, kind, revision, &mut |value, decrypt| {
                vault.protect(value, decrypt)
            })
        })
        .await?;
    let current = reviews::prepare(
        &state.prototype_directory,
        &state.prototype_paths,
        card,
        configuration,
        token,
    )
    .await?;
    if current != recorded {
        return Err("Les commits ou le besoin ont changé. Relance la review.".into());
    }
    tested_sources(state, card, flow).await?;
    let card = card.clone();
    let now =
        time::OffsetDateTime::now_utc().format(&time::format_description::well_known::Rfc3339)?;
    state
        .database
        .call(move |store| {
            reviews::validate(store, &card, &id, &current, &now)?;
            Ok(())
        })
        .await
}

pub(super) async fn tick(state: &Shared) -> Result<bool> {
    let Some((card, flow)) = state.database.call(workflow::running).await? else {
        return Ok(false);
    };
    let card_id = card.id.clone();
    let operation_id = flow.id.clone();
    let result = async {
        if super::updates::guard(state.installation_root.as_deref()).is_some() {
            return Err(
                "Parcours suspendu pendant la maintenance. Vérifie le résultat avant de reprendre."
                    .into(),
            );
        }
        if flow.step == Step::Publication && flow.publication_plan.is_some() {
            return super::workflow_publication::perform(state, &card.id, &flow.id).await;
        }
        if flow.step == Step::Commit {
            let id = card.id.clone();
            let operation = flow.id.clone();
            let (expected, approved) = state
                .database
                .call(move |store| workflow::commit(store, &id, &operation))
                .await?;
            let input = atelier_core::contracts::LocalCommitRequest {
                expected,
                paths: approved.snapshot.paths,
                author_name: approved.author_name,
                author_email: approved.author_email,
                message: approved.message,
                confirmed: true,
            };
            let committed = super::commits::perform(state, card.id.clone(), input, false).await?;
            let operation = flow.id.clone();
            return state
                .database
                .call(move |store| workflow::commit_complete(store, &committed, &operation))
                .await;
        }
        if flow.run_id.is_some() {
            if flow.step == Step::Review {
                validate_review(state, &card, &flow).await?;
            }
            let id = card.id.clone();
            let operation = flow.id.clone();
            state
                .database
                .call(move |store| workflow::settle(store, &id, &operation))
                .await?;
        } else {
            if flow.step == Step::Review {
                tested_sources(state, &card, &flow).await?;
            }
            let run_id = uuid::Uuid::new_v4().to_string();
            let selected = run_id.clone();
            let id = card.id.clone();
            let operation = flow.id.clone();
            let (card, flow, body) = state
                .database
                .call(move |store| workflow::claim(store, &id, &operation, selected))
                .await?;
            if flow.step == Step::Tests {
                super::runs::launch_verification(state, card.id, body, run_id).await?;
            } else {
                super::runs::launch(state, card.id, body, run_id, false).await?;
            }
        }
        Ok::<_, crate::Error>(())
    }
    .await;
    if let Err(error) = result {
        let message = error.to_string();
        state
            .database
            .call(move |store| workflow::fail(store, &card_id, &operation_id, &message))
            .await?;
    }
    let _ = state.changed.send(());
    Ok(true)
}
