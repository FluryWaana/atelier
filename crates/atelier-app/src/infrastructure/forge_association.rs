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
    ForgeFailure::OperationConflict("La tâche ou sa PR/MR a changé. Recharge avant de réessayer.")
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
            return Err("Arrête l’exécution avant de modifier la liaison de PR/MR.".into());
        }
    }
    for entry in store.documents::<Document>(Documents::Times)? {
        if entry.string_equals("cardId", &card.id)?
            && (entry.string_equals("exportState", "sending")?
                || entry.string_equals("exportState", "uncertain")?)
        {
            return Err(
                "Termine ou vérifie l’export de temps avant de modifier cette tâche.".into(),
            );
        }
    }
    Ok(card)
}
pub fn prepare(store: &Store<'_>, input: &PullRequestAttach) -> Result<(Card, ForgeConfiguration)> {
    if input.expected.fields.raw("connectorPullRequest").is_some() {
        return Err(
            "Retire la liaison de PR/MR du module avant de choisir une forge native.".into(),
        );
    }
    if !input.confirmed {
        return Err("Confirme l’association de cette PR/MR à la tâche.".into());
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
            .map_err(|_| "Adresse de forge invalide.")?;
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
                .ok_or("Chemin Azure invalide.")?
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
        .ok_or("Prépare le worktree avant de lier une PR/MR.")?;
    if worktree.state != WorktreeState::Ready
        || !atelier_core::domain::projects::same_repository(
            &card.fields.get::<String>("repo")?.unwrap_or_default(),
            &worktree.source,
            cfg!(windows),
        )
    {
        return Err("Le dépôt source ne correspond plus au worktree de la PR/MR.".into());
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
    .map_err(|_| "Remote origin de la PR/MR invalide.")?;
    if !matches_origin(configuration, repository, &origin)? {
        return Err("Le remote origin ne correspond pas au dépôt de cette PR/MR.".into());
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
            return Err("Cette PR/MR ne peut pas être associée à cette tâche.".into());
        }
        let worktree = card
            .fields
            .get::<Worktree>("worktree")?
            .ok_or("Worktree de PR/MR absent.")?;
        if worktree.state != WorktreeState::Ready || pr.source_branch != worktree.branch {
            return Err("La branche source de la PR/MR ne correspond pas au worktree.".into());
        }
        let previous = card.fields.get::<PullRequest>("pullRequest")?;
        if previous.as_ref().is_some_and(|previous| {
            previous.kind != pr.kind
                || previous.base_url != pr.base_url
                || previous.repository != pr.repository
                || previous.number != pr.number
        }) {
            return Err("Retire la liaison précédente avant d’associer une autre PR/MR.".into());
        }
        for other in store.documents::<Card>(Documents::Cards)? {
            if other.id != card.id
                && let Some(link) = other.fields.get::<PullRequest>("pullRequest")?
                && link.kind == pr.kind
                && link.base_url == pr.base_url
                && link.repository == pr.repository
                && link.number == pr.number
            {
                return Err("Cette PR/MR est déjà associée à une autre tâche.".into());
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
                "La PR/MR ou ses commits ont changé. Relance la review.",
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
            return Err("Confirme le retrait de la liaison de PR/MR.".into());
        }
        let mut card = snapshot(store, &input.expected)?;
        if card.fields.raw("pullRequest").is_none()
            && card.fields.raw("connectorPullRequest").is_none()
        {
            return Err("Cette tâche n’a aucune PR/MR associée.".into());
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
            "La liaison de PR/MR a été retirée. Relance la review.",
        )?;
        store.put(Documents::Cards, &card)?;
        Ok(card)
    })
}
