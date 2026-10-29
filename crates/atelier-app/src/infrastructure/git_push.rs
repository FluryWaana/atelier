use super::{
    files::Repository,
    forge, forge_association,
    git_transport::{HttpsAuthentication, PreparedPush},
    store::{Documents, Store},
};
use crate::Result;
use atelier_core::contracts::{
    Card, ForgeConfiguration, ForgeWriteState as Phase, GitPushIntent, GitPushPreview,
    GitPushRequest, PullRequest, Worktree,
};
use cap_std::fs::Dir;
use std::path::PathBuf;

async fn origin(repository: &Repository) -> Result<String> {
    Ok(String::from_utf8(
        repository
            .git(&[
                "config",
                "--local",
                "--no-includes",
                "--get",
                "remote.origin.url",
            ])
            .await?,
    )?
    .trim_end_matches(['\r', '\n'])
    .into())
}
async fn head(repository: &Repository) -> Result<String> {
    Ok(String::from_utf8(
        repository
            .git(&["rev-parse", "--verify", "HEAD^{commit}"])
            .await?,
    )?
    .trim()
    .into())
}
pub async fn preview(
    root: &Dir,
    roots: &[PathBuf],
    card: &Card,
    configuration: ForgeConfiguration,
    token: String,
) -> Result<GitPushPreview> {
    let linked = card
        .fields
        .get::<PullRequest>("pullRequest")?
        .ok_or("message.link_a_pr_mr_before_preparing_the_push")?;
    if linked.kind != configuration.kind || linked.base_url != configuration.base {
        return Err("message.the_pr_mr_connection_changed_read_its_link_again".into());
    }
    let repository =
        forge_association::verify(root, roots, card, &configuration, &linked.repository).await?;
    let remote = forge::Forge::new(configuration.clone(), token.clone(), linked.revision)?
        .pull_request(&linked.repository, linked.number)
        .await?;
    if !forge::same_pull_request(&linked, &remote)
        || remote.fork
        || remote.status != "active"
        || remote.source_branch == remote.target_branch
    {
        return Err("message.read_the_open_pr_mr_for_the_task_branch_before_pushing".into());
    }
    let worktree = card
        .fields
        .get::<Worktree>("worktree")?
        .ok_or("message.push_worktree_missing")?;
    if remote.source_branch != worktree.branch
        || remote.source_branch != linked.source_branch
        || remote.target_branch != linked.target_branch
    {
        return Err(
            "message.the_pr_mr_branches_changed_check_its_association_before_pushing".into(),
        );
    }
    let previous = remote
        .source_commit
        .clone()
        .ok_or("message.remote_source_commit_unavailable")?;
    let commit = head(&repository).await?;
    if commit == previous {
        return Err("message.no_new_commit_to_push".into());
    }
    repository
        .git(&["merge-base", "--is-ancestor", &previous, &commit])
        .await
        .map_err(|_| "message.push_refused_fetch_and_integrate_the_remote_branch_first")?;
    let authentication = HttpsAuthentication::new(configuration.clone(), linked.repository, token)?;
    let origin = authentication.origin(&origin(&repository).await?)?;
    if authentication
        .remote_head(&origin, &format!("refs/heads/{}", remote.source_branch))
        .await?
        != previous
    {
        return Err("message.the_remote_branch_changed_read_the_pr_mr_again".into());
    }
    let local_changes = !repository
        .without_programs(&["status", "--porcelain=v1", "--untracked-files=all"])
        .await?
        .is_empty();
    if head(&repository).await? != commit {
        return Err("message.head_changed_during_the_push_preview".into());
    }
    Ok(GitPushPreview {
        configuration,
        revision: remote.revision,
        repository: remote.repository.clone(),
        worktree,
        pull_request: Some(remote),
        origin,
        commit,
        previous: Some(previous),
        local_changes,
    })
}
pub fn save_preview(
    store: &Store<'_>,
    expected: &Card,
    revision: u64,
    preview: GitPushPreview,
) -> Result<Card> {
    store.transaction(|store| {
        forge::require_revision(store, revision)?;
        let mut card = forge_association::snapshot(store, expected)?;
        if card.fields.get::<Worktree>("worktree")?.as_ref() != Some(&preview.worktree) {
            return Err("message.the_worktree_changed_during_the_push_preview".into());
        }
        card.fields.set("gitPushPreview", &preview)?;
        store.put(Documents::Cards, &card)?;
        Ok(card)
    })
}
pub fn reserve(store: &Store<'_>, input: &GitPushRequest, id: &str) -> Result<Card> {
    let preview = input
        .expected
        .fields
        .get::<GitPushPreview>("gitPushPreview")?
        .ok_or("message.prepare_a_preview_before_pushing")?;
    reserve_preview(store, input, id, preview)
}
pub(super) fn reserve_preview(
    store: &Store<'_>,
    input: &GitPushRequest,
    id: &str,
    preview: GitPushPreview,
) -> Result<Card> {
    store.transaction(|store| {
        if !input.confirmed || uuid::Uuid::parse_str(id).is_err() {
            return Err("message.confirm_the_commit_and_branch_to_push".into());
        }
        let mut card = forge_association::snapshot(store, &input.expected)?;
        let configuration =
            forge::configuration(store, preview.configuration.kind, input.revision)?;
        let linked = card.fields.get::<PullRequest>("pullRequest")?;
        let link_matches = match (linked.as_ref(), preview.pull_request.as_ref()) {
            (Some(linked), Some(expected)) => {
                forge::same_pull_request(linked, expected)
                    && expected.kind == preview.configuration.kind
                    && expected.base_url == preview.configuration.base
                    && expected.repository == preview.repository
                    && expected.revision == preview.revision
                    && expected.source_branch == preview.worktree.branch
                    && expected.source_commit == preview.previous
            }
            (None, None) => true,
            _ => false,
        };
        if preview.revision != input.revision
            || preview.configuration.base != configuration.base
            || card.fields.get::<Worktree>("worktree")?.as_ref() != Some(&preview.worktree)
            || !link_matches
        {
            return Err("message.the_push_preview_is_stale_prepare_it_again".into());
        }
        card.fields.set(
            "gitPush",
            &GitPushIntent {
                id: id.into(),
                state: Phase::Preparing,
                preview,
                observed: None,
            },
        )?;
        store.put(Documents::Cards, &card)?;
        Ok(card)
    })
}
pub fn require_pull_request(preview: &GitPushPreview, remote: &PullRequest) -> Result<()> {
    let expected = preview
        .pull_request
        .as_ref()
        .ok_or("message.push_pr_mr_missing")?;
    if !forge::same_pull_request(expected, remote)
        || remote.revision != preview.revision
        || remote.source_branch != preview.worktree.branch
        || remote.source_commit != preview.previous
        || remote.status != "active"
        || remote.fork
    {
        return Err(
            "message.the_pr_mr_or_its_commits_changed_after_the_preview_read_it_again".into(),
        );
    }
    Ok(())
}

