use atelier_app::infrastructure::{
    automation, connector_effects, database::Database, queue, runs, store::Documents,
};
use atelier_core::{contracts::Card, document::Document};
use serde_json::{Value, json};

fn doc(value: Value) -> Document {
    serde_json::from_str(&value.to_string()).unwrap()
}
fn journal(state: &str) -> Value {
    json!([{"id":"synthetic-operation","module":"synthetic-guard","version":1,"revision":1,"manifest":{"id":"synthetic-guard","name":"Synthetic guard","description":"No execution","sourceLocale":"en-US","methods":["check","action","reconcile"],"fields":[],"tabs":[{"id":"synthetic","label":"Synthetic","description":"No remote access","actions":[{"id":"write","label":"Synthetic write","effect":"remote"}]}],"translations":{}},"binary":{"path":"C:/synthetic/unexecuted.exe","sha256":"ab".repeat(32)},"values":{},"method":"action","action":"write","input":{},"state":state,"startedAt":"2026-10-27T00:00:00Z","updatedAt":"2026-10-27T00:00:00Z","reply":null,"requestId":"synthetic-request"}])
}

#[tokio::test]
async fn pending_module_writes_pause_queue_and_block_sql_run_reservation_without_consuming_work() {
    for phase in ["preparing", "sending", "uncertain"] {
        let database = Database::open(std::path::Path::new(":memory:")).unwrap();
        let phase = phase.to_owned();
        database.call(move|store| {
            let card:Card=serde_json::from_str(&json!({"id":"synthetic-task","title":"Human draft","agent":"codex","mode":"edit","repo":"synthetic/source","worktree":{"source":"synthetic/source","path":"synthetic/work","branch":"task","base":"main","baseCommit":"a".repeat(40),"state":"ready"}}).to_string())?;
            store.put(Documents::Cards,&card)?;
            let added=queue::change(store,&doc(json!({"action":"add","revision":0,"confirmed":true,"expected":card})))?;
            queue::change(store,&doc(json!({"action":"resume","revision":added.revision,"confirmed":true})))?;
            let mut settings=store.settings::<Document>()?;settings.set("connectorEffects",&journal(&phase))?;store.save_settings(&settings)?;
            assert_eq!(connector_effects::read(&settings)?.len(),1);
            assert!(queue::claim(store,"must-not-spawn".into())?.is_none());
            let paused=queue::read(store)?;assert!(paused.paused && paused.active.is_none());assert_eq!(paused.ids,vec!["synthetic-task"]);
            assert_eq!(paused.message, "message.queue_paused_complete_or_reconcile_the_module_write_before_resum");
            let run=doc(json!({"id":"manual-reservation","cardId":"synthetic-task","repo":"synthetic/work","status":"starting"}));
            let error=runs::reserve(store,&run).unwrap_err();assert_eq!(error.to_string(), "message.complete_or_reconcile_module_writes_before_starting_a_run");
            assert!(store.documents::<Document>(Documents::Runs)?.is_empty());
            assert_eq!(serde_json::to_value(store.document::<Card>(Documents::Cards,&card.id)?.unwrap())?,serde_json::to_value(&card)?);
            let mut settings=store.settings::<Document>()?;settings.set("connectorEffects",&journal("failed"))?;store.save_settings(&settings)?;
            assert!(queue::claim(store,"no-automatic-replay".into())?.is_none());
            queue::change(store,&doc(json!({"action":"resume","revision":paused.revision,"confirmed":true})))?;
            assert!(queue::claim(store,"explicit-resumption".into())?.is_some());
            assert_eq!(queue::read(store)?.active.unwrap().run_id,"explicit-resumption");
            Ok(())
        }).await.unwrap();
        database.close().await.unwrap();
    }
}

#[tokio::test]
async fn automation_worker_keeps_its_step_and_identity_while_module_effects_are_pending() {
    let database = Database::open(std::path::Path::new(":memory:")).unwrap();
    database.call(|store| {
        let project=doc(json!({"id":"project","name":"Synthetic project","repo":"synthetic/source","automationEnabled":true,"testCommand":"synthetic-check","prTarget":"main"}));store.put(Documents::Projects,&project)?;
        let card:Card=serde_json::from_str(&json!({"id":"workflow","title":"Human task","agent":"codex","mode":"edit","repo":"synthetic/source","worktree":{"source":"synthetic/source","path":"synthetic/work","branch":"task","base":"main","baseCommit":"a".repeat(40),"state":"ready"}}).to_string())?;store.put(Documents::Cards,&card)?;
        let added=queue::change(store,&doc(json!({"action":"add","workflow":true,"revision":0,"confirmed":true,"expected":card})))?;
        queue::change(store,&doc(json!({"action":"resume","revision":added.revision,"confirmed":true})))?;
        let (active,_)=queue::claim(store,"workflow-start".into())?.unwrap();
        let original=automation::read(&active)?.unwrap();
        for phase in ["preparing","sending","uncertain"] {
            let mut settings=store.settings::<Document>()?;settings.set("connectorEffects",&journal(phase))?;store.save_settings(&settings)?;
            assert!(automation::running(store)?.is_none());
            assert_eq!(serde_json::to_value(store.document::<Card>(Documents::Cards,&active.id)?.unwrap())?,serde_json::to_value(&active)?);
            assert!(store.documents::<Document>(Documents::Runs)?.is_empty());
        }
        let mut settings=store.settings::<Document>()?;settings.set("connectorEffects",&journal("failed"))?;store.save_settings(&settings)?;
        let (_,resumed)=automation::running(store)?.unwrap();
        assert_eq!(serde_json::to_value(resumed)?,serde_json::to_value(original)?);
        Ok(())
    }).await.unwrap();
    database.close().await.unwrap();
}
