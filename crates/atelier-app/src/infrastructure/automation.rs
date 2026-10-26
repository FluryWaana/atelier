use super::{
    runs,
    store::{Documents, Store},
    verification,
};
use crate::Result;
use atelier_core::{
    application::{
        automation::{Flow, PublicationPlan, State, Step},
        preparation,
        reviews::{Coverage, Priority, Review},
    },
    contracts::{Card, Worktree, WorktreeState},
    document::Document,
};

fn now() -> Result<String> {
    Ok(time::OffsetDateTime::now_utc().format(&time::format_description::well_known::Rfc3339)?)
}
fn profile(store: &Store<'_>, id: &str) -> Result<Document> {
    atelier_core::domain::agent_options::providers(&atelier_core::application::settings::config(
        &store.settings::<Document>()?,
    )?)?
    .into_iter()
    .find(|profile| profile.string_equals("id", id).unwrap_or(false))
    .ok_or_else(|| "Profil du parcours introuvable.".into())
}
pub fn read(card: &Card) -> Result<Option<Flow>> {
    let Some(value) = card.fields.get::<Document>("automation")? else {
        return Ok(None);
    };
    if !value.truthy("id")? {
        if value.string_equals("state", "running")? {
            return Err(
                "Parcours actif sans identifiant : examine les processus avant de poursuivre."
                    .into(),
            );
        }
        return Ok(None);
    }
    Ok(card.fields.get("automation")?)
}
fn save(store: &Store<'_>, card: &mut Card, flow: &mut Flow) -> Result<()> {
    flow.updated_at = now()?;
    card.fields.set("automation", flow)?;
    store.put(Documents::Cards, card)
}
fn current(store: &Store<'_>, card: &Card, flow: &Flow) -> Result<()> {
    if flow.requirements != preparation::requirements(card)?
        || card.fields.get::<Worktree>("worktree")?.as_ref() != Some(&flow.worktree)
        || card
            .fields
            .get::<String>("instructions")?
            .unwrap_or_default()
            != flow.instructions
        || card.fields.get::<String>("agent")?.unwrap_or_default() != flow.agent
        || card.fields.get::<String>("model")?.unwrap_or_default() != flow.model
        || card.fields.get::<String>("effort")?.unwrap_or_default() != flow.effort
        || serde_json::to_value(profile(store, &flow.agent)?)?
            != serde_json::to_value(&flow.profile)?
        || serde_json::to_value(verification::project(store, card)?)?
            != serde_json::to_value(&flow.project)?
    {
        return Err("Le besoin, le profil, le worktree ou le projet du parcours a changé. Lance un nouveau parcours après vérification.".into());
    }
    Ok(())
}

