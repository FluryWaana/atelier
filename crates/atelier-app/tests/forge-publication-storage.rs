#![cfg(windows)]
use atelier_app::infrastructure::{
    backups::BackupDirectory, database::Database, forge_draft, forge_publication as workflow,
    store::Documents,
};
use atelier_core::{
    contracts::{
        Card, ForgeConfiguration, ForgeKind, ForgePublicationIntent, ForgePublicationPreview,
        ForgePublicationRequest, ForgeWriteState as Phase, PullRequest, Worktree, WorktreeState,
    },
    document::Document,
};
use serde_json::json;
#[tokio::test]
async fn recovery_keeps_the_candidate_identity_without_replaying_or_overwriting_human_content() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().to_owned();
    let database = Database::open(std::path::Path::new(":memory:")).unwrap();
    database.call(move|store|{
        let settings:Document=serde_json::from_str(&json!({"forgeRevision":1,"forgeConnections":{"github":{"kind":"github","base":"https://github.example.test"}}}).to_string())?;store.save_settings(&settings)?;
        let worktree=Worktree{path:"synthetic-worktree".into(),source:"synthetic-source".into(),branch:"task".into(),base:"main".into(),base_commit:"b".repeat(40),state:WorktreeState::Ready};
        let card:Card=serde_json::from_str(&json!({"id":"task","title":"Human title","description":"Keep human text","repo":"synthetic-source","worktree":worktree,"checklist":[{"text":"Human criterion","done":true}]}).to_string())?;store.put(Documents::Cards,&card)?;
        let backups=BackupDirectory::open(&path)?;let old=backups.create(store)?;
        let preview=ForgePublicationPreview{configuration:ForgeConfiguration{kind:ForgeKind::Github,base:"https://github.example.test".into()},revision:1,repository:"owner/repo".into(),worktree,target:"main".into(),source_commit:"a".repeat(40),target_commit:"b".repeat(40),title:"Published title".into(),body:"Published description".into(),draft:true};
        let card=workflow::save_preview(store,&card,preview)?;
        let mut input=ForgePublicationRequest{expected:card,revision:1,kind:ForgeKind::Github,repository:"owner/repo".into(),target:"main".into(),title:"Published title".into(),body:"Published description".into(),draft:true,confirmed:false};
        let operation=uuid::Uuid::new_v4().to_string();assert!(workflow::reserve(store,&input,&operation).is_err());input.confirmed=true;
        workflow::reserve(store,&input,&operation)?;assert!(backups.restore(store,&old.name).is_err());forge_draft::recover(store)?;
        input.expected=store.document::<Card>(Documents::Cards,"task")?.unwrap();assert_eq!(input.expected.fields.get::<ForgePublicationIntent>("forgePublication")?.unwrap().state,Phase::Failed);
        let reserved=workflow::reserve(store,&input,&operation)?;workflow::sending(store,&reserved,&operation)?;workflow::remember_number(store,"task",&operation,7)?;forge_draft::recover(store)?;
        let recovered=store.document::<Card>(Documents::Cards,"task")?.unwrap();let intent=recovered.fields.get::<ForgePublicationIntent>("forgePublication")?.unwrap();assert_eq!(intent.state,Phase::Uncertain);assert_eq!(intent.number,Some(7));
        assert!(forge_draft::blocks(store,"/api/cards/task/worktree/remove")?);assert!(!forge_draft::blocks(store,"/api/cards/task/forge/publication-reconcile")?);
        let pr:PullRequest=serde_json::from_value(json!({"revision":1,"kind":"github","repository":"owner/repo","number":7,"title":"Published title","url":"https://github.example.test/owner/repo/pull/7","status":"active","draft":true,"sourceBranch":"task","targetBranch":"main","sourceCommit":"c".repeat(40),"targetCommit":"b".repeat(40),"fork":false,"baseUrl":"https://github.example.test","checkedAt":"2026-10-25T10:00:00Z"}))?;
        let mut wrong=pr.clone();wrong.number=8;assert!(workflow::record(store,"task",&operation,wrong).is_err());
        let other:Card=serde_json::from_str(&json!({"id":"other","title":"Other task","pullRequest":pr}).to_string())?;store.put(Documents::Cards,&other)?;
        let duplicate=workflow::record(store,"task",&operation,pr.clone())?;assert!(duplicate.fields.raw("pullRequest").is_none());assert_eq!(duplicate.fields.get::<ForgePublicationIntent>("forgePublication")?.unwrap().state,Phase::Uncertain);
        store.delete(Documents::Cards,"other")?;let linked=workflow::record(store,"task",&operation,pr)?;
        assert_eq!(linked.fields.get::<ForgePublicationIntent>("forgePublication")?.unwrap().state,Phase::Confirmed);assert!(linked.fields.raw("forgePublicationEffect").is_some());assert!(linked.fields.raw("forgePublicationPreview").is_none());
        assert_eq!(linked.title.utf8()?,"Human title");assert!(linked.fields.string_equals("description","Keep human text")?);assert!(linked.fields.get::<Vec<atelier_core::domain::checklist::Criterion>>("checklist")?.unwrap()[0].done);
        assert!(workflow::sending(store,&reserved,&operation).is_err());assert!(backups.restore(store,&old.name).is_err());Ok(())
    }).await.unwrap();
    database.close().await.unwrap();
}
