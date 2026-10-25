use atelier_app::infrastructure::{database::Database, runs, store::Documents, verification};
use atelier_core::{contracts::Card, document::Document};
use serde_json::json;
fn document(value: serde_json::Value) -> Document {
    serde_json::from_str(&value.to_string()).unwrap()
}
#[tokio::test]
async fn verification_evidence_requires_matching_sources_requirements_command_and_success() {
    let database = Database::open(std::path::Path::new(":memory:")).unwrap();
    database.call(|store|{
        let card:Card=serde_json::from_str(&json!({"id":"task","title":"Synthetic","repo":"/synthetic/repo","checklist":[{"text":"Human criterion","done":false}],"worktree":{"source":"/synthetic/repo","path":"/synthetic/work","branch":"task","base":"main","baseCommit":"a".repeat(40),"state":"ready"}}).to_string())?;
        let project=document(json!({"id":"project","repo":"/synthetic/repo","testCommand":"synthetic verify"}));store.put(Documents::Cards,&card)?;store.put(Documents::Projects,&project)?;
        verification::require_command(store,&card,"synthetic verify")?;assert!(verification::require_command(store,&card,"other verify").is_err());
        let run=document(json!({"id":"verification","cardId":"task","repo":"/synthetic/work","status":"starting","purpose":"verification","verificationState":"running","sourceFingerprint":"synthetic-hash","verificationCommand":"synthetic verify","verificationWorktree":card.fields.get::<atelier_core::contracts::Worktree>("worktree")?,"requirements":atelier_core::application::preparation::requirements(&card)?}));
        runs::reserve(store,&run)?;runs::transition(store,"verification","running","time",None)?;runs::transition(store,"verification","completed","time",Some(0))?;
        verification::finish(store,"verification",Some("synthetic-hash"))?;assert!(store.document::<Document>(Documents::Runs,"verification")?.unwrap().string_equals("verificationState","passed")?);
        verification::finish(store,"verification",Some("changed-hash"))?;assert!(store.document::<Document>(Documents::Runs,"verification")?.unwrap().string_equals("verificationState","stale")?);
        let mut changed=project;changed.set("testCommand",&"other verify")?;store.put(Documents::Projects,&changed)?;
        verification::finish(store,"verification",Some("synthetic-hash"))?;assert!(store.document::<Document>(Documents::Runs,"verification")?.unwrap().string_equals("verificationState","stale")?);
        assert_eq!(serde_json::to_value(store.document::<Card>(Documents::Cards,"task")?.unwrap())?,serde_json::to_value(&card)?);
        let mut ended=store.document::<Document>(Documents::Runs,"verification")?.unwrap();ended.set("verificationState",&"running")?;store.put(Documents::Runs,&ended)?;runs::recover(store,"restart")?;
        assert!(store.document::<Document>(Documents::Runs,"verification")?.unwrap().string_equals("verificationState","interrupted")?);
        Ok(())
    }).await.unwrap();
    database.close().await.unwrap();
}
