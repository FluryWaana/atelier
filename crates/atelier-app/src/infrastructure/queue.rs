use super::store::{Documents, Store};
use crate::Result;
use atelier_core::{
    contracts::{Card, QueuedRun, RunQueue, Worktree, WorktreeState},
    document::Document,
};
fn succeeded(run: &Document, active: &QueuedRun) -> Result<bool> {
    Ok(run.string_equals("status", "completed")?
        && run.string_equals("cardId", &active.card_id)?
        && run.get::<i32>("exitCode")? == Some(0)
        && !run.truthy("attention")?)
}
pub fn read(store: &Store<'_>) -> Result<RunQueue> {
    let queue = store
        .settings::<Document>()?
        .get::<RunQueue>("runQueue")?
        .unwrap_or_default();
    if queue.ids.len() > 100
        || queue
            .ids
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            != queue.ids.len()
        || queue
            .active
            .as_ref()
            .is_some_and(|active| queue.ids.first() != Some(&active.card_id))
    {
        return Err("File d’attente invalide.".into());
    }
    Ok(queue)
}
fn save(store: &Store<'_>, mut queue: RunQueue) -> Result<RunQueue> {
    queue.revision = queue
        .revision
        .checked_add(1)
        .ok_or("Révision de file épuisée.")?;
    let mut settings = store.settings::<Document>()?;
    settings.set("runQueue", &queue)?;
    store.save_settings(&settings)?;
    Ok(queue)
}
pub fn pause(store: &Store<'_>, message: &str) -> Result<RunQueue> {
    store.transaction(|store| {
        let mut queue = read(store)?;
        queue.paused = true;
        queue.message = message.into();
        save(store, queue)
    })
}
pub fn change(store: &Store<'_>, body: &Document) -> Result<RunQueue> {
    store.transaction(|store| {
        let mut queue = read(store)?;
        if body.get::<u64>("revision")? != Some(queue.revision) {
            return Err(
                atelier_core::application::card_lifecycle::CardFailure::Conflict(
                    "La file a changé. Recharge avant de réessayer.",
                )
                .into(),
            );
        }
        if body.get::<bool>("confirmed")? != Some(true) {
            return Err("Confirme la modification de la file d’attente.".into());
        }
        match body.get::<String>("action")?.as_deref() {
            Some("add") => {
                let expected = body
                    .get::<Card>("expected")?
                    .ok_or("Instantané de tâche requis.")?;
                let card = super::forge_association::snapshot(store, &expected)?;
                if card
                    .fields
                    .get::<Worktree>("worktree")?
                    .is_none_or(|worktree| worktree.state != WorktreeState::Ready)
                {
                    return Err("Prépare le worktree avant d’ajouter la tâche à la file.".into());
                }
                if !queue.ids.contains(&card.id) {
                    if queue.ids.len() >= 100 {
                        return Err("File limitée à 100 tâches.".into());
                    }
                    queue.ids.push(card.id);
                }
            }
            Some("remove") => {
                let id = body.get::<String>("cardId")?.ok_or("Carte absente.")?;
                if queue
                    .active
                    .as_ref()
                    .is_some_and(|active| active.card_id == id)
                {
                    return Err(
                        "Attends la fin de la tâche active avant de la retirer de la file.".into(),
                    );
                }
                queue.ids.retain(|card| card != &id);
            }
            Some("resume") => {
                queue.paused = false;
                queue.message.clear();
            }
            _ => return Err("Action de file inconnue.".into()),
        }
        save(store, queue)
    })
}
pub fn recover(store: &Store<'_>) -> Result<()> {
    store.transaction(recover_in_transaction)
}
pub(super) fn recover_in_transaction(store: &Store<'_>) -> Result<()> {
    if store.settings::<Document>()?.raw("runQueue").is_none() {
        return Ok(());
    }
    let mut queue = read(store)?;
    if let Some(active) = queue.active.take() {
        if store
            .document::<Document>(Documents::Runs, &active.run_id)?
            .as_ref()
            .map(|run| succeeded(run, &active))
            .transpose()?
            .unwrap_or(false)
        {
            queue.ids.retain(|id| id != &active.card_id);
        }
    }
    queue.paused = true;
    queue.message =
        "File en pause après redémarrage. Vérifie les tâches avant de reprendre.".into();
    save(store, queue)?;
    Ok(())
}
pub fn claim(store: &Store<'_>, run_id: String) -> Result<Option<(Card, Document)>> {
    store.transaction(|store| {
        let mut queue = read(store)?;
        if queue.paused || queue.active.is_some() || queue.ids.is_empty() {
            return Ok(None);
        }
        let card = store
            .document::<Card>(Documents::Cards, &queue.ids[0])?
            .ok_or("Carte introuvable.")?;
        super::forge_association::snapshot(store, &card)?;
        let agent = card
            .fields
            .get::<String>("agent")?
            .filter(|agent| !agent.is_empty())
            .ok_or("Profil requis.")?;
        let mode = card
            .fields
            .get::<String>("mode")?
            .unwrap_or_else(|| "edit".into());
        if card.fields.truthy("brainstormSession")? || !["analyze", "edit"].contains(&mode.as_str())
        {
            return Err(
                "Seules les tâches enregistrées peuvent être exécutées dans la file.".into(),
            );
        }
        let mut body = Document::default();
        body.set("agent", &agent)?;
        body.set("mode", &mode)?;
        body.set("purpose", &"work")?;
        body.set(
            "expected",
            &atelier_core::application::jira::card_snapshot(&card),
        )?;
        queue.active = Some(QueuedRun {
            card_id: card.id.clone(),
            run_id,
        });
        save(store, queue)?;
        Ok(Some((card, body)))
    })
}
pub fn require_claim(store: &Store<'_>, run: &Document) -> Result<()> {
    let queue = read(store)?;
    if queue.paused
        || queue.active.is_none_or(|active| {
            run.get::<String>("id").ok().flatten() != Some(active.run_id)
                || run.get::<String>("cardId").ok().flatten() != Some(active.card_id)
        })
    {
        return Err("La file est en pause ou son lancement a changé.".into());
    }
    Ok(())
}
pub fn settle(store: &Store<'_>) -> Result<bool> {
    store.transaction(|store|{
    let mut queue=read(store)?;let Some(active)=queue.active.take()else{return Ok(false);};
    let run=store.document::<Document>(Documents::Runs,&active.run_id)?;
    if run.as_ref().is_some_and(|run|super::runs::active(run).unwrap_or(true)){return Ok(false);}
    if run.as_ref().map(|run|succeeded(run,&active)).transpose()?.unwrap_or(false){queue.ids.retain(|id|id!=&active.card_id);}
    else{queue.paused=true;queue.message="Exécution arrêtée, incertaine ou en échec. Vérifie la tâche avant de reprendre la file.".into();}
    save(store,queue)?;Ok(true)
})
}