pub fn change(store: &Store<'_>, id: &str, body: &Document) -> Result<Card> {
    store.transaction(|store| {
        if body.get::<bool>("confirmed")? != Some(true) { return Err("Confirme l’étape du parcours automatique.".into()); }
        let expected = body.get::<Card>("expected")?.filter(|card|card.id==id).ok_or("Instantané de tâche requis.")?;
        let mut card = super::forge_association::snapshot(store,&expected)?;
        let mut flow = read(&card)?;
        match body.get::<String>("action")?.as_deref() {
            Some("start") => {
                if flow.as_ref().is_some_and(|flow| matches!(flow.state,State::Running|State::Waiting)) { return Err("Arrête le parcours précédent avant d’en démarrer un autre.".into()); }
                let project = verification::project(store,&card)?;
                if body.get::<Document>("project")?.as_ref().map(serde_json::to_value).transpose()? != Some(serde_json::to_value(&project)?) {return Err("Les réglages du parcours ont changé. Recharge le projet avant de confirmer.".into());}
                if project.get::<bool>("automationEnabled")? != Some(true) || project.get::<String>("testCommand")?.is_none_or(|value|value.trim().is_empty()) || project.get::<String>("prTarget")?.is_none_or(|value|value.trim().is_empty()) { return Err("Active le parcours et renseigne la commande de vérification et la cible dans le projet.".into()); }
                let worktree = card.fields.get::<Worktree>("worktree")?.filter(|worktree|worktree.state==WorktreeState::Ready).ok_or("Prépare le worktree avant le parcours automatique.")?;
                let agent = card.fields.get::<String>("agent")?.filter(|value|!value.is_empty()).ok_or("Profil requis.")?;
                let auto_fix = project.get::<bool>("autoFix")?.unwrap_or(true);
                let profile = profile(store,&agent)?;
                if body.get::<Document>("profile")?.as_ref().map(serde_json::to_value).transpose()?!=Some(serde_json::to_value(&profile)?) {return Err("Le profil du parcours a changé. Relis ses réglages avant de confirmer.".into());}
                let stamp = now()?;
                flow = Some(Flow {id:uuid::Uuid::new_v4().to_string(),state:State::Running,step:Step::Development,project,requirements:preparation::requirements(&card)?,worktree,agent,profile,model:card.fields.get::<String>("model")?.unwrap_or_default(),effort:card.fields.get::<String>("effort")?.unwrap_or_default(),instructions:card.fields.get::<String>("instructions")?.unwrap_or_default(),run_id:None,review_id:None,verification_id:None,publication_plan:None,commit_plan:None,review_requested:false,correction_indexes:Vec::new(),cycles:0,auto_fix,started_at:stamp.clone(),updated_at:stamp,message:String::new()});
            },
            Some("resume") => {
                let active = flow.as_mut().ok_or("Aucun parcours à reprendre.")?;
                if !matches!(active.state,State::Interrupted|State::Error|State::Cancelled) { return Err("Ce parcours ne peut pas être repris à cette étape.".into()); }
                current(store,&card,active)?;
                // Repeating an interrupted local step requires this new explicit confirmation.
                active.run_id = None; active.state = State::Running; active.message.clear();
                active.publication_plan=None;
                active.commit_plan=None;
                if matches!(active.step,Step::Commit|Step::Publication|Step::Review) { active.step = Step::Tests; }
            },
            Some("review") => {
                let active = flow.as_mut().filter(|flow|flow.state==State::Waiting && flow.step==Step::Publication).ok_or("Le parcours n’attend pas de publication.")?;
                current(store,&card,active)?;
                let linked = card.fields.get::<atelier_core::contracts::PullRequest>("pullRequest")?.ok_or("Publie ou lie une PR/MR avant de poursuivre la review.")?;
                if active.project.get::<String>("prTarget")?.as_deref()!=Some(linked.target_branch.as_str()) || linked.source_branch != active.worktree.branch { return Err("Les branches de la PR/MR ne correspondent pas au parcours.".into()); }
                active.review_requested=true;active.step=Step::Tests;active.state=State::Running;active.run_id=None;active.message.clear();
            },
            Some("commit") => {
                let active=flow.as_mut().filter(|flow|flow.state==State::Waiting && flow.step==Step::Publication).ok_or("Le parcours n’attend pas de publication.")?;
                current(store,&card,active)?;
                let preview=card.fields.get::<atelier_core::contracts::LocalCommitPreview>("localCommitPreview")?.ok_or("Prépare l’aperçu du commit avant d’autoriser son enchaînement.")?;
                if preview.worktree!=active.worktree || preview.snapshot.branch!=active.worktree.branch {return Err("Le commit ne correspond pas au worktree du parcours.".into());}
                active.commit_plan=Some(Box::new(preview));active.publication_plan=None;active.review_requested=false;active.step=Step::Commit;active.state=State::Running;active.run_id=None;active.message.clear();
            },
            Some("publish") => {
                let active=flow.as_mut().filter(|flow|flow.state==State::Waiting && flow.step==Step::Publication).ok_or("Le parcours n’attend pas de publication.")?;
                current(store,&card,active)?;
                let target=active.project.get::<String>("prTarget")?.ok_or("Branche cible absente.")?;
                let plan=if card.fields.raw("pullRequest").is_some() {
                    let preview=card.fields.get::<atelier_core::contracts::GitPushPreview>("gitPushPreview")?.ok_or("Prépare l’aperçu du push avant d’autoriser son enchaînement.")?;
                    let linked=card.fields.get::<atelier_core::contracts::PullRequest>("pullRequest")?.ok_or("PR/MR absente.")?;
                    if preview.worktree!=active.worktree || preview.pull_request.as_ref().is_none_or(|remote|!super::forge::same_pull_request(remote,&linked)) || linked.target_branch!=target {return Err("L’aperçu ne correspond pas aux branches du parcours.".into());}
                    super::forge::configuration(store,preview.configuration.kind,preview.revision)?;
                    PublicationPlan::Push(Box::new(preview))
                } else {
                    let preview=card.fields.get::<atelier_core::contracts::ForgePublicationPreview>("forgePublicationPreview")?.ok_or("Prépare l’aperçu de publication avant d’autoriser son enchaînement.")?;
                    if preview.worktree!=active.worktree || preview.target!=target || !preview.draft {return Err("Le parcours exige les branches prévues et une publication en brouillon.".into());}
                    if preview.remote_source.is_none() && body.get::<bool>("allowCreateBranch")?!=Some(true) {return Err("Confirme séparément la création de la branche distante absente.".into());}
                    super::forge::configuration(store,preview.configuration.kind,preview.revision)?;
                    PublicationPlan::Create(Box::new(preview))
                };
                active.publication_plan=Some(plan);active.review_requested=false;active.step=Step::Tests;active.state=State::Running;active.run_id=None;active.message.clear();
            },
            _ => return Err("Action de parcours inconnue.".into()),
        }
        save(store,&mut card,flow.as_mut().ok_or("Parcours absent.")?)?;
        Ok(card)
    })
}

