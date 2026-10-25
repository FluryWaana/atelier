use atelier_app::infrastructure::{
    backups::BackupDirectory, commits, database::Database, forge_draft, store::Documents,
};
use atelier_core::contracts::{
    Card, ForgeWriteState as Phase, LocalCommitIntent, LocalCommitPreview, LocalCommitRequest,
    LocalCommitSnapshot, Worktree, WorktreeState,
};
use serde_json::json;

#[tokio::test]
async fn commit_identity_survives_restart_and_backup_cannot_erase_an_observed_effect() {
    let home = tempfile::tempdir().unwrap();
    let path = home.path().to_owned();
    let database = Database::open(std::path::Path::new(":memory:")).unwrap();
    database.call(move |store| {
        let worktree = Worktree { path: "synthetic-worktree".into(), source: "synthetic-source".into(), branch: "task".into(), base: "main".into(), base_commit: "a".repeat(40), state: WorktreeState::Ready };
        let card: Card = serde_json::from_str(&json!({"id":"task","title":"Human title","repo":"synthetic-source","description":"Keep human text","worktree":worktree,"checklist":[{"text":"Human criterion","done":true}]}).to_string())?;
        store.put(Documents::Cards, &card)?;
        let backups = BackupDirectory::open(&path)?;
        let old = backups.create(store)?;
        let preview = LocalCommitPreview { worktree, snapshot: LocalCommitSnapshot { branch: "task".into(), head: "a".repeat(40), tree: "b".repeat(40), paths: vec!["selected.txt".into()] }, author_name: "Synthetic".into(), author_email: "synthetic@example.invalid".into(), message: "Synthetic selected commit".into() };
        let card = commits::save_preview(store, &card, preview.clone())?;
        let mut input = LocalCommitRequest { expected: card, paths: preview.snapshot.paths, author_name: preview.author_name, author_email: preview.author_email, message: preview.message, confirmed: false };
        let id = uuid::Uuid::new_v4().to_string();
        assert!(commits::reserve(store, &input, &id).is_err());
        input.confirmed = true;
        input.message.push_str(" changed");
        assert!(commits::reserve(store, &input, &id).is_err());
        input.message = "Synthetic selected commit".into();
        let reserved = commits::reserve(store, &input, &id)?;
        assert!(atelier_core::application::forge::unresolved(&reserved.fields)?);
        assert!(backups.restore(store, &old.name).is_err());
        forge_draft::recover(store)?;
        input.expected = store.document::<Card>(Documents::Cards, "task")?.unwrap();
        assert_eq!(input.expected.fields.get::<LocalCommitIntent>("localCommit")?.unwrap().state, Phase::Failed);
        let reserved = commits::reserve(store, &input, &id)?;
        commits::sending(store, &reserved, &id, "c".repeat(40))?;
        forge_draft::recover(store)?;
        let recovered = store.document::<Card>(Documents::Cards, "task")?.unwrap();
        let intent = recovered.fields.get::<LocalCommitIntent>("localCommit")?.unwrap();
        assert_eq!(intent.state, Phase::Uncertain);
        assert_eq!(intent.commit, Some("c".repeat(40)));
        for path in ["/api/cards/task", "/api/cards/task/commit/create", "/api/cards/task/worktree/remove", "/api/projects", "/api/backups"] { assert!(forge_draft::blocks(store, path)?); }
        assert!(!forge_draft::blocks(store, "/api/cards/task/commit/reconcile")?);
        let unchanged = commits::record(store, "task", &id, "a".repeat(40))?;
        assert_eq!(unchanged.fields.get::<LocalCommitIntent>("localCommit")?.unwrap().state, Phase::Uncertain);
        let confirmed = commits::record(store, "task", &id, "c".repeat(40))?;
        assert_eq!(confirmed.fields.get::<LocalCommitIntent>("localCommit")?.unwrap().state, Phase::Confirmed);
        assert!(confirmed.fields.raw("localCommitEffect").is_some());
        assert!(confirmed.fields.raw("localCommitPreview").is_none());
        assert!(!atelier_core::application::forge::unresolved(&confirmed.fields)?);
        assert_eq!(confirmed.title.utf8()?, "Human title");
        assert!(confirmed.fields.string_equals("description", "Keep human text")?);
        assert!(confirmed.fields.get::<Vec<atelier_core::domain::checklist::Criterion>>("checklist")?.unwrap()[0].done);
        assert!(backups.restore(store, &old.name).is_err());
        assert!(commits::sending(store, &reserved, &id, "d".repeat(40)).is_err());
        assert!(commits::record(store, "task", &id, "d".repeat(40)).is_err());
        Ok(())
    }).await.unwrap();
    database.close().await.unwrap();
}
