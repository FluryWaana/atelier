use super::{Shared, failure, responses::response};
use crate::{
    Result,
    infrastructure::{
        commits as workflow, forge_association, local_commit, store::Documents, worktrees,
    },
};
use atelier_core::contracts::{
    Card, ForgeWriteState as Phase, LocalCommitIntent, LocalCommitPreview, LocalCommitRequest,
    LocalCommitResponse,
};
use axum::{
    extract::{Path, Request, State},
    http::StatusCode,
    response::Response,
};

pub(super) async fn changes(State(state): State<Shared>, Path(id): Path<String>) -> Response {
    let Ok(_permit) = state.file_reads.try_acquire() else {
        return failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "L’explorateur est occupé. Réessaie dans un instant.",
        );
    };
    let operation = async {
        let card = state
            .database
            .call(move |store| {
                store
                    .document::<Card>(Documents::Cards, &id)?
                    .ok_or_else(|| "Carte introuvable.".into())
            })
            .await?;
        let worktree = workflow::worktree(&card)?;
        let repository = worktrees::inspect(
            &state.prototype_directory,
            &state.prototype_paths,
            &worktree,
        )
        .await?;
        repository.changes(None).await
    };
    response(
        tokio::time::timeout(std::time::Duration::from_secs(60), operation)
            .await
            .unwrap_or_else(|_| Err("Lecture des changements trop longue.".into())),
    )
}

pub(super) async fn preview(
    State(state): State<Shared>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    let input: LocalCommitRequest = match super::jira_transitions::input(request).await {
        Ok(input) => input,
        Err(response) => return *response,
    };
    if id != input.expected.id {
        return failure(StatusCode::BAD_REQUEST, "Carte de commit incohérente.");
    }
    let operation = async {
        local_commit::identity(&input.author_name, &input.author_email, &input.message)?;
        let expected = input.expected;
        let card = state
            .database
            .call(move |store| forge_association::snapshot(store, &expected))
            .await?;
        let worktree = workflow::worktree(&card)?;
        let repository = worktrees::inspect(
            &state.prototype_directory,
            &state.prototype_paths,
            &worktree,
        )
        .await?;
        let preview = local_commit::preview(&repository, input.paths).await?;
        let saved = LocalCommitPreview {
            worktree,
            snapshot: preview.snapshot,
            author_name: input.author_name,
            author_email: input.author_email,
            message: input.message,
        };
        let card = state
            .database
            .call(move |store| workflow::save_preview(store, &card, saved))
            .await?;
        Ok::<_, crate::Error>(LocalCommitResponse {
            card,
            patch: preview.patch,
        })
    };
    response(
        tokio::time::timeout(std::time::Duration::from_secs(180), operation)
            .await
            .unwrap_or_else(|_| Err("Aperçu du commit trop long.".into())),
    )
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
    let input: LocalCommitRequest = match super::jira_transitions::input(request).await {
        Ok(input) => input,
        Err(response) => return *response,
    };
    if id != input.expected.id || !input.confirmed {
        return failure(
            StatusCode::BAD_REQUEST,
            "Confirme les fichiers et le message du commit.",
        );
    }
    response(perform(&state, id, input, reconcile).await)
}

pub(super) async fn perform(
    state: &Shared,
    id: String,
    input: LocalCommitRequest,
    reconcile: bool,
) -> Result<Card> {
    if id != input.expected.id || !input.confirmed {
        return Err("Confirme les fichiers et le message du commit.".into());
    }
    let operation_id = uuid::Uuid::new_v4().to_string();
    let card = state
        .database
        .call(move |store| {
            if !reconcile {
                return workflow::reserve(store, &input, &operation_id);
            }
            let card = store
                .document::<Card>(Documents::Cards, &input.expected.id)?
                .ok_or("Carte introuvable.")?;
            if serde_json::to_value(&card)? != serde_json::to_value(&input.expected)?
                || !card
                    .fields
                    .get::<LocalCommitIntent>("localCommit")?
                    .is_some_and(|intent| intent.state == Phase::Uncertain)
            {
                return Err("Aucun commit incertain sur cet instantané de tâche.".into());
            }
            Ok(card)
        })
        .await?;
    let intent = card
        .fields
        .get::<LocalCommitIntent>("localCommit")?
        .ok_or("Intention de commit absente.")?;
    let _ = state.changed.send(());
    let work = async {
        // Reconciliation verifies the original managed worktree, never an arbitrary path.
        let repository = worktrees::inspect(
            &state.prototype_directory,
            &state.prototype_paths,
            &intent.preview.worktree,
        )
        .await?;
        if !reconcile {
            let preview = &intent.preview;
            let commit = local_commit::prepare(
                &repository,
                &preview.snapshot,
                &preview.author_name,
                &preview.author_email,
                &preview.message,
            )
            .await?;
            let expected = card.clone();
            let operation_id = intent.id.clone();
            let reserved_commit = commit.clone();
            state
                .database
                .call(move |store| {
                    workflow::sending(store, &expected, &operation_id, reserved_commit)
                })
                .await?;
            let _ = state.changed.send(());
            local_commit::publish(&repository, &preview.snapshot, &commit).await?;
        }
        let observed = local_commit::current(&repository, &intent.preview.snapshot.branch).await?;
        let operation_id = intent.id.clone();
        let card_id = id.clone();
        state
            .database
            .call(move |store| workflow::record(store, &card_id, &operation_id, observed))
            .await
    };
    let mut stopped = state.shutdown.subscribe();
    let stopping = *stopped.borrow();
    let result = if stopping {
        drop(work);
        Err("Atelier est en cours d’arrêt.".into())
    } else {
        tokio::select! {
            result = tokio::time::timeout(std::time::Duration::from_secs(180), work) => result.unwrap_or_else(|_| Err("Commit interrompu. Vérifie sa branche avant toute nouvelle tentative.".into())),
            _ = stopped.changed() => Err("Atelier est en cours d’arrêt.".into()),
        }
    };
    let result = match result {
        Err(error) if !reconcile => {
            let operation_id = intent.id;
            match state
                .database
                .call(move |store| workflow::interrupted(store, &id, &operation_id))
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