pub fn cancel(store: &Store<'_>, id: &str, operation: &str) -> Result<Option<String>> {
    store.transaction(|store| {
        let mut card = store
            .document::<Card>(Documents::Cards, id)?
            .ok_or("Carte introuvable.")?;
        let mut flow = read(&card)?
            .filter(|flow| {
                flow.id == operation && matches!(flow.state, State::Running | State::Waiting)
            })
            .ok_or("Le parcours a changé ou est déjà arrêté.")?;
        flow.state = State::Cancelled;
        flow.message =
            "Parcours arrêté. Vérifie les fichiers et les effets distants avant de reprendre."
                .into();
        let run = flow.run_id.clone();
        save(store, &mut card, &mut flow)?;
        Ok(run)
    })
}

pub fn recover_in_transaction(store: &Store<'_>) -> Result<()> {
    for mut card in store.documents::<Card>(Documents::Cards)? {
        if let Some(mut flow) = read(&card)?
            && flow.state == State::Running
        {
            flow.state = State::Interrupted;
            flow.message="Parcours interrompu après redémarrage. Aucun lancement ni publication n’a été rejoué.".into();
            save(store, &mut card, &mut flow)?;
        }
    }
    Ok(())
}
pub fn recover(store: &Store<'_>) -> Result<()> {
    store.transaction(recover_in_transaction)
}
pub fn require_run(
    store: &Store<'_>,
    card: &Card,
    run_id: &str,
    operation: Option<&str>,
) -> Result<()> {
    let flow = read(card)?;
    if let Some(operation) = operation {
        let flow = flow
            .filter(|flow| {
                flow.id == operation
                    && flow.state == State::Running
                    && flow.run_id.as_deref() == Some(run_id)
            })
            .ok_or("Le lancement du parcours a été arrêté ou remplacé.")?;
        current(store, card, &flow)?;
    } else if flow.is_some_and(|flow| flow.state == State::Running) {
        return Err("Le parcours réserve cette tâche. Arrête-le avant un lancement manuel.".into());
    }
    Ok(())
}
pub fn blocks(store: &Store<'_>, path: &str) -> Result<bool> {
    let affected = path
        .strip_prefix("/api/cards/")
        .map(|tail| tail.split('/').next().unwrap_or_default());
    for card in store.documents::<Card>(Documents::Cards)? {
        if read(&card)?.is_some_and(|flow| flow.state == State::Running)
            && (path.starts_with("/api/backups")
                || path.starts_with("/api/updates")
                || path.starts_with("/api/projects")
                || path.starts_with("/api/settings")
                || path.starts_with("/api/agent-providers")
                || affected == Some(card.id.as_str()))
        {
            return Ok(true);
        }
    }
    Ok(false)
}
pub fn running(store: &Store<'_>) -> Result<Option<(Card, Flow)>> {
    for card in store.documents::<Card>(Documents::Cards)? {
        if let Some(flow) = read(&card)?
            && flow.state == State::Running
        {
            return Ok(Some((card, flow)));
        }
    }
    Ok(None)
}
pub fn fail(store: &Store<'_>, id: &str, operation: &str, message: &str) -> Result<()> {
    store.transaction(|store| {
        let mut card = store
            .document::<Card>(Documents::Cards, id)?
            .ok_or("Carte introuvable.")?;
        let Some(mut flow) =
            read(&card)?.filter(|flow| flow.id == operation && flow.state == State::Running)
        else {
            return Ok(());
        };
        flow.state = State::Error;
        flow.message = message.chars().take(1000).collect();
        save(store, &mut card, &mut flow)
    })
}

pub fn commit(
    store: &Store<'_>,
    id: &str,
    operation: &str,
) -> Result<(Card, atelier_core::contracts::LocalCommitPreview)> {
    let card = store
        .document::<Card>(Documents::Cards, id)?
        .ok_or("Carte introuvable.")?;
    let card = super::forge_association::snapshot(store, &card)?;
    let flow = read(&card)?
        .filter(|flow| {
            flow.id == operation
                && flow.state == State::Running
                && flow.step == Step::Commit
                && flow.run_id.is_none()
        })
        .ok_or("Le commit du parcours a été arrêté ou remplacé.")?;
    current(store, &card, &flow)?;
    Ok((
        card,
        *flow
            .commit_plan
            .ok_or("Aperçu du commit du parcours absent.")?,
    ))
}
pub fn commit_complete(store: &Store<'_>, expected: &Card, operation: &str) -> Result<()> {
    store.transaction(|store| {
        super::forge_association::snapshot(store,expected)?;
        let (mut card,approved)=commit(store,&expected.id,operation)?;
        let effect=card.fields.get::<atelier_core::contracts::LocalCommitIntent>("localCommit")?.ok_or("Intention de commit absente.")?;
        if effect.state!=atelier_core::contracts::ForgeWriteState::Confirmed || effect.commit.is_none() || effect.observed!=effect.commit || serde_json::to_value(effect.preview)?!=serde_json::to_value(approved)? {
            return Err("Le commit du parcours n’est pas confirmé sur son aperçu. Relis la branche avant de poursuivre.".into());
        }
        let mut flow=read(&card)?.ok_or("Parcours absent.")?;
        flow.commit_plan=None;flow.publication_plan=None;flow.verification_id=None;flow.step=Step::Tests;flow.run_id=None;flow.review_requested=false;flow.message="Commit observé. Les sources vont être revérifiées avant toute publication.".into();
        save(store,&mut card,&mut flow)
    })
}

