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
        let queued=queue::change(store,&doc(json!({"action":"add","revision":0,"confirmed":true,"expected":card})))?;assert_eq!(queued.ids,vec!["task"]);assert!(queued.paused);
        assert!(queue::change(store,&doc(json!({"action":"resume","revision":0,"confirmed":true}))).is_err());
        queue::change(store,&doc(json!({"action":"resume","revision":queued.revision,"confirmed":true})))?;
        assert!(queue::claim(store,"first".into())?.is_some());assert!(queue::claim(store,"second".into())?.is_none());
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