pub fn require_pushed_pull_request(preview: &GitPushPreview, remote: &PullRequest) -> Result<()> {
    let mut comparable = remote.clone();
    comparable.source_commit = preview.previous.clone();
    require_pull_request(preview, &comparable)?;
    if remote.source_commit.as_deref() != Some(preview.commit.as_str()) {
        return Err(
            "message.the_pr_mr_does_not_expose_the_pushed_commit_check_its_commits_be".into(),
        );
    }
    Ok(())
}

pub async fn prepare(
    root: &Dir,
    roots: &[PathBuf],
    card: &Card,
    configuration: &ForgeConfiguration,
    authentication: &HttpsAuthentication,
) -> Result<PreparedPush> {
    let intent = card
        .fields
        .get::<GitPushIntent>("gitPush")?
        .ok_or("message.push_intent_missing")?;
    let preview = intent.preview;
    let repository =
        forge_association::verify(root, roots, card, configuration, &preview.repository).await?;
    if card.fields.get::<Worktree>("worktree")?.as_ref() != Some(&preview.worktree)
        || head(&repository).await? != preview.commit
        || authentication.origin(&origin(&repository).await?)? != preview.origin
    {
        return Err("message.the_repository_or_head_changed_prepare_a_new_push_preview".into());
    }
    authentication
        .prepare_push(
            &repository,
            &preview.origin,
            &format!("refs/heads/{}", preview.worktree.branch),
            &preview.commit,
            preview.previous.as_deref(),
        )
        .await
}
pub fn sending(store: &Store<'_>, expected: &Card, id: &str) -> Result<()> {
    store.transaction(|store| {
        let (mut card, mut intent) = active(store, &expected.id, id)?;
        forge::require_revision(store, intent.preview.revision)?;
        if intent.state != Phase::Preparing
            || serde_json::to_value(atelier_core::application::jira::card_snapshot(&card))?
                != serde_json::to_value(atelier_core::application::jira::card_snapshot(expected))?
        {
            return Err("message.the_push_intent_changed".into());
        }
        intent.state = Phase::Sending;
        card.fields.set("gitPush", &intent)?;
        forge_association::invalidate(
            store,
            &mut card,
            "message.a_push_started_read_the_pr_mr_again_and_revalidate_its_reviews",
        )?;
        store.put(Documents::Cards, &card)
    })
}
fn active(store: &Store<'_>, card_id: &str, id: &str) -> Result<(Card, GitPushIntent)> {
    let card = store
        .document::<Card>(Documents::Cards, card_id)?
        .ok_or("message.push_card_missing")?;
    let intent = card
        .fields
        .get::<GitPushIntent>("gitPush")?
        .filter(|intent| intent.id == id)
        .ok_or("message.the_push_intent_changed")?;
    Ok((card, intent))
}
pub fn interrupted(store: &Store<'_>, card_id: &str, id: &str) -> Result<Card> {
    store.transaction(|store| {
        let (mut card, mut intent) = active(store, card_id, id)?;
        intent.state = match intent.state {
            Phase::Preparing => Phase::Failed,
            Phase::Sending => Phase::Uncertain,
            _ => return Err("message.the_push_intent_changed".into()),
        };
        card.fields.set("gitPush", &intent)?;
        store.put(Documents::Cards, &card)?;
        Ok(card)
    })
}
pub fn record(
    store: &Store<'_>,
    card_id: &str,
    id: &str,
    observed: String,
    revision: u64,
    reconciled: bool,
) -> Result<Card> {
    store.transaction(|store| {
        let (mut card, mut intent) = active(store, card_id, id)?;
        let configuration =
            forge::configuration(store, intent.preview.configuration.kind, revision)?;
        if !matches!(intent.state, Phase::Sending | Phase::Uncertain)
            || configuration.base != intent.preview.configuration.base
        {
            return Err("message.the_push_intent_changed".into());
        }
        if ![40, 64].contains(&observed.len())
            || !observed.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err("message.invalid_remote_commit".into());
        }
        intent.observed = Some(observed.clone());
        intent.state =
            if reconciled && Some(observed.as_str()) != intent.preview.previous.as_deref() {
                // A delayed request with the old lease cannot replace this different remote value.
                Phase::Reconciled
            } else if observed == intent.preview.commit {
                Phase::Confirmed
            } else {
                Phase::Uncertain
            };
        if matches!(intent.state, Phase::Confirmed | Phase::Reconciled) {
            if intent.preview.pull_request.is_none()
                && let Some(mut publication) = card
                    .fields
                    .get::<atelier_core::contracts::ForgePublicationPreview>(
                    "forgePublicationPreview",
                )?
                && publication.configuration.kind == intent.preview.configuration.kind
                && publication.configuration.base == intent.preview.configuration.base
                && publication.repository == intent.preview.repository
                && publication.worktree == intent.preview.worktree
                && publication.source_commit == intent.preview.commit
            {
                publication.remote_source = Some(observed.clone());
                card.fields.set("forgePublicationPreview", &publication)?;
            }
            card.fields.set("gitPushEffect", &intent)?;
            card.fields.remove("gitPushPreview");
        }
        card.fields.set("gitPush", &intent)?;
        store.put(Documents::Cards, &card)?;
        Ok(card)
    })
}