pub fn publication(
    store: &Store<'_>,
    id: &str,
    operation: &str,
) -> Result<(Card, Flow, PublicationPlan)> {
    let card = store
        .document::<Card>(Documents::Cards, id)?
        .ok_or("Carte introuvable.")?;
    let card = super::forge_association::snapshot(store, &card)?;
    let flow = read(&card)?
        .filter(|flow| {
            flow.id == operation
                && flow.state == State::Running
                && flow.step == Step::Publication
                && flow.run_id.is_none()
        })
        .ok_or("La publication du parcours a été arrêtée ou remplacée.")?;
    current(store, &card, &flow)?;
    let plan = flow
        .publication_plan
        .clone()
        .ok_or("Aperçu de publication du parcours absent.")?;
    Ok((card, flow, plan))
}

pub fn publication_complete(store: &Store<'_>, expected: &Card, operation: &str) -> Result<()> {
    store.transaction(|store| {
        super::forge_association::snapshot(store,expected)?;
        let (mut card,mut flow,plan)=publication(store,&expected.id,operation)?;
        let linked=card.fields.get::<atelier_core::contracts::PullRequest>("pullRequest")?.ok_or("PR/MR publiée absente.")?;
        let (kind,revision,repository,commit,target_commit)=match &plan {
            PublicationPlan::Create(preview)=>(preview.configuration.kind,preview.revision,&preview.repository,&preview.source_commit,Some(preview.target_commit.as_str())),
            PublicationPlan::Push(preview)=>(preview.configuration.kind,preview.revision,&preview.repository,&preview.commit,preview.pull_request.as_ref().and_then(|remote|remote.target_commit.as_deref())),
        };
        if linked.kind!=kind || linked.revision!=revision || linked.repository!=*repository || linked.source_commit.as_ref()!=Some(commit) || linked.target_commit.as_deref()!=target_commit || linked.source_branch!=flow.worktree.branch || flow.project.get::<String>("prTarget")?.as_deref()!=Some(linked.target_branch.as_str()) || linked.status!="active" || linked.fork {
            return Err("La PR/MR observée ne correspond pas à la publication approuvée. Relis ses commits avant de poursuivre.".into());
        }
        flow.publication_plan=None;flow.review_requested=true;flow.step=Step::Review;flow.run_id=None;flow.message="Publication observée. La review va examiner les commits vérifiés.".into();
        save(store,&mut card,&mut flow)
    })
}

pub fn claim(
    store: &Store<'_>,
    id: &str,
    operation: &str,
    run_id: String,
) -> Result<(Card, Flow, Document)> {
    store.transaction(|store| {
        let card = store
            .document::<Card>(Documents::Cards, id)?
            .ok_or("Carte introuvable.")?;
        let mut card = super::forge_association::snapshot(store, &card)?;
        let mut flow = read(&card)?
            .filter(|flow| {
                flow.id == operation && flow.state == State::Running && flow.run_id.is_none()
            })
            .ok_or("L’étape du parcours a changé.")?;
        current(store, &card, &flow)?;
        flow.run_id = Some(run_id);
        save(store, &mut card, &mut flow)?;
        let mut body = Document::default();
        body.set("automationId", &flow.id)?;
        body.set("expected", &card)?;
        body.set("agent", &flow.agent)?;
        body.set("model", &flow.model)?;
        body.set("effort", &flow.effort)?;
        body.set("confirmed", &true)?;
        match flow.step {
            Step::Development => {
                body.set("purpose", &"work")?;
                body.set("mode", &"edit")?;
            }
            Step::Tests => {
                body.set(
                    "command",
                    &flow
                        .project
                        .get::<String>("testCommand")?
                        .ok_or("Commande de vérification absente.")?,
                )?;
            }
            Step::Review => {
                body.set("purpose", &"review")?;
                body.set("mode", &"analyze")?;
            }
            Step::Correction => {
                body.set("purpose", &"correction")?;
                body.set("mode", &"edit")?;
                body.set("reviewId", &flow.review_id)?;
                body.set("indexes", &flow.correction_indexes)?;
            }
            Step::Publication => return Err("La publication exige un contrôle explicite.".into()),
            Step::Commit => return Err("Le commit exige un aperçu confirmé.".into()),
        }
        Ok((card, flow, body))
    })
}

