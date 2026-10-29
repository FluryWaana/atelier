use super::{
    files::Repository,
    forge, runs,
    store::{Documents, Store},
    worktrees,
};
use crate::Result;
use atelier_core::{
    application::{
        forge::ForgeFailure, jira::card_snapshot, task_creation::CreationContext, task_editing,
    },
    contracts::{
        Card, ForgeConfiguration, ForgeKind, PullRequest, PullRequestAttach, PullRequestDetach,
        Worktree, WorktreeState,
    },
    document::Document,
};
use cap_std::fs::Dir;
use reqwest::Url;
use std::path::PathBuf;
fn conflict() -> crate::Error {
    ForgeFailure::OperationConflict(
        "message.the_task_or_its_pr_mr_changed_reload_before_trying_again",
    )
    .into()
}
pub fn snapshot(store: &Store<'_>, expected: &Card) -> Result<Card> {
    let card = store
        .document::<Card>(Documents::Cards, &expected.id)?
        .ok_or_else(conflict)?;
    if serde_json::to_value(card_snapshot(&card))? != serde_json::to_value(card_snapshot(expected))?
        || card.fields.truthy("archived")?
        || card.fields.truthy("brainstormSession")?
        || atelier_core::application::jira::unresolved(&card.fields)?
        || atelier_core::application::forge::unresolved(&card.fields)?
    {
        return Err(conflict());
    }
    for run in store.documents::<Document>(Documents::Runs)? {
        if run.string_equals("cardId", &card.id)? && runs::active(&run)? {
            return Err("message.stop_the_run_before_changing_the_pr_mr_link".into());
        }
    }
    for entry in store.documents::<Document>(Documents::Times)? {
        if entry.string_equals("cardId", &card.id)?
            && (entry.string_equals("exportState", "sending")?
                || entry.string_equals("exportState", "uncertain")?)
        {
            return Err("message.finish_or_check_the_time_export_before_changing_this_task".into());
        }
    }
    Ok(card)
}
pub fn prepare(store: &Store<'_>, input: &PullRequestAttach) -> Result<(Card, ForgeConfiguration)> {
    if input.expected.fields.raw("connectorPullRequest").is_some() {
        return Err("message.remove_connector_pr_before_native_forge".into());
    }
    if !input.confirmed {
        return Err("message.confirm_linking_this_pr_mr_to_the_task".into());
    }
    atelier_core::application::forge::repository(input.kind, &input.repository, input.number)?;
    Ok((
        snapshot(store, &input.expected)?,
        forge::configuration(store, input.kind, input.revision)?,
    ))
}

