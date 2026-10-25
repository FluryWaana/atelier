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
        .ok_or("Associe une PR/MR avant de préparer le push.")?;
    if linked.kind != configuration.kind || linked.base_url != configuration.base {
        return Err("La connexion de PR/MR a changé. Relis sa liaison.".into());
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
        return Err("Relis la PR/MR ouverte de la branche de tâche avant le push.".into());
    }
    let worktree = card
        .fields
        .get::<Worktree>("worktree")?
        .ok_or("Worktree de push absent.")?;
    let previous = remote
        .source_commit
        .clone()
        .ok_or("Commit source distant indisponible.")?;
    let commit = head(&repository).await?;
    if commit == previous {
        return Err("Aucun nouveau commit à pousser.".into());
    }
    repository
        .git(&["merge-base", "--is-ancestor", &previous, &commit])
        .await
        .map_err(|_| "Push refusé : récupère et intègre d’abord la branche distante.")?;
    let authentication = HttpsAuthentication::new(configuration, linked.repository, token)?;
    let origin = authentication.origin(&origin(&repository).await?)?;
    if authentication
        .remote_head(&origin, &format!("refs/heads/{}", remote.source_branch))
        .await?
        != previous
    {
        return Err("La branche distante a changé. Relis la PR/MR.".into());
    }
    let local_changes = !repository
        .without_programs(&["status", "--porcelain=v1", "--untracked-files=all"])
        .await?
        .is_empty();
    if head(&repository).await? != commit {
        return Err("HEAD a changé pendant l’aperçu du push.".into());
    }
    Ok(GitPushPreview {
        worktree,
        pull_request: remote,
        origin,
        commit,
        previous,
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
            return Err("Le worktree a changé pendant l’aperçu du push.".into());
        }
        card.fields.set("gitPushPreview", &preview)?;
        store.put(Documents::Cards, &card)?;
        Ok(card)
    })
}
pub fn reserve(store: &Store<'_>, input: &GitPushRequest, id: &str) -> Result<Card> {
    store.transaction(|store| {
        if !input.confirmed || uuid::Uuid::parse_str(id).is_err() {
            return Err("Confirme le commit et la branche à pousser.".into());
        }
        let mut card = forge_association::snapshot(store, &input.expected)?;
        let preview = card
            .fields
            .get::<GitPushPreview>("gitPushPreview")?
            .ok_or("Prépare un aperçu avant de pousser.")?;
        let configuration = forge::configuration(store, preview.pull_request.kind, input.revision)?;
        if preview.pull_request.revision != input.revision
            || preview.pull_request.base_url != configuration.base
            || card.fields.get::<Worktree>("worktree")?.as_ref() != Some(&preview.worktree)
            || !card
                .fields
                .get::<PullRequest>("pullRequest")?
                .is_some_and(|linked| forge::same_pull_request(&linked, &preview.pull_request))
        {
            return Err("L’aperçu du push est périmé. Prépare-le à nouveau.".into());
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
        .ok_or("Intention de push absente.")?;
    let preview = intent.preview;
    let repository = forge_association::verify(
        root,
        roots,
        card,
        configuration,
        &preview.pull_request.repository,
    )
    .await?;
    if card.fields.get::<Worktree>("worktree")?.as_ref() != Some(&preview.worktree)
        || head(&repository).await? != preview.commit
        || authentication.origin(&origin(&repository).await?)? != preview.origin
    {
        return Err("Le dépôt ou HEAD a changé. Prépare un nouvel aperçu du push.".into());
    }
    authentication
        .prepare_push(
            &repository,
            &preview.origin,
            &format!("refs/heads/{}", preview.pull_request.source_branch),
            &preview.commit,
            &preview.previous,
        )
        .await
}
pub fn sending(store: &Store<'_>, expected: &Card, id: &str) -> Result<()> {
    store.transaction(|store| {
        let (mut card, mut intent) = active(store, &expected.id, id)?;
        forge::require_revision(store, intent.preview.pull_request.revision)?;
        if intent.state != Phase::Preparing
            || serde_json::to_value(atelier_core::application::jira::card_snapshot(&card))?
                != serde_json::to_value(atelier_core::application::jira::card_snapshot(expected))?
        {
            return Err("L’intention de push a changé.".into());
        }
        intent.state = Phase::Sending;
        card.fields.set("gitPush", &intent)?;
        forge_association::invalidate(
            store,
            &mut card,
            "Un push a démarré. Relis la PR/MR et revalide ses reviews.",
        )?;
        store.put(Documents::Cards, &card)
    })
}
fn active(store: &Store<'_>, card_id: &str, id: &str) -> Result<(Card, GitPushIntent)> {
    let card = store
        .document::<Card>(Documents::Cards, card_id)?
        .ok_or("Carte de push absente.")?;
    let intent = card
        .fields
        .get::<GitPushIntent>("gitPush")?
        .filter(|intent| intent.id == id)
        .ok_or("L’intention de push a changé.")?;
    Ok((card, intent))
}
pub fn interrupted(store: &Store<'_>, card_id: &str, id: &str) -> Result<Card> {
    store.transaction(|store| {
        let (mut card, mut intent) = active(store, card_id, id)?;
        intent.state = match intent.state {
            Phase::Preparing => Phase::Failed,
            Phase::Sending => Phase::Uncertain,
            _ => return Err("L’intention de push a changé.".into()),
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
            forge::configuration(store, intent.preview.pull_request.kind, revision)?;
        if !matches!(intent.state, Phase::Sending | Phase::Uncertain)
            || configuration.base != intent.preview.pull_request.base_url
        {
            return Err("L’intention de push a changé.".into());
        }
        if ![40, 64].contains(&observed.len())
            || !observed.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err("Commit distant invalide.".into());
        }
        intent.observed = Some(observed.clone());
        intent.state = if reconciled && observed != intent.preview.previous {
            // A delayed request with the old lease cannot replace this different remote value.
            Phase::Reconciled
        } else if observed == intent.preview.commit {
            Phase::Confirmed
        } else {
            Phase::Uncertain
        };
        if matches!(intent.state, Phase::Confirmed | Phase::Reconciled) {
            card.fields.set("gitPushEffect", &intent)?;
            card.fields.remove("gitPushPreview");
        }
        card.fields.set("gitPush", &intent)?;
        store.put(Documents::Cards, &card)?;
        Ok(card)
    })
}
