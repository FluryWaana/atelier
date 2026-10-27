use super::{Shared, failure, object};
use crate::{
    Result,
    infrastructure::{forge, reviews, store::Documents},
};
use atelier_core::{
    application::reviews::Context,
    contracts::{Card, PullRequest},
    document::Document,
};
use axum::{
    extract::{Path, Request, State},
    response::Response,
};

pub(super) async fn validate(
    State(state): State<Shared>,
    Path((card_id, run_id)): Path<(String, String)>,
    request: Request,
) -> Response {
    let body = match object(request.into_body()).await {
        Ok(body) => body,
        Err((status, message)) => return failure(status, message),
    };
    let result: Result<Document> = async {
        if body.get::<bool>("confirmed")? != Some(true) {
            return Err("Confirme la revalidation de cette review.".into());
        }
        let expected = body
            .get::<Card>("expected")?
            .ok_or("Instantané de tâche requis.")?;
        if card_id != expected.id {
            return Err("Carte incohérente.".into());
        }
        if expected.fields.raw("connectorPullRequest").is_some() {
            return super::connectors::repositories::validate_review(&state, &expected, &run_id)
                .await;
        }
        let selected = run_id.clone();
        let input = expected.clone();
        let vault = state.vault.clone();
        let (card, recorded, configuration, token) = state
            .database
            .call(move |store| {
                let card = crate::infrastructure::forge_association::snapshot(store, &input)?;
                let mut run = store
                    .document::<Document>(Documents::Runs, &selected)?
                    .ok_or("Review introuvable.")?;
                if !run.string_equals("cardId", &card.id)?
                    || !run.string_equals("kind", "pr-review")?
                    || !run.string_equals("status", "completed")?
                    || run
                        .get::<atelier_core::application::reviews::Review>("review")?
                        .is_none()
                {
                    return Err("Review incomplète ou périmée. Relance la review.".into());
                }
                // A failed refresh must never leave the previous validation looking current.
                run.set("reviewStaleReason", &reviews::UNCHECKED)?;
                store.put(Documents::Runs, &run)?;
                let recorded = run
                    .get::<Context>("reviewContext")?
                    .ok_or("Contexte de review absent.")?;
                reviews::matches(store, &card, &recorded)?;
                let linked = card
                    .fields
                    .get::<PullRequest>("pullRequest")?
                    .ok_or("Liaison de PR/MR retirée.")?;
                let (configuration, token) = forge::credentials(
                    store,
                    linked.kind,
                    linked.revision,
                    &mut |value, decrypt| vault.protect(value, decrypt),
                )?;
                Ok((card, recorded, configuration, token))
            })
            .await?;
        let current = reviews::prepare(
            &state.prototype_directory,
            &state.prototype_paths,
            &card,
            configuration,
            token,
        )
        .await?;
        if current != recorded {
            return Err("Les commits ou le besoin ont changé. Relance la review.".into());
        }
        let now = time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)?;
        state
            .database
            .call(move |store| reviews::validate(store, &card, &run_id, &current, &now))
            .await
    }
    .await;
    let _ = state.changed.send(());
    super::responses::response(result)
}
