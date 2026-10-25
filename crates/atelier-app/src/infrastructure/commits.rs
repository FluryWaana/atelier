use super::{
    forge_association,
    store::{Documents, Store},
};
use crate::Result;
use atelier_core::contracts::{
    Card, ForgeWriteState as Phase, LocalCommitIntent, LocalCommitPreview, LocalCommitRequest,
    Worktree, WorktreeState,
};

pub fn worktree(card: &Card) -> Result<Worktree> {
    let worktree = card
        .fields
        .get::<Worktree>("worktree")?
        .filter(|worktree| worktree.state == WorktreeState::Ready)
        .ok_or("Prépare le worktree de la tâche avant de créer un commit.")?;
    if card.fields.get::<String>("repo")?.as_deref() != Some(worktree.source.as_str()) {
        return Err("Le dépôt de la tâche et le worktree ne correspondent plus.".into());
    }
    Ok(worktree)
}

pub fn save_preview(
    store: &Store<'_>,
    expected: &Card,
    preview: LocalCommitPreview,
) -> Result<Card> {
    store.transaction(|store| {
        let mut card = forge_association::snapshot(store, expected)?;
        if worktree(&card)? != preview.worktree
            || preview.worktree.branch != preview.snapshot.branch
        {
            return Err("Le worktree a changé pendant l’aperçu du commit.".into());
        }
        card.fields.set("localCommitPreview", &preview)?;
        store.put(Documents::Cards, &card)?;
        Ok(card)
    })
}

pub fn reserve(store: &Store<'_>, input: &LocalCommitRequest, id: &str) -> Result<Card> {
    store.transaction(|store| {
        if !input.confirmed || uuid::Uuid::parse_str(id).is_err() {
            return Err("Confirme les fichiers et le message du commit.".into());
        }
        let mut card = forge_association::snapshot(store, &input.expected)?;
        let preview = card
            .fields
            .get::<LocalCommitPreview>("localCommitPreview")?
            .ok_or("Prépare un aperçu avant de créer le commit.")?;
        if worktree(&card)? != preview.worktree
            || input.paths != preview.snapshot.paths
            || input.author_name != preview.author_name
            || input.author_email != preview.author_email
            || input.message != preview.message
        {
            return Err("Le brouillon du commit a changé. Prépare un nouvel aperçu.".into());
        }
        card.fields.set(
            "localCommit",
            &LocalCommitIntent {
                id: id.into(),
                state: Phase::Preparing,
                preview,
                commit: None,
                observed: None,
            },
        )?;
        store.put(Documents::Cards, &card)?;
        Ok(card)
    })
}

fn active(store: &Store<'_>, card_id: &str, id: &str) -> Result<(Card, LocalCommitIntent)> {
    let card = store
        .document::<Card>(Documents::Cards, card_id)?
        .ok_or("Carte introuvable.")?;
    let intent = card
        .fields
        .get::<LocalCommitIntent>("localCommit")?
        .filter(|intent| intent.id == id)
        .ok_or("L’intention de commit a changé.")?;
    Ok((card, intent))
}

pub fn sending(store: &Store<'_>, expected: &Card, id: &str, commit: String) -> Result<()> {
    store.transaction(|store| {
        let (mut card, mut intent) = active(store, &expected.id, id)?;
        if intent.state != Phase::Preparing || serde_json::to_value(&card)? != serde_json::to_value(expected)? {
            return Err("L’intention de commit a changé.".into());
        }
        if !matches!(commit.len(), 40 | 64) || !commit.bytes().all(|c| c.is_ascii_hexdigit()) {
            return Err("Identité du commit invalide.".into());
        }
        intent.state = Phase::Sending;
        intent.commit = Some(commit);
        card.fields.set("localCommit", &intent)?;
        forge_association::invalidate(store, &mut card, "Un commit local a démarré. Revalide les reviews et vérifications sur les sources actuelles.")?;
        // Every subsequent push/publication requires a fresh explicit preview.
        card.fields.remove("gitPushPreview");
        card.fields.remove("forgePublicationPreview");
        store.put(Documents::Cards, &card)
    })
}

pub fn interrupted(store: &Store<'_>, card_id: &str, id: &str) -> Result<Card> {
    store.transaction(|store| {
        let (mut card, mut intent) = active(store, card_id, id)?;
        intent.state = match intent.state {
            Phase::Preparing => Phase::Failed,
            Phase::Sending => Phase::Uncertain,
            _ => return Ok(card),
        };
        card.fields.set("localCommit", &intent)?;
        store.put(Documents::Cards, &card)?;
        Ok(card)
    })
}

pub fn record(store: &Store<'_>, card_id: &str, id: &str, observed: String) -> Result<Card> {
    store.transaction(|store| {
        let (mut card, mut intent) = active(store, card_id, id)?;
        if !matches!(intent.state, Phase::Sending | Phase::Uncertain)
            || intent.commit.is_none()
            || !matches!(observed.len(), 40 | 64)
            || !observed.bytes().all(|c| c.is_ascii_hexdigit())
        {
            return Err("L’intention de commit ou son observation a changé.".into());
        }
        intent.state = if intent.commit.as_deref() == Some(observed.as_str()) {
            Phase::Confirmed
        } else if observed != intent.preview.snapshot.head {
            Phase::Reconciled
        } else {
            Phase::Uncertain
        };
        intent.observed = Some(observed);
        if matches!(intent.state, Phase::Confirmed | Phase::Reconciled) {
            card.fields.set("localCommitEffect", &intent)?;
            card.fields.remove("localCommitPreview");
        }
        card.fields.set("localCommit", &intent)?;
        store.put(Documents::Cards, &card)?;
        Ok(card)
    })
}
