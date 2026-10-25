use atelier_app::infrastructure::{database::Database, reviews, runs, store::Documents};
use atelier_core::{
    application::reviews::{self as domain, Context},
    contracts::{Card, ForgeKind, PullRequest, Worktree, WorktreeState},
    document::Document,
};
use serde_json::json;
fn document(value: serde_json::Value) -> Document {
    serde_json::from_str(&value.to_string()).unwrap()
}

#[tokio::test]
async fn review_results_are_complete_snapshot_bound_and_never_tick_human_criteria() {
    let database = Database::open(std::path::Path::new(":memory:")).unwrap();
    database.call(|store| {
        let worktree=Worktree {path:"/synthetic/work".into(),source:"/synthetic/repo".into(),branch:"task".into(),base:"main".into(),base_commit:"a".repeat(40),state:WorktreeState::Ready};
        let pr=PullRequest {revision:7,kind:ForgeKind::Github,repository:"owner/repo".into(),number:1,title:"Synthetic".into(),url:"https://github.example.test/owner/repo/pull/1".into(),status:"active".into(),draft:true,source_branch:"task".into(),target_branch:"main".into(),source_commit:Some("b".repeat(40)),target_commit:Some("a".repeat(40)),fork:false,base_url:"https://github.example.test".into(),checked_at:"time".into()};
        let mut card:Card=serde_json::from_str(&json!({"id":"task","title":"Synthetic","repo":"/synthetic/repo","checklist":[{"text":"Observed behavior","done":false}],"worktree":worktree,"pullRequest":pr}).to_string())?;
        let context=Context {requirements:domain::requirements(&card)?,worktree:worktree.clone(),kind:ForgeKind::Github,revision:7,repository:"owner/repo".into(),number:1,source:"b".repeat(40),target:"a".repeat(40),base:"a".repeat(40)};
        store.save_settings(&document(json!({"forgeRevision":7})))?;
        store.put(Documents::Cards,&card)?;
        let run=document(json!({"id":"review","cardId":"task","repo":"/synthetic/work","status":"starting","kind":"pr-review","purpose":"review","agentType":"command","reviewContext":context,"reviewStaleReason":reviews::UNCHECKED}));
        runs::reserve(store,&run)?;runs::transition(store,"review","running","time",None)?;
        runs::append(store,"review","stdout",&json!({"summary":"Read code, UI not executed","findings":[],"criteria":[{"index":0,"status":"unverifiable","evidence":"UI not executed"}]}).to_string())?;
        let completed=runs::transition(store,"review","completed","time",Some(0))?;
        assert!(completed.get::<domain::Review>("review")?.is_some());
        assert!(completed.string_equals("reviewStaleReason",reviews::UNCHECKED)?);
        let validated=reviews::validate(store,&card,"review",&context,"verified")?;
        assert!(validated.raw("reviewStaleReason").is_none());assert!(validated.string_equals("reviewValidatedAt","verified")?);
        let retained=store.document::<Card>(Documents::Cards,"task")?.unwrap();
        assert_eq!(serde_json::to_value(&retained)?,serde_json::to_value(&card)?);
        let mut advanced=context.clone();advanced.source="c".repeat(40);assert!(reviews::validate(store,&card,"review",&advanced,"later").is_err());
        card.fields.set("description",&"Changed need")?;store.put(Documents::Cards,&card)?;
        assert!(reviews::validate(store,&card,"review",&context,"later").is_err());
        card=retained;store.put(Documents::Cards,&card)?;
        store.save_settings(&document(json!({"forgeRevision":8})))?;
        assert!(reviews::validate(store,&card,"review",&context,"later").is_err());
        store.save_settings(&document(json!({"forgeRevision":7})))?;
        store.put(Documents::Runs,&document(json!({"id":"busy","cardId":"task","status":"running"})))?;
        assert!(reviews::validate(store,&card,"review",&context,"later").is_err());store.delete(Documents::Runs,"busy")?;
        let mut moved=worktree;moved.branch="different".into();card.fields.set("worktree",&moved)?;store.put(Documents::Cards,&card)?;
        assert!(reviews::validate(store,&card,"review",&context,"later").is_err());
        assert!(store.document::<Document>(Documents::Runs,"review")?.unwrap().string_equals("reviewValidatedAt","verified")?);
        Ok(())
    }).await.unwrap();
    database.close().await.unwrap();
}
