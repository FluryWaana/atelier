use super::{
    forge, forge_association,
    git_transport::HttpsAuthentication,
    store::{Documents, Store},
};
use crate::Result;
use atelier_core::contracts::{
    Card, ForgeConfiguration, ForgeKind, ForgePublicationIntent, ForgePublicationPreview,
    ForgePublicationRequest, ForgeWriteState as Phase, PullRequest, Worktree,
};
use cap_std::fs::Dir;
use std::path::PathBuf;

fn validate(input: &ForgePublicationRequest) -> Result<String> {
    atelier_core::application::forge::repository(input.kind, &input.repository, 1)?;
    if input.title.trim().is_empty()
        || input.title.encode_utf16().count() > 255
        || input.title.chars().any(char::is_control)
        || input.body.encode_utf16().count() > 3500
        || input
            .body
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\r' | '\n' | '\t'))
    {
        return Err("Titre ou description de publication invalide ou trop long.".into());
    }
    atelier_core::application::worktrees::validate_reference(&input.target)?;
    let prefixed = ["draft:", "wip:", "[draft]", "(draft)", "[wip]", "(wip)"]
        .iter()
        .any(|prefix| {
            input
                .title
                .trim_start()
                .to_ascii_lowercase()
                .starts_with(prefix)
        });
    if input.kind == ForgeKind::Gitlab && !input.draft && prefixed {
        return Err(
            "Retire le préfixe brouillon du titre pour créer une MR prête pour review.".into(),
        );
    }
    let title = if input.kind == ForgeKind::Gitlab && input.draft && !prefixed {
        format!("Draft: {}", input.title)
    } else {
        input.title.clone()
    };
    if title.encode_utf16().count() > 255 {
        return Err("Titre de publication trop long avec son préfixe brouillon.".into());
    }
    Ok(title)
}
pub async fn preview(
    root: &Dir,
    roots: &[PathBuf],
    input: &ForgePublicationRequest,
    configuration: ForgeConfiguration,
    token: String,
) -> Result<ForgePublicationPreview> {
    let title = validate(input)?;
    let worktree = input
        .expected
        .fields
        .get::<Worktree>("worktree")?
        .ok_or("Prépare le worktree avant de publier une PR/MR.")?;
    if worktree.branch == input.target || input.expected.fields.raw("pullRequest").is_some() {
        return Err(
            "Cette publication exige une branche de tâche distincte et aucune PR/MR déjà liée."
                .into(),
        );
    }
    let directory = forge_association::verify(
        root,
        roots,
        &input.expected,
        &configuration,
        &input.repository,
    )
    .await?;
    directory
        .git(&["check-ref-format", &format!("refs/heads/{}", input.target)])
        .await?;
    let head = String::from_utf8(
        directory
            .git(&["rev-parse", "--verify", "HEAD^{commit}"])
            .await?,
    )?
    .trim()
    .to_owned();
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
    )?;
    let authentication = HttpsAuthentication::new(
        configuration.clone(),
        input.repository.clone(),
        token.clone(),
    )?;
    let origin = authentication.origin(&origin)?;
    let remote_source = authentication
        .remote_head_optional(&origin, &format!("refs/heads/{}", worktree.branch))
        .await?;
    if let Some(previous) = remote_source.as_deref()
        && previous != head
    {
        directory
            .git(&["merge-base", "--is-ancestor", previous, &head])
            .await
            .map_err(|_| "Push refusé : récupère et intègre d’abord la branche distante.")?;
    }
    let source_commit = head;
    let target_commit = authentication
        .remote_head(&origin, &format!("refs/heads/{}", input.target))
        .await?;
    if source_commit == target_commit {
        return Err("Les branches de publication désignent le même commit.".into());
    }
    let preview = ForgePublicationPreview {
        configuration: configuration.clone(),
        revision: input.revision,
        repository: input.repository.clone(),
        worktree,
        target: input.target.clone(),
        origin,
        remote_source,
        local_changes: !directory
            .without_programs(&["status", "--porcelain=v1", "--untracked-files=all"])
            .await?
            .is_empty(),
        source_commit,
        target_commit,
        title,
        body: input.body.clone(),
        draft: input.draft,
    };
    // The same adapter preflight is repeated immediately before dispatch, without writing here.
    forge::Forge::new(configuration, token, input.revision)?
        .prepare_publication(&preview, &uuid::Uuid::new_v4().to_string())
        .await?;
    if String::from_utf8(
        directory
            .git(&["rev-parse", "--verify", "HEAD^{commit}"])
            .await?,
    )?
    .trim()
        != preview.source_commit
    {
        return Err("HEAD a changé pendant l’aperçu de publication.".into());
    }
    Ok(preview)
}
pub fn save_preview(
    store: &Store<'_>,
    expected: &Card,
    preview: ForgePublicationPreview,
) -> Result<Card> {
    store.transaction(|store| {
        forge::require_revision(store, preview.revision)?;
        let mut card = forge_association::snapshot(store, expected)?;
        if card.fields.get::<Worktree>("worktree")?.as_ref() != Some(&preview.worktree)
            || card.fields.raw("pullRequest").is_some()
        {
            return Err("La tâche a changé pendant l’aperçu de publication.".into());
        }
        card.fields.set("forgePublicationPreview", &preview)?;
        store.put(Documents::Cards, &card)?;
        Ok(card)
    })
}
pub fn reserve(store: &Store<'_>, input: &ForgePublicationRequest, id: &str) -> Result<Card> {
    store.transaction(|store| {
        forge::publication_marker(id)?;
        let (mut card, preview) = checked_preview(store, input)?;
        if preview.remote_source.as_deref() != Some(preview.source_commit.as_str()) {
            return Err(
                "Pousse la branche confirmée puis actualise l’aperçu avant de créer la PR/MR."
                    .into(),
            );
        }
        card.fields.set(
            "forgePublication",
            &ForgePublicationIntent {
                id: id.into(),
                state: Phase::Preparing,
                preview,
                number: None,
                result: None,
            },
        )?;
        store.put(Documents::Cards, &card)?;
        Ok(card)
    })
}
fn checked_preview(
    store: &Store<'_>,
    input: &ForgePublicationRequest,
) -> Result<(Card, ForgePublicationPreview)> {
    if !input.confirmed {
        return Err("Confirme les branches et le contenu de la PR/MR à créer.".into());
    }
    let card = forge_association::snapshot(store, &input.expected)?;
    let preview = card
        .fields
        .get::<ForgePublicationPreview>("forgePublicationPreview")?
        .ok_or("Prépare un aperçu avant de créer la PR/MR.")?;
    let configuration = forge::configuration(store, input.kind, input.revision)?;
    if preview.configuration.kind != input.kind
        || preview.configuration.base != configuration.base
        || preview.revision != input.revision
        || preview.repository != input.repository
        || preview.target != input.target
        || preview.title != validate(input)?
        || preview.body != input.body
        || preview.draft != input.draft
        || card.fields.get::<Worktree>("worktree")?.as_ref() != Some(&preview.worktree)
        || card.fields.raw("pullRequest").is_some()
    {
        return Err("L’aperçu de publication est périmé. Prépare-le à nouveau.".into());
    }
    Ok((card, preview))
}
pub fn reserve_branch(
    store: &Store<'_>,
    input: &ForgePublicationRequest,
    id: &str,
) -> Result<Card> {
    let (card, preview) = checked_preview(store, input)?;
    if preview.remote_source.as_deref() == Some(preview.source_commit.as_str()) {
        return Err("La branche est déjà à jour. Actualise l’aperçu de publication.".into());
    }
    let push = atelier_core::contracts::GitPushPreview {
        configuration: preview.configuration,
        revision: preview.revision,
        repository: preview.repository,
        worktree: preview.worktree,
        pull_request: None,
        origin: preview.origin,
        commit: preview.source_commit,
        previous: preview.remote_source,
        local_changes: preview.local_changes,
    };
    super::git_push::reserve_preview(
        store,
        &atelier_core::contracts::GitPushRequest {
            expected: card,
            revision: input.revision,
            confirmed: input.confirmed,
        },
        id,
        push,
    )
}
pub async fn prepare(
    root: &Dir,
    roots: &[PathBuf],
    card: &Card,
    configuration: &ForgeConfiguration,
    authentication: &HttpsAuthentication,
) -> Result<()> {
    let intent = card
        .fields
        .get::<ForgePublicationIntent>("forgePublication")?
        .ok_or("Intention de publication absente.")?;
    let preview = intent.preview;
    if preview.remote_source.as_deref() != Some(preview.source_commit.as_str()) {
        return Err(
            "Pousse la branche confirmée puis actualise l’aperçu avant de créer la PR/MR.".into(),
        );
    }
    let directory =
        forge_association::verify(root, roots, card, configuration, &preview.repository).await?;
    if card.fields.get::<Worktree>("worktree")?.as_ref() != Some(&preview.worktree) {
        return Err("Le worktree de publication a changé.".into());
    }
    let head = String::from_utf8(
        directory
            .git(&["rev-parse", "--verify", "HEAD^{commit}"])
            .await?,
    )?
    .trim()
    .to_owned();
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
    )?;
    if head != preview.source_commit
        || authentication
            .remote_head(&origin, &format!("refs/heads/{}", preview.worktree.branch))
            .await?
            != preview.source_commit
        || authentication
            .remote_head(&origin, &format!("refs/heads/{}", preview.target))
            .await?
            != preview.target_commit
    {
        return Err("Les commits de publication ont changé. Prépare un nouvel aperçu.".into());
    }
    Ok(())
}
fn active(store: &Store<'_>, card_id: &str, id: &str) -> Result<(Card, ForgePublicationIntent)> {
    let card = store
        .document::<Card>(Documents::Cards, card_id)?
        .ok_or("Carte de publication absente.")?;
    let intent = card
        .fields
        .get::<ForgePublicationIntent>("forgePublication")?
        .filter(|intent| intent.id == id)
        .ok_or("L’intention de publication a changé.")?;
    Ok((card, intent))
}
pub fn sending(store: &Store<'_>, expected: &Card, id: &str) -> Result<()> {
    store.transaction(|store| {
        let (mut card, mut intent) = active(store, &expected.id, id)?;
        forge::require_revision(store, intent.preview.revision)?;
        if intent.state != Phase::Preparing
            || serde_json::to_value(atelier_core::application::jira::card_snapshot(&card))?
                != serde_json::to_value(atelier_core::application::jira::card_snapshot(expected))?
        {
            return Err("L’intention de publication a changé.".into());
        }
        intent.state = Phase::Sending;
        card.fields.set("forgePublication", &intent)?;
        forge_association::invalidate(
            store,
            &mut card,
            "Une publication a démarré. Relis la PR/MR avant toute review.",
        )?;
        store.put(Documents::Cards, &card)
    })
}
pub fn remember_number(store: &Store<'_>, card_id: &str, id: &str, number: u64) -> Result<()> {
    store.transaction(|store| {
        let (mut card, mut intent) = active(store, card_id, id)?;
        if intent.state != Phase::Sending || number == 0 || number > i32::MAX as u64 {
            return Err("Identité de publication invalide.".into());
        }
        intent.number = Some(number);
        card.fields.set("forgePublication", &intent)?;
        store.put(Documents::Cards, &card)
    })
}
pub fn interrupted(store: &Store<'_>, card_id: &str, id: &str) -> Result<Card> {
    store.transaction(|store| {
        let (mut card, mut intent) = active(store, card_id, id)?;
        intent.state = match intent.state {
            Phase::Preparing => Phase::Failed,
            Phase::Sending => Phase::Uncertain,
            _ => return Err("L’intention de publication a changé.".into()),
        };
        card.fields.set("forgePublication", &intent)?;
        store.put(Documents::Cards, &card)?;
        Ok(card)
    })
}
pub fn record(store: &Store<'_>, card_id: &str, id: &str, pr: PullRequest) -> Result<Card> {
    store.transaction(|store| {
        let (mut card, mut intent) = active(store, card_id, id)?;
        let preview = &intent.preview;
        let configuration = forge::configuration(store, preview.configuration.kind, pr.revision)?;
        if !matches!(intent.state, Phase::Sending | Phase::Uncertain)
            || configuration.base != preview.configuration.base
            || pr.kind != configuration.kind
            || pr.base_url != configuration.base
            || pr.repository != preview.repository
            || pr.source_branch != preview.worktree.branch
            || pr.target_branch != preview.target
            || pr.fork
            || intent.number.is_some_and(|number| number != pr.number)
            || pr.url != forge::browser_link(&configuration, &preview.repository, pr.number)?
            || card.fields.raw("pullRequest").is_some()
        {
            return Err("La PR/MR retrouvée ne correspond pas à cette publication.".into());
        }
        let mut duplicate = false;
        for other in store.documents::<Card>(Documents::Cards)? {
            if other.id != card.id
                && let Some(linked) = other.fields.get::<PullRequest>("pullRequest")?
            {
                duplicate |= linked.kind == pr.kind
                    && linked.base_url == pr.base_url
                    && linked.repository == pr.repository
                    && linked.number == pr.number;
            }
        }
        intent.number = Some(pr.number);
        intent.result = Some(pr.clone());
        intent.state = if duplicate {
            Phase::Uncertain
        } else {
            Phase::Confirmed
        };
        if !duplicate {
            card.fields.set("pullRequest", &pr)?;
            card.fields.set("forgePublicationEffect", &intent)?;
            card.fields.remove("forgePublicationPreview");
        }
        card.fields.set("forgePublication", &intent)?;
        forge_association::invalidate(
            store,
            &mut card,
            "La PR/MR publiée a été relue. Les reviews doivent porter sur ses commits actuels.",
        )?;
        store.put(Documents::Cards, &card)?;
        Ok(card)
    })
}
