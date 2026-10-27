use atelier_app::infrastructure::{database::Database, queue, runs, store::Documents};
use atelier_core::{contracts::Card, document::Document};
use serde_json::json;
fn doc(value: serde_json::Value) -> Document {
    serde_json::from_str(&value.to_string()).unwrap()
}
#[tokio::test]
async fn queue_claims_are_unique_pause_blocks_spawn_and_restart_never_relaunches_uncertain_work() {
    let database = Database::open(std::path::Path::new(":memory:")).unwrap();
    database.call(|store|{
        let card:Card=serde_json::from_str(&json!({"id":"task","title":"Synthetic","agent":"synthetic-command","mode":"edit","repo":"/synthetic/repo","worktree":{"source":"/synthetic/repo","path":"/synthetic/work","branch":"task","base":"main","baseCommit":"a".repeat(40),"state":"ready"}}).to_string())?;store.put(Documents::Cards,&card)?;
        assert!(queue::change(store,&doc(json!({"action":"add","revision":0,"confirmed":false,"expected":card}))).is_err());
        assert!(!queue::needs_tick(store)?);let queued=queue::change(store,&doc(json!({"action":"add","revision":0,"confirmed":true,"expected":card})))?;assert_eq!(queued.ids,vec!["task"]);assert!(queued.paused);assert!(!queue::needs_tick(store)?);
        assert!(queue::change(store,&doc(json!({"action":"resume","revision":0,"confirmed":true}))).is_err());
        queue::change(store,&doc(json!({"action":"resume","revision":queued.revision,"confirmed":true})))?;
        assert!(queue::needs_tick(store)?);assert!(queue::claim(store,"first".into())?.is_some());assert!(queue::claim(store,"second".into())?.is_none());
        queue::pause(store,"Synthetic pause")?;let run=|id:&str|doc(json!({"id":id,"cardId":"task","status":"starting","queued":true,"purpose":"work","repo":"/synthetic/work"}));
        assert!(runs::reserve(store,&run("first")).is_err());assert!(store.document::<Document>(Documents::Runs,"first")?.is_none());queue::settle(store)?;let paused=queue::read(store)?;assert!(paused.paused);assert_eq!(paused.ids,vec!["task"]);assert!(paused.active.is_none());
        queue::change(store,&doc(json!({"action":"resume","revision":paused.revision,"confirmed":true})))?;queue::claim(store,"completed".into())?;runs::reserve(store,&run("completed"))?;runs::transition(store,"completed","running","time",None)?;runs::transition(store,"completed","completed","time",Some(0))?;assert!(queue::settle(store)?);assert!(queue::read(store)?.ids.is_empty());
        let state=queue::read(store)?;queue::change(store,&doc(json!({"action":"add","revision":state.revision,"confirmed":true,"expected":card})))?;queue::claim(store,"finished-before-crash".into())?;
        runs::reserve(store,&run("finished-before-crash"))?;runs::transition(store,"finished-before-crash","running","time",None)?;runs::transition(store,"finished-before-crash","completed","time",Some(0))?;
        queue::recover(store)?;assert!(queue::read(store)?.ids.is_empty());
        let state=queue::read(store)?;let added=queue::change(store,&doc(json!({"action":"add","revision":state.revision,"confirmed":true,"expected":card})))?;queue::change(store,&doc(json!({"action":"resume","revision":added.revision,"confirmed":true})))?;queue::claim(store,"uncertain".into())?;
        queue::recover(store)?;
        let recovered=queue::read(store)?;assert!(recovered.paused);assert_eq!(recovered.ids,vec!["task"]);assert!(recovered.active.is_none());assert!(queue::claim(store,"must-not-start".into())?.is_none());
        Ok(())
    }).await.unwrap();
    database.close().await.unwrap();
}

