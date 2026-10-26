use super::Shared;
use crate::{
    Result,
    infrastructure::{
        automation as workflow, forge, forge_association, forge_publication, git_push,
    },
};
use atelier_core::{
    application::automation::PublicationPlan,
    contracts::{
        Card, ForgeKind, ForgePublicationPreview, ForgePublicationRequest, ForgeWriteState,
        GitPushIntent, GitPushPreview, GitPushRequest, PullRequestAttach,
    },
};

async fn current(state: &Shared, id: &str, operation: &str) -> Result<Card> {
    let id = id.to_owned();
    let operation = operation.to_owned();
    state
        .database
        .call(move |store| Ok(workflow::publication(store, &id, &operation)?.0))
        .await
}
async fn credentials(
    state: &Shared,
    kind: ForgeKind,
    revision: u64,
) -> Result<(atelier_core::contracts::ForgeConfiguration, String)> {
    let vault = state.vault.clone();
    state
        .database
        .call(move |store| {
            forge::credentials(store, kind, revision, &mut |value, decrypt| {
                vault.protect(value, decrypt)
            })
        })
        .await
}
fn request(card: Card, preview: &ForgePublicationPreview) -> ForgePublicationRequest {
    ForgePublicationRequest {
        expected: card,
        revision: preview.revision,
        kind: preview.configuration.kind,
        repository: preview.repository.clone(),
        target: preview.target.clone(),
        title: preview.title.clone(),
        body: preview.body.clone(),
        draft: preview.draft,
        confirmed: true,
    }
}
async fn checked_preview(
    state: &Shared,
    card: Card,
    approved: &ForgePublicationPreview,
    pushed: bool,
) -> Result<Card> {
    let (configuration, token) =
        credentials(state, approved.configuration.kind, approved.revision).await?;
    let fresh = forge_publication::preview(
        &state.prototype_directory,
        &state.prototype_paths,
        &request(card.clone(), approved),
        configuration,
        token,
    )
    .await?;
    let mut expected = approved.clone();
    if pushed {
        expected.remote_source = Some(expected.source_commit.clone());
    }
    if serde_json::to_value(&fresh)? != serde_json::to_value(&expected)? {
        return Err(
            "Les branches ou le contenu de publication ont changé depuis leur approbation.".into(),
        );
    }
    state
        .database
        .call(move |store| forge_publication::save_preview(store, &card, fresh))
        .await
}
fn confirmed_push(card: &Card, commit: &str) -> Result<()> {
    if !card
        .fields
        .get::<GitPushIntent>("gitPush")?
        .is_some_and(|intent| {
            intent.state == ForgeWriteState::Confirmed
                && intent.observed.as_deref() == Some(commit)
                && intent.preview.commit == commit
        })
    {
        return Err(
            "Le push du parcours n’est pas confirmé. Relis la branche distante sans renvoyer."
                .into(),
        );
    }
    Ok(())
}
async fn refresh_pushed_pr(state: &Shared, card: Card, approved: &GitPushPreview) -> Result<Card> {
    let original = approved
        .pull_request
        .as_ref()
        .ok_or("PR/MR du push absente.")?;
    let (configuration, token) =
        credentials(state, approved.configuration.kind, approved.revision).await?;
    let remote = forge::Forge::new(configuration.clone(), token, approved.revision)?
        .pull_request(&approved.repository, original.number)
        .await?;
    git_push::require_pushed_pull_request(approved, &remote)?;
    forge_association::verify(
        &state.prototype_directory,
        &state.prototype_paths,
        &card,
        &configuration,
        &approved.repository,
    )
    .await?;
    let input = PullRequestAttach {
        expected: card,
        revision: approved.revision,
        kind: approved.configuration.kind,
        repository: approved.repository.clone(),
        number: original.number,
        confirmed: true,
    };
    state
        .database
        .call(move |store| forge_association::attach(store, &input, remote))
        .await
}

pub(super) async fn perform(state: &Shared, id: &str, operation: &str) -> Result<()> {
    let selected = id.to_owned();
    let op = operation.to_owned();
    let (card, flow, plan) = state
        .database
        .call(move |store| workflow::publication(store, &selected, &op))
        .await?;
    super::automation::tested_sources(state, &card, &flow).await?;
    let commit = match &plan {
        PublicationPlan::Create(preview) => &preview.source_commit,
        PublicationPlan::Push(preview) => &preview.commit,
    };
    let repository = crate::infrastructure::worktrees::inspect(
        &state.prototype_directory,
        &state.prototype_paths,
        &flow.worktree,
    )
    .await?;
    if !crate::infrastructure::local_commit::disk_matches_commit(&repository, commit).await? {
        return Err("Les sources vérifiées ne correspondent pas au commit approuvé. Prépare le commit avant de publier.".into());
    }
    let _permit = state
        .integration_reads
        .try_acquire()
        .map_err(|_| "Les recherches distantes sont occupées. Réessaie dans un instant.")?;
    match plan {
        PublicationPlan::Create(approved) => {
            let card = checked_preview(state, card, &approved, false).await?;
            if approved.remote_source.as_deref() != Some(approved.source_commit.as_str()) {
                let input = request(card.clone(), &approved);
                let card = super::git_push::perform(
                    state,
                    id.into(),
                    GitPushRequest {
                        expected: card,
                        revision: approved.revision,
                        confirmed: true,
                    },
                    false,
                    Some(input),
                )
                .await?;
                confirmed_push(&card, &approved.source_commit)?;
            }
            let card = current(state, id, operation).await?;
            let card = checked_preview(state, card, &approved, true).await?;
            let card = super::forge_publication::perform(
                state,
                id.into(),
                request(card, &approved),
                false,
            )
            .await?;
            let op = operation.to_owned();
            state
                .database
                .call(move |store| workflow::publication_complete(store, &card, &op))
                .await?;
        }
        PublicationPlan::Push(approved) => {
            let (configuration, token) =
                credentials(state, approved.configuration.kind, approved.revision).await?;
            let fresh = git_push::preview(
                &state.prototype_directory,
                &state.prototype_paths,
                &card,
                configuration,
                token,
            )
            .await?;
            let mut comparable = fresh.clone();
            if let (Some(actual), Some(expected)) =
                (&mut comparable.pull_request, &approved.pull_request)
            {
                actual.checked_at = expected.checked_at.clone();
            }
            if serde_json::to_value(&comparable)? != serde_json::to_value(&approved)? {
                return Err("L’aperçu du push a changé depuis l’approbation du parcours.".into());
            }
            let revision = approved.revision;
            let card = state
                .database
                .call(move |store| git_push::save_preview(store, &card, revision, fresh))
                .await?;
            let card = super::git_push::perform(
                state,
                id.into(),
                GitPushRequest {
                    expected: card,
                    revision,
                    confirmed: true,
                },
                false,
                None,
            )
            .await?;
            confirmed_push(&card, &approved.commit)?;
            let card = current(state, id, operation).await?;
            let card = refresh_pushed_pr(state, card, &approved).await?;
            let op = operation.to_owned();
            state
                .database
                .call(move |store| workflow::publication_complete(store, &card, &op))
                .await?;
        }
    }
    Ok(())
}
