use super::store::{Documents, Store};
use crate::Result;
use atelier_core::{
    application::automation::State as WorkflowState,
    contracts::{Card, QueuedRun, RunQueue, Worktree, WorktreeState},
    document::Document,
};
fn remove(queue: &mut RunQueue, id: &str) {
    queue.ids.retain(|card| card != id);
    queue.workflows.remove(id);
}
fn workflow_state(store: &Store<'_>, active: &QueuedRun) -> Result<Option<WorkflowState>> {
    let Some(card) = store.document::<Card>(Documents::Cards, &active.card_id)? else {
        return Ok(None);
    };
    Ok(super::automation::read(&card)?
        .filter(|flow| flow.id == active.run_id)
        .map(|flow| flow.state))
}
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
        || queue.workflows.iter().any(|(id, operation)| {
            !queue.ids.contains(id)
                || operation
                    .as_ref()
                    .is_some_and(|operation| uuid::Uuid::parse_str(operation).is_err())
        })
        || queue.active.as_ref().is_some_and(|active| {
            queue
                .workflows
                .get(&active.card_id)
                .is_some_and(|operation| operation.as_deref() != Some(active.run_id.as_str()))
        })
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
pub fn needs_tick(store: &Store<'_>) -> Result<bool> {
    let queue = read(store)?;
    let settling = match &queue.active {
        Some(active) if queue.workflows.contains_key(&active.card_id) => !matches!(
            workflow_state(store, active)?,
            Some(WorkflowState::Running | WorkflowState::Waiting)
        ),
        Some(_) => true,
        None => !queue.paused && !queue.ids.is_empty(),
    };
    Ok(settling || super::automation::running(store)?.is_some())
}
pub fn pause(store: &Store<'_>, message: &str) -> Result<RunQueue> {
    store.transaction(|store| {
        let mut queue = read(store)?;
        if queue.paused && queue.message == message {
            return Ok(queue);
        }
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
                let workflow=body.get::<bool>("workflow")?.unwrap_or(false);
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
                    if workflow {
                        let project=super::verification::project(store,&card)?;
                        if project.get::<bool>("automationEnabled")?!=Some(true) || project.get::<String>("testCommand")?.is_none_or(|command|command.trim().is_empty()) || project.get::<String>("prTarget")?.is_none_or(|target|target.trim().is_empty()) {return Err("Configure et active le parcours du projet avant de l’ajouter à la file.".into());}
                        let existing=super::automation::read(&card)?;
                        if existing.as_ref().is_some_and(|flow|flow.state==WorkflowState::Blocked) {return Err("Vérifie le parcours bloqué avant de le remettre dans la file.".into());}
                        queue.workflows.insert(card.id.clone(),existing.filter(|flow|flow.state!=WorkflowState::Ready).map(|flow|flow.id));
                    }
                    queue.ids.push(card.id);
                } else if queue.workflows.contains_key(&card.id)!=workflow {
                    return Err("Retire la tâche de la file avant de changer son mode d’exécution.".into());
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
                remove(&mut queue,&id);
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
        let completed = if queue.workflows.contains_key(&active.card_id) {
            workflow_state(store, &active)? == Some(WorkflowState::Ready)
        } else {
            store
                .document::<Document>(Documents::Runs, &active.run_id)?
                .as_ref()
                .map(|run| succeeded(run, &active))
                .transpose()?
                .unwrap_or(false)
        };
        if completed {
            remove(&mut queue, &active.card_id);
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
        if atelier_core::application::agent_cli::busy(&store.settings::<Document>()?)?{return Ok(None);}
        let mut queue = read(store)?;
        if queue.paused || queue.active.is_some() || queue.ids.is_empty() {
            return Ok(None);
        }
        if super::connector_effects::read(&store.settings::<Document>()?)?
            .iter().any(super::connector_effects::pending) {
            queue.paused=true;
            queue.message="File en pause : termine ou rapproche l’écriture du connecteur avant de reprendre.".into();
            save(store,queue)?;
            return Ok(None);
        }
        let card = store
            .document::<Card>(Documents::Cards, &queue.ids[0])?
            .ok_or("Carte introuvable.")?;
        super::forge_association::snapshot(store, &card)?;
        if let Some(bound)=queue.workflows.get(&card.id).cloned() {
            let existing=super::automation::read(&card)?;
            let action=if let Some(operation)=bound.as_ref() {
                let flow=existing.as_ref().filter(|flow|&flow.id==operation).ok_or("Le parcours lié à la file a été remplacé. Retire puis ajoute à nouveau la tâche.")?;
                match flow.state {
                    WorkflowState::Running|WorkflowState::Waiting|WorkflowState::Ready=>None,
                    WorkflowState::Interrupted|WorkflowState::Error|WorkflowState::Cancelled=>Some("resume"),
                    WorkflowState::Blocked=>return Err("Vérifie le parcours bloqué avant de reprendre la file.".into()),
                }
            } else {Some("start")};
            let card=if let Some(action)=action {
                let agent=card.fields.get::<String>("agent")?.ok_or("Profil requis.")?;
                let mut body=Document::default();body.set("action",&action)?;body.set("expected",&card)?;body.set("confirmed",&true)?;
                body.set("project",&super::verification::project(store,&card)?)?;body.set("profile",&super::automation::profile(store,&agent)?)?;
                super::automation::change_in_transaction(store,&card.id,&body)?
            } else {card};
            let flow=super::automation::read(&card)?.ok_or("Parcours de file absent.")?;
            queue.workflows.insert(card.id.clone(),Some(flow.id.clone()));queue.active=Some(QueuedRun{card_id:card.id.clone(),run_id:flow.id});
            save(store,queue)?;let mut body=Document::default();body.set("workflow",&true)?;return Ok(Some((card,body)));
        }
        if super::automation::read(&card)?
            .is_some_and(|flow| flow.state == atelier_core::application::automation::State::Running)
        {
            return Err(
                "Le parcours réserve cette tâche. Arrête-le avant un lancement manuel.".into(),
            );
        }
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
            queue.workflows.contains_key(&active.card_id)
                || run.get::<String>("id").ok().flatten() != Some(active.run_id)
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
    if queue.workflows.contains_key(&active.card_id) {
        match workflow_state(store,&active)? {
            Some(WorkflowState::Running|WorkflowState::Waiting)=>return Ok(false),
            Some(WorkflowState::Ready)=>remove(&mut queue,&active.card_id),
            _=>{queue.paused=true;queue.message="Parcours arrêté, bloqué ou interrompu. Vérifie la tâche avant de reprendre la file.".into();},
        }
        save(store,queue)?;return Ok(true);
    }
    let run=store.document::<Document>(Documents::Runs,&active.run_id)?;
    if run.as_ref().is_some_and(|run|super::runs::active(run).unwrap_or(true)){return Ok(false);}
    if run.as_ref().map(|run|succeeded(run,&active)).transpose()?.unwrap_or(false){remove(&mut queue,&active.card_id);}
    else{queue.paused=true;queue.message="Exécution arrêtée, incertaine ou en échec. Vérifie la tâche avant de reprendre la file.".into();}
    save(store,queue)?;Ok(true)
})
}