#[tokio::test]
async fn project_workflows_keep_their_identity_through_waits_restart_and_queue_resumption() {
    use atelier_app::infrastructure::automation;
    use atelier_core::application::automation::{Flow, State, Step};
    let database = Database::open(std::path::Path::new(":memory:")).unwrap();
    database.call(|store| {
        let project=doc(json!({"id":"project","name":"Synthetic project","repo":"synthetic/source","automationEnabled":true,"testCommand":"synthetic-check","prTarget":"main"}));store.put(Documents::Projects,&project)?;
        let card:Card=serde_json::from_str(&json!({"id":"workflow","title":"Synthetic workflow","agent":"codex","mode":"edit","repo":"synthetic/source","worktree":{"source":"synthetic/source","path":"synthetic/work","branch":"task","base":"main","baseCommit":"a".repeat(40),"state":"ready"}}).to_string())?;store.put(Documents::Cards,&card)?;
        let mut second=card.clone();second.id="simple".into();second.fields.remove("automation");store.put(Documents::Cards,&second)?;
        let added=queue::change(store,&doc(json!({"action":"add","workflow":true,"revision":0,"confirmed":true,"expected":card})))?;
        let added=queue::change(store,&doc(json!({"action":"add","revision":added.revision,"confirmed":true,"expected":second})))?;
        assert!(queue::change(store,&doc(json!({"action":"add","workflow":false,"revision":added.revision,"confirmed":true,"expected":card}))).is_err());
        queue::change(store,&doc(json!({"action":"resume","revision":added.revision,"confirmed":true})))?;
        let (mut active,body)=queue::claim(store,"not-a-command-run".into())?.unwrap();assert_eq!(body.get::<bool>("workflow")?,Some(true));
        let original=automation::read(&active)?.unwrap();assert_eq!(queue::read(store)?.active.unwrap().run_id,original.id);
        assert!(store.documents::<Document>(Documents::Runs)?.is_empty());
        let impostor=doc(json!({"id":original.id,"cardId":"workflow","status":"starting","repo":"synthetic/work","queued":true}));assert!(runs::reserve(store,&impostor).is_err());
        assert!(!queue::settle(store)?);
        // State fixture only: no agent, verification or publication result is asserted here.
        let mut waiting=original.clone();waiting.state=State::Waiting;waiting.step=Step::Publication;active.fields.set("automation",&waiting)?;store.put(Documents::Cards,&active)?;
        assert!(!queue::needs_tick(store)?);assert!(!queue::settle(store)?);assert_eq!(queue::read(store)?.ids,vec!["workflow","simple"]);
        automation::recover(store)?;queue::recover(store)?;let paused=queue::read(store)?;assert!(paused.paused && paused.active.is_none());
        assert_eq!(paused.workflows.get("workflow"),Some(&Some(original.id.clone())));
        queue::change(store,&doc(json!({"action":"resume","revision":paused.revision,"confirmed":true})))?;
        let (mut active,_)=queue::claim(store,"must-not-start-another-workflow".into())?.unwrap();let mut resumed=automation::read(&active)?.unwrap();assert_eq!(resumed.id,original.id);assert!(resumed.state==State::Waiting);
        resumed.state=State::Ready;active.fields.set("automation",&resumed)?;store.put(Documents::Cards,&active)?;assert!(queue::needs_tick(store)?);
        assert!(queue::settle(store)?);assert_eq!(queue::read(store)?.ids,vec!["simple"]);assert!(queue::read(store)?.workflows.is_empty());
        let (_,body)=queue::claim(store,"simple-run".into())?.unwrap();assert_eq!(body.get::<String>("purpose")?,Some("work".into()));
        let run=doc(json!({"id":"simple-run","cardId":"simple","repo":"synthetic/work","status":"starting","queued":true}));runs::reserve(store,&run)?;runs::transition(store,"simple-run","running","synthetic",None)?;runs::transition(store,"simple-run","completed","synthetic",Some(0))?;queue::settle(store)?;
        let current=store.document::<Card>(Documents::Cards,"workflow")?.unwrap();let state=queue::read(store)?;
        let added=queue::change(store,&doc(json!({"action":"add","workflow":true,"revision":state.revision,"confirmed":true,"expected":current})))?;
        queue::change(store,&doc(json!({"action":"resume","revision":added.revision,"confirmed":true})))?;
        let (active,_)=queue::claim(store,"new-authorization".into())?.unwrap();let next=automation::read(&active)?.unwrap();assert_ne!(next.id,original.id);
        automation::recover(store)?;queue::recover(store)?;
        let paused=queue::pause(store,"Synthetic stable pause")?;assert_eq!(queue::pause(store,"Synthetic stable pause")?.revision,paused.revision);
        queue::change(store,&doc(json!({"action":"resume","revision":paused.revision,"confirmed":true})))?;
        let (active,_)=queue::claim(store,"resume-same-operation".into())?.unwrap();let resumed:Flow=automation::read(&active)?.unwrap();assert_eq!(resumed.id,next.id);assert!(resumed.state==State::Running);
        assert!(resumed.commit_plan.is_none() && resumed.publication_plan.is_none());assert_eq!(store.documents::<Document>(Documents::Runs)?.len(),1);
        Ok(())
    }).await.unwrap();
    database.close().await.unwrap();
}

#[tokio::test]
async fn queue_claim_rolls_back_workflow_creation_when_the_settings_reservation_fails() {
    let home = tempfile::tempdir().unwrap();
    let path = home.path().join("synthetic.sqlite");
    let database = Database::open(&path).unwrap();
    database.call(|store| {
        store.put(Documents::Projects,&doc(json!({"id":"project","name":"Synthetic","repo":"synthetic/source","automationEnabled":true,"testCommand":"synthetic-check","prTarget":"main"})))?;
        let card:Card=serde_json::from_str(&json!({"id":"atomic-workflow","title":"Synthetic atomic workflow","repo":"synthetic/source","agent":"codex","worktree":{"source":"synthetic/source","path":"synthetic/work","branch":"task","base":"main","baseCommit":"a".repeat(40),"state":"ready"}}).to_string())?;store.put(Documents::Cards,&card)?;
        let added=queue::change(store,&doc(json!({"action":"add","workflow":true,"revision":0,"confirmed":true,"expected":card})))?;
        queue::change(store,&doc(json!({"action":"resume","revision":added.revision,"confirmed":true})))?;Ok(())
    }).await.unwrap();
    rusqlite::Connection::open(&path).unwrap().execute_batch("CREATE TRIGGER refuse_workflow_claim BEFORE UPDATE ON settings WHEN json_extract(NEW.data,'$.runQueue.active.cardId')='atomic-workflow' BEGIN SELECT RAISE(ABORT,'synthetic claim rollback'); END;").unwrap();
    database
        .call(|store| {
            assert!(queue::claim(store, "synthetic-not-spawned".into()).is_err());
            assert!(queue::read(store)?.active.is_none());
            assert!(
                store
                    .document::<Card>(Documents::Cards, "atomic-workflow")?
                    .unwrap()
                    .fields
                    .raw("automation")
                    .is_none()
            );
            assert!(store.documents::<Document>(Documents::Runs)?.is_empty());
            Ok(())
        })
        .await
        .unwrap();
    database.close().await.unwrap();
}