pub fn settle(store: &Store<'_>, id: &str, operation: &str) -> Result<()> {
    store.transaction(|store| {
        let mut card=store.document::<Card>(Documents::Cards,id)?.ok_or("Carte introuvable.")?;
        let mut flow=read(&card)?.filter(|flow|flow.id==operation && flow.state==State::Running).ok_or("L’étape du parcours a changé.")?;
        current(store,&card,&flow)?;
        let run_id=flow.run_id.as_ref().ok_or("Exécution du parcours absente.")?;
        let run=store.document::<Document>(Documents::Runs,run_id)?.ok_or("Lancement interrompu. Vérifie le worktree avant de reprendre.")?;
        if runs::active(&run)? {return Ok(());}
        if !run.string_equals("automationId",operation)? || !run.string_equals("cardId",id)? || !run.string_equals("status","completed")? || run.get::<i32>("exitCode")?!=Some(0) || run.truthy("attention")? {return Err("L’étape du parcours n’a pas abouti. Consulte le journal avant de reprendre.".into());}
        match flow.step {
            Step::Development|Step::Correction => {flow.step=Step::Tests;flow.review_requested=false;flow.publication_plan=None;flow.commit_plan=None;},
            Step::Tests => {
                if !run.string_equals("purpose","verification")? || !run.string_equals("verificationState","passed")? {return Err("La vérification du parcours a échoué ou ses sources ont changé.".into());}
                flow.verification_id=Some(run_id.clone());
                if flow.publication_plan.is_some() {flow.step=Step::Publication;}
                else if flow.review_requested { flow.step=Step::Review; }
                else {flow.step=Step::Publication;flow.state=State::Waiting;flow.message="Vérification réussie. Contrôle le commit, le push et la PR/MR avant de poursuivre la review.".into();}
            },
            Step::Review => {
                if run.truthy("reviewStaleReason")? || !run.truthy("reviewValidatedAt")? {return Err("La review du parcours doit être revalidée sur les commits actuels.".into());}
                let review=run.get::<Review>("review")?.ok_or("Review incomplète.")?;
                if review.criteria.len()!=flow.requirements["criteria"].as_array().map_or(0,Vec::len) || review.criteria.iter().enumerate().any(|(index,criterion)|criterion.index!=index) {return Err("Le bilan du parcours ne couvre pas exactement les critères enregistrés.".into());}
                flow.review_id=Some(run_id.clone());
                let indexes=review.findings.iter().enumerate().filter(|(_,finding)|matches!(finding.priority,Priority::P0|Priority::P1)).map(|(index,_)|index).collect::<Vec<_>>();
                if !indexes.is_empty() && flow.auto_fix && flow.cycles<2 {flow.cycles+=1;flow.correction_indexes=indexes;flow.step=Step::Correction;}
                else {
                    let blocked=!indexes.is_empty() || review.criteria.is_empty() || review.criteria.iter().any(|criterion|criterion.status!=Coverage::Covered);
                    flow.state=if blocked {State::Blocked}else{State::Ready};flow.message=if blocked {"Contrôle humain nécessaire : constats bloquants ou critères incomplets."}else{"PR/MR analysée. Le contrôle humain des critères reste nécessaire."}.into();
                }
            },
            Step::Publication => return Err("La publication exige un contrôle explicite.".into()),
            Step::Commit => return Err("Le commit exige un aperçu confirmé.".into()),
        }
        flow.run_id=None;save(store,&mut card,&mut flow)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn doc(value: serde_json::Value) -> Document {
        serde_json::from_str(&value.to_string()).unwrap()
    }
    fn seed(store: &Store<'_>) -> (Card, Document) {
        let project = doc(
            json!({"id":"project","name":"Synthetic","repo":"synthetic/source","automationEnabled":true,"autoFix":true,"testCommand":"synthetic-verification","prTarget":"main"}),
        );
        store.put(Documents::Projects, &project).unwrap();
        let card:Card=serde_json::from_str(&json!({"id":"task","title":"Synthetic workflow","description":"No process or remote call in this storage fixture","agent":"codex","mode":"edit","repo":"synthetic/source","checklist":[{"text":"Synthetic criterion","done":false}],"worktree":{"source":"synthetic/source","path":"synthetic/work","branch":"task","base":"main","baseCommit":"a".repeat(40),"state":"ready"}}).to_string()).unwrap();
        store.put(Documents::Cards, &card).unwrap();
        (card, project)
    }
    fn body(card: &Card, project: &Document, action: &str) -> Document {
        let profile = atelier_core::domain::agent_options::providers(&Document::default())
            .unwrap()
            .remove(0);
        doc(
            json!({"expected":card,"project":project,"profile":profile,"action":action,"confirmed":true}),
        )
    }
    fn card(store: &Store<'_>) -> Card {
        store.document(Documents::Cards, "task").unwrap().unwrap()
    }
    fn synthetic_stage_result(
        store: &Store<'_>,
        flow: &Flow,
        suffix: &str,
        fields: serde_json::Value,
    ) {
        let id = format!("synthetic-{suffix}");
        claim(store, "task", &flow.id, id.clone()).unwrap();
        let mut run = doc(
            json!({"id":id,"cardId":"task","automationId":flow.id,"status":"completed","exitCode":0}),
        );
        run.overlay(&doc(fields));
        store.put(Documents::Runs, &run).unwrap();
    }

    #[test]
    fn persisted_reservations_cancel_late_launches_and_restart_requires_explicit_resume() {
        let home = tempfile::tempdir().unwrap();
        let path = home.path().join("synthetic.sqlite");
        let connection = super::super::store::open(&path).unwrap();
        let store = Store::new(&connection);
        let (initial, project) = seed(&store);
        let mut request = body(&initial, &project, "start");
        request.set("confirmed", &false).unwrap();
        assert!(change(&store, "task", &request).is_err());
        request.set("confirmed", &true).unwrap();
        let started = change(&store, "task", &request).unwrap();
        let flow = read(&started).unwrap().unwrap();
        assert!(blocks(&store, "/api/backups/restore").unwrap());
        assert!(blocks(&store, "/api/projects/project").unwrap());
        assert!(blocks(&store, "/api/updates/apply").unwrap());
        assert!(!blocks(&store, "/api/cards/other").unwrap());
        let (reserved, _, _) = claim(&store, "task", &flow.id, "late-run".into()).unwrap();
        assert!(require_run(&store, &reserved, "manual", None).is_err());
        assert!(require_run(&store, &reserved, "late-run", Some(&flow.id)).is_ok());
        cancel(&store, "task", &flow.id).unwrap();
        assert!(require_run(&store, &card(&store), "late-run", Some(&flow.id)).is_err());
        let late = doc(
            json!({"id":"late-run","cardId":"task","automationId":flow.id,"repo":"synthetic/work","status":"starting"}),
        );
        assert!(runs::reserve(&store, &late).is_err());
        assert!(settle(&store, "task", &flow.id).is_err());
        let resumed = change(&store, "task", &body(&card(&store), &project, "resume")).unwrap();
        assert!(read(&resumed).unwrap().unwrap().run_id.is_none());
        claim(&store, "task", &flow.id, "interrupted-launch".into()).unwrap();
        drop(connection);
        let connection = super::super::store::open(&path).unwrap();
        let store = Store::new(&connection);
        recover(&store).unwrap();
        assert!(running(&store).unwrap().is_none());
        let interrupted = card(&store);
        assert!(read(&interrupted).unwrap().unwrap().state == State::Interrupted);
        assert!(
            store
                .documents::<Document>(Documents::Runs)
                .unwrap()
                .is_empty()
        );
        let mut changed = project.clone();
        changed.set("testCommand", &"changed").unwrap();
        store.put(Documents::Projects, &changed).unwrap();
        assert!(change(&store, "task", &body(&interrupted, &changed, "resume")).is_err());
        assert_eq!(
            serde_json::to_value(card(&store)).unwrap(),
            serde_json::to_value(interrupted).unwrap()
        );
    }

    #[test]
    fn successful_local_steps_wait_for_publication_and_only_two_blocking_corrections_are_allowed() {
        let home = tempfile::tempdir().unwrap();
        let connection = super::super::store::open(&home.path().join("synthetic.sqlite")).unwrap();
        let store = Store::new(&connection);
        let (initial, project) = seed(&store);
        let started = change(&store, "task", &body(&initial, &project, "start")).unwrap();
        let mut flow = read(&started).unwrap().unwrap();
        synthetic_stage_result(&store, &flow, "development", json!({"purpose":"work"}));
        settle(&store, "task", &flow.id).unwrap();
        flow = read(&card(&store)).unwrap().unwrap();
        assert!(flow.step == Step::Tests);
        synthetic_stage_result(
            &store,
            &flow,
            "tests",
            json!({"purpose":"verification","verificationState":"stale"}),
        );
        assert!(settle(&store, "task", &flow.id).is_err());
        let mut run = store
            .document::<Document>(Documents::Runs, "synthetic-tests")
            .unwrap()
            .unwrap();
        run.set("verificationState", &"passed").unwrap();
        store.put(Documents::Runs, &run).unwrap();
        settle(&store, "task", &flow.id).unwrap();
        assert!(read(&card(&store)).unwrap().unwrap().state == State::Waiting);
        assert!(change(&store, "task", &body(&card(&store), &project, "review")).is_err());
        let mut linked = card(&store);
        linked.fields.set("pullRequest",&json!({"revision":1,"kind":"github","repository":"synthetic/repo","number":1,"title":"Synthetic","url":"https://synthetic.example.test/pr/1","status":"open","draft":true,"sourceBranch":"task","targetBranch":"main","fork":false,"baseUrl":"https://synthetic.example.test","checkedAt":"synthetic"})).unwrap();
        store.put(Documents::Cards, &linked).unwrap();
        for cycle in 0..3 {
            let resumed = change(&store, "task", &body(&card(&store), &project, "review")).unwrap();
            flow = read(&resumed).unwrap().unwrap();
            assert!(flow.step == Step::Tests);
            synthetic_stage_result(
                &store,
                &flow,
                &format!("published-tests-{cycle}"),
                json!({"purpose":"verification","verificationState":"passed"}),
            );
            settle(&store, "task", &flow.id).unwrap();
            flow = read(&card(&store)).unwrap().unwrap();
            assert!(flow.step == Step::Review);
            synthetic_stage_result(
                &store,
                &flow,
                &format!("review-{cycle}"),
                json!({"purpose":"review","reviewValidatedAt":"synthetic","review":{"summary":"Synthetic fixture, not an executed agent","findings":[{"priority":"P1","title":"Synthetic blocker","file":"file.rs","line":1,"problem":"Synthetic problem","suggestion":"Synthetic correction"}],"criteria":[{"index":0,"status":"covered","evidence":"Synthetic state fixture only"}]}}),
            );
            settle(&store, "task", &flow.id).unwrap();
            flow = read(&card(&store)).unwrap().unwrap();
            if cycle < 2 {
                assert!(flow.step == Step::Correction && flow.cycles == cycle + 1);
                synthetic_stage_result(
                    &store,
                    &flow,
                    &format!("correction-{cycle}"),
                    json!({"purpose":"correction"}),
                );
                settle(&store, "task", &flow.id).unwrap();
                flow = read(&card(&store)).unwrap().unwrap();
                synthetic_stage_result(
                    &store,
                    &flow,
                    &format!("corrected-tests-{cycle}"),
                    json!({"purpose":"verification","verificationState":"passed"}),
                );
                settle(&store, "task", &flow.id).unwrap();
            } else {
                assert!(flow.state == State::Blocked && flow.cycles == 2);
            }
        }
        assert_eq!(
            card(&store)
                .fields
                .get::<Vec<serde_json::Value>>("checklist")
                .unwrap()
                .unwrap()[0]["done"],
            false
        );
    }

    #[test]
    fn publication_freezes_the_preview_requires_separate_branch_consent_and_never_replays_after_recovery()
     {
        let home = tempfile::tempdir().unwrap();
        let connection = super::super::store::open(&home.path().join("synthetic.sqlite")).unwrap();
        let store = Store::new(&connection);
        store.save_settings(&doc(json!({"forgeRevision":1,"forgeConnections":{"github":{"kind":"github","base":"https://github-synthetic.example.test"}}}))).unwrap();
        let (initial, project) = seed(&store);
        let started = change(&store, "task", &body(&initial, &project, "start")).unwrap();
        let mut flow = read(&started).unwrap().unwrap();
        synthetic_stage_result(
            &store,
            &flow,
            "publication-development",
            json!({"purpose":"work"}),
        );
        settle(&store, "task", &flow.id).unwrap();
        flow = read(&card(&store)).unwrap().unwrap();
        synthetic_stage_result(
            &store,
            &flow,
            "publication-tests",
            json!({"purpose":"verification","verificationState":"passed"}),
        );
        settle(&store, "task", &flow.id).unwrap();
        assert!(change(&store, "task", &body(&card(&store), &project, "publish")).is_err());
        let mut ready = card(&store);
        let preview:atelier_core::contracts::ForgePublicationPreview=serde_json::from_str(&json!({"configuration":{"kind":"github","base":"https://github-synthetic.example.test"},"revision":1,"repository":"owner/repo","worktree":flow.worktree,"target":"main","origin":"https://github-synthetic.example.test/owner/repo.git","remoteSource":null,"localChanges":false,"sourceCommit":"a".repeat(40),"targetCommit":"b".repeat(40),"title":"Synthetic approved title","body":"Synthetic approved body","draft":true}).to_string()).unwrap();
        ready
            .fields
            .set("forgePublicationPreview", &preview)
            .unwrap();
        store.put(Documents::Cards, &ready).unwrap();
        assert!(change(&store, "task", &body(&ready, &project, "publish")).is_err());
        let mut input = body(&ready, &project, "publish");
        input.set("allowCreateBranch", &true).unwrap();
        let authorized = change(&store, "task", &input).unwrap();
        flow = read(&authorized).unwrap().unwrap();
        assert!(
            flow.state == State::Running
                && flow.step == Step::Tests
                && flow.publication_plan.is_some()
        );
        assert!(publication(&store, "task", &flow.id).is_err());
        synthetic_stage_result(
            &store,
            &flow,
            "publication-reverification",
            json!({"purpose":"verification","verificationState":"passed"}),
        );
        settle(&store, "task", &flow.id).unwrap();
        let (_, _, frozen) = publication(&store, "task", &flow.id).unwrap();
        assert_eq!(
            serde_json::to_value(frozen).unwrap(),
            serde_json::to_value(PublicationPlan::Create(Box::new(preview))).unwrap()
        );
        let original = card(&store);
        let mut changed = original.clone();
        changed.fields.set("pullRequest",&json!({"revision":1,"kind":"github","repository":"owner/repo","number":7,"title":"Synthetic approved title","url":"https://github-synthetic.example.test/owner/repo/pull/7","status":"active","draft":true,"sourceBranch":"task","targetBranch":"main","sourceCommit":"a".repeat(40),"targetCommit":"c".repeat(40),"fork":false,"baseUrl":"https://github-synthetic.example.test","checkedAt":"synthetic"})).unwrap();
        store.put(Documents::Cards, &changed).unwrap();
        assert!(publication_complete(&store, &changed, &flow.id).is_err());
        assert!(read(&card(&store)).unwrap().unwrap().step == Step::Publication);
        store.put(Documents::Cards, &original).unwrap();
        let before = store.documents::<Document>(Documents::Runs).unwrap().len();
        recover(&store).unwrap();
        assert!(publication(&store, "task", &flow.id).is_err());
        assert!(running(&store).unwrap().is_none());
        let resumed = change(&store, "task", &body(&card(&store), &project, "resume")).unwrap();
        let flow = read(&resumed).unwrap().unwrap();
        assert!(flow.step == Step::Tests && flow.publication_plan.is_none());
        assert_eq!(
            store.documents::<Document>(Documents::Runs).unwrap().len(),
            before
        );
        cancel(&store, "task", &flow.id).unwrap();
        assert!(publication(&store, "task", &flow.id).is_err());
        assert!(
            store
                .document::<Document>(Documents::Cards, "task")
                .unwrap()
                .unwrap()
                .raw("gitPush")
                .is_none()
        );
        assert!(
            store
                .document::<Document>(Documents::Cards, "task")
                .unwrap()
                .unwrap()
                .raw("forgePublication")
                .is_none()
        );
    }

    #[test]
    fn workflow_commit_keeps_the_approved_snapshot_and_never_treats_commit_as_verification() {
        let home = tempfile::tempdir().unwrap();
        let connection = super::super::store::open(&home.path().join("synthetic.sqlite")).unwrap();
        let store = Store::new(&connection);
        let (initial, project) = seed(&store);
        let started = change(&store, "task", &body(&initial, &project, "start")).unwrap();
        let mut flow = read(&started).unwrap().unwrap();
        synthetic_stage_result(
            &store,
            &flow,
            "commit-development",
            json!({"purpose":"work"}),
        );
        settle(&store, "task", &flow.id).unwrap();
        flow = read(&card(&store)).unwrap().unwrap();
        synthetic_stage_result(
            &store,
            &flow,
            "commit-prior-tests",
            json!({"purpose":"verification","verificationState":"passed"}),
        );
        settle(&store, "task", &flow.id).unwrap();
        let mut ready = card(&store);
        assert!(change(&store, "task", &body(&ready, &project, "commit")).is_err());
        let preview = atelier_core::contracts::LocalCommitPreview {
            worktree: flow.worktree.clone(),
            snapshot: atelier_core::contracts::LocalCommitSnapshot {
                branch: "task".into(),
                head: "a".repeat(40),
                tree: "b".repeat(40),
                paths: vec!["file.txt".into()],
            },
            author_name: "Synthetic author".into(),
            author_email: "synthetic@example.invalid".into(),
            message: "Synthetic immutable commit".into(),
        };
        ready.fields.set("localCommitPreview", &preview).unwrap();
        store.put(Documents::Cards, &ready).unwrap();
        let authorized = change(&store, "task", &body(&ready, &project, "commit")).unwrap();
        let (_, frozen) = commit(&store, "task", &flow.id).unwrap();
        assert_eq!(
            serde_json::to_value(frozen).unwrap(),
            serde_json::to_value(&preview).unwrap()
        );
        assert!(read(&authorized).unwrap().unwrap().step == Step::Commit);
        assert!(commit_complete(&store, &authorized, &flow.id).is_err());
        let mut observed = authorized;
        let mut effect = atelier_core::contracts::LocalCommitIntent {
            id: "synthetic-effect".into(),
            state: atelier_core::contracts::ForgeWriteState::Confirmed,
            preview: preview.clone(),
            commit: Some("c".repeat(40)),
            observed: Some("d".repeat(40)),
        };
        observed.fields.set("localCommit", &effect).unwrap();
        store.put(Documents::Cards, &observed).unwrap();
        assert!(commit_complete(&store, &observed, &flow.id).is_err());
        effect.observed = effect.commit.clone();
        observed.fields.set("localCommit", &effect).unwrap();
        store.put(Documents::Cards, &observed).unwrap();
        commit_complete(&store, &observed, &flow.id).unwrap();
        let finished = card(&store);
        let flow = read(&finished).unwrap().unwrap();
        assert!(
            flow.state == State::Running
                && flow.step == Step::Tests
                && flow.commit_plan.is_none()
                && flow.verification_id.is_none()
        );
        assert_eq!(finished.title, initial.title);
        assert_eq!(
            finished
                .fields
                .get::<Vec<serde_json::Value>>("checklist")
                .unwrap()
                .unwrap()[0]["done"],
            false
        );
        recover(&store).unwrap();
        let resumed = change(&store, "task", &body(&card(&store), &project, "resume")).unwrap();
        let flow = read(&resumed).unwrap().unwrap();
        assert!(flow.step == Step::Tests && flow.commit_plan.is_none());
        cancel(&store, "task", &flow.id).unwrap();
        assert!(commit(&store, "task", &flow.id).is_err());
    }
}