pub fn matches_origin(
    configuration: &ForgeConfiguration,
    repository: &str,
    origin: &str,
) -> Result<bool> {
    let parts = atelier_core::application::forge::repository(configuration.kind, repository, 1)?;
    let base = Url::parse(&configuration.base)?;
    let mut web = base.clone();
    {
        let mut path = web
            .path_segments_mut()
            .map_err(|_| "message.invalid_forge_address")?;
        path.pop_if_empty();
        for (index, part) in parts.iter().enumerate() {
            if configuration.kind == ForgeKind::Azure && index == 1 {
                path.push("_git");
            }
            path.push(part);
        }
    }
    let origin = origin.trim_end_matches(['\r', '\n']);
    if origin.chars().any(char::is_control) {
        return Ok(false);
    }
    let remote = if let Some(scp) = origin.strip_prefix("git@") {
        let Some((host, path)) = scp.split_once(':') else {
            return Ok(false);
        };
        if host.is_empty()
            || host
                .chars()
                .any(|c| c.is_whitespace() || matches!(c, '/' | '\\' | '@' | '?' | '#'))
        {
            return Ok(false);
        }
        Url::parse(&format!("ssh://git@{host}/{path}"))
    } else {
        Url::parse(origin)
    };
    let Ok(remote) = remote else {
        return Ok(false);
    };
    if remote.password().is_some() || remote.query().is_some() || remote.fragment().is_some() {
        return Ok(false);
    }
    if remote.scheme() == "https" {
        let azure_username = configuration.kind == ForgeKind::Azure
            && base
                .path_segments()
                .and_then(|mut parts| parts.next())
                .is_some_and(|organization| organization.eq_ignore_ascii_case(remote.username()));
        if (!remote.username().is_empty() && !azure_username) || remote.origin() != web.origin() {
            return Ok(false);
        }
        let actual = remote.path().trim_end_matches('/');
        let expected = web.path().trim_end_matches('/');
        return Ok(actual == expected || actual.strip_suffix(".git") == Some(expected));
    }
    if remote.scheme() != "ssh"
        || remote.username() != "git"
        || remote.port().is_some_and(|port| port != 22)
    {
        return Ok(false);
    }
    let expected_path = if configuration.kind == ForgeKind::Azure {
        if remote.host_str() != Some("ssh.dev.azure.com") {
            return Ok(false);
        }
        {
            let segments = web
                .path_segments()
                .ok_or("message.invalid_azure_path")?
                .collect::<Vec<_>>();
            if segments.len() != 4 {
                return Ok(false);
            }
            format!("/v3/{}/{}/{}", segments[0], segments[1], segments[3])
        }
    } else {
        if remote.host_str() != base.host_str()
            || base.port_or_known_default() != Some(443)
            || !base.path().trim_matches('/').is_empty()
        {
            return Ok(false);
        }
        web.path().into()
    };
    let actual = remote.path().trim_end_matches('/');
    Ok(actual == expected_path || actual.strip_suffix(".git") == Some(expected_path.as_str()))
}
pub async fn verify(
    root: &Dir,
    roots: &[PathBuf],
    card: &Card,
    configuration: &ForgeConfiguration,
    repository: &str,
) -> Result<Repository> {
    let worktree = card
        .fields
        .get::<Worktree>("worktree")?
        .ok_or("message.prepare_the_worktree_before_linking_a_pr_mr")?;
    let repository_path = card.fields.get::<String>("repo")?.unwrap_or_default();
    let repository_path =
        super::task_creation::path_string(std::path::Path::new(&repository_path)).utf8()?;
    let source_path =
        super::task_creation::path_string(std::path::Path::new(&worktree.source)).utf8()?;
    if worktree.state != WorktreeState::Ready
        || !atelier_core::domain::projects::same_repository(
            &repository_path,
            &source_path,
            cfg!(windows),
        )
    {
        return Err("message.the_source_repository_no_longer_matches_the_pr_mr_worktree".into());
    }
    let directory = worktrees::inspect(root, roots, &worktree).await?;
    let origin = String::from_utf8(
        directory
            .git(&[
                "config",
                "--local",
                "--no-includes",
                "--get",
                "remote.origin.url",
            ])
            .await?,
    )
    .map_err(|_| "message.invalid_pr_mr_origin_remote")?;
    if !matches_origin(configuration, repository, &origin)? {
        return Err("message.the_origin_remote_does_not_match_this_pr_mr_repository".into());
    }
    Ok(directory)
}
pub fn attach(store: &Store<'_>, input: &PullRequestAttach, pr: PullRequest) -> Result<Card> {
    store.transaction(|store| {
        let (mut card, configuration) = prepare(store, input)?;
        if pr.revision != input.revision
            || pr.kind != input.kind
            || pr.repository != input.repository
            || pr.number != input.number
            || pr.base_url != configuration.base
            || pr.url != forge::browser_link(&configuration, &input.repository, input.number)?
            || pr.fork
        {
            return Err("message.this_pr_mr_cannot_be_linked_to_this_task".into());
        }
        let worktree = card
            .fields
            .get::<Worktree>("worktree")?
            .ok_or("message.pr_mr_worktree_missing")?;
        if worktree.state != WorktreeState::Ready || pr.source_branch != worktree.branch {
            return Err("message.the_pr_mr_source_branch_does_not_match_the_worktree".into());
        }
        let previous = card.fields.get::<PullRequest>("pullRequest")?;
        if previous.as_ref().is_some_and(|previous| {
            previous.kind != pr.kind
                || previous.base_url != pr.base_url
                || previous.repository != pr.repository
                || previous.number != pr.number
        }) {
            return Err("message.remove_the_previous_link_before_linking_another_pr_mr".into());
        }
        for other in store.documents::<Card>(Documents::Cards)? {
            if other.id != card.id
                && let Some(link) = other.fields.get::<PullRequest>("pullRequest")?
                && link.kind == pr.kind
                && link.base_url == pr.base_url
                && link.repository == pr.repository
                && link.number == pr.number
            {
                return Err("message.this_pr_mr_is_already_linked_to_another_task".into());
            }
        }
        let changed = previous.as_ref().is_none_or(|previous| {
            previous.source_commit != pr.source_commit
                || previous.target_commit != pr.target_commit
                || previous.source_branch != pr.source_branch
                || previous.target_branch != pr.target_branch
                || previous.status != pr.status
                || previous.draft != pr.draft
        });
        card.fields.set("pullRequest", &pr)?;
        card.fields
            .set("updatedAt", &CreationContext::timestamp(store)?)?;
        if changed {
            invalidate(
                store,
                &mut card,
                "message.the_pr_mr_or_its_commits_changed_run_the_review_again",
            )?;
        }
        store.put(Documents::Cards, &card)?;
        Ok(card)
    })
}
pub(super) fn invalidate(store: &Store<'_>, card: &mut Card, reason: &str) -> Result<()> {
    let invalidation = task_editing::invalidate_reviews(
        &card.id,
        &mut card.fields,
        store.documents(Documents::Runs)?,
        reason,
    )?;
    for run in invalidation.runs {
        store.put(Documents::Runs, &run)?;
    }
    Ok(())
}
pub fn detach(store: &Store<'_>, input: PullRequestDetach) -> Result<Card> {
    store.transaction(|store| {
        if !input.confirmed {
            return Err("message.confirm_removing_the_pr_mr_link".into());
        }
        let mut card = snapshot(store, &input.expected)?;
        if card.fields.raw("pullRequest").is_none()
            && card.fields.raw("connectorPullRequest").is_none()
        {
            return Err("message.this_task_has_no_linked_pr_mr".into());
        }
        card.fields.remove("pullRequest");
        card.fields.remove("connectorPullRequest");
        card.fields.remove("connectorRepositoryPreview");
        card.fields.remove("connectorReviewContext");
        card.fields
            .set("updatedAt", &CreationContext::timestamp(store)?)?;
        invalidate(
            store,
            &mut card,
            "message.the_pr_mr_link_was_removed_run_the_review_again",
        )?;
        store.put(Documents::Cards, &card)?;
        Ok(card)
    })
}
