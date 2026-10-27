use atelier_app::infrastructure::{
    database::Database, forge_association as workflow, store::Documents,
};
use atelier_core::{
    contracts::{
        Card, ForgeConfiguration, ForgeKind, PullRequest, PullRequestAttach, PullRequestDetach,
    },
    document::Document,
};
use serde_json::json;
fn document(value: serde_json::Value) -> Document {
    serde_json::from_str(&value.to_string()).unwrap()
}
fn configuration(kind: ForgeKind, base: &str) -> ForgeConfiguration {
    ForgeConfiguration {
        kind,
        base: base.into(),
    }
}
fn pr() -> PullRequest {
    PullRequest {
        revision: 1,
        kind: ForgeKind::Github,
        repository: "owner/repo".into(),
        number: 7,
        title: "Synthetic PR".into(),
        body: String::new(),
        url: "https://github.example.test/owner/repo/pull/7".into(),
        status: "active".into(),
        draft: true,
        source_branch: "task/synthetic".into(),
        target_branch: "main".into(),
        source_commit: Some("a".repeat(40)),
        target_commit: Some("b".repeat(40)),
        fork: false,
        base_url: "https://github.example.test".into(),
        checked_at: "2026-10-25T10:00:00Z".into(),
    }
}
#[test]
fn origin_matching_binds_host_instance_repository_and_transport_without_reading_credentials() {
    assert!(
        !workflow::matches_origin(
            &configuration(ForgeKind::Gitlab, "https://gitlab.example.test"),
            "path/owner/repo",
            "git@gitlab.example.test/path:owner/repo"
        )
        .unwrap()
    );
    for (kind, base, repo, accepted) in [
        (
            ForgeKind::Github,
            "https://github.example.test",
            "owner/repo",
            vec![
                "https://github.example.test/owner/repo.git",
                "git@github.example.test:owner/repo.git",
                "ssh://git@github.example.test/owner/repo",
            ],
        ),
        (
            ForgeKind::Gitlab,
            "https://gitlab.example.test/prefix",
            "group/nested/repo",
            vec!["https://gitlab.example.test/prefix/group/nested/repo.git"],
        ),
        (
            ForgeKind::Azure,
            "https://dev.azure.com/synthetic-org",
            "project/repo",
            vec![
                "https://dev.azure.com/synthetic-org/project/_git/repo",
                "git@ssh.dev.azure.com:v3/synthetic-org/project/repo",
                "ssh://git@ssh.dev.azure.com/v3/synthetic-org/project/repo",
            ],
        ),
    ] {
        let configuration = configuration(kind, base);
        for origin in accepted {
            assert!(workflow::matches_origin(&configuration, repo, origin).unwrap());
        }
        for origin in [
            "https://evil.example.test/owner/repo.git",
            "https://github.example.test.evil.test/owner/repo.git",
            "https://user:synthetic-token@github.example.test/owner/repo",
            "https://github.example.test/other/repo",
            "file:///synthetic",
            "git@other.example.test:owner/repo",
            "ssh://user@github.example.test/owner/repo",
            "ssh://git@github.example.test:2222/owner/repo",
            "https://github.example.test/owner/repo?token=synthetic-token",
        ] {
            assert!(!workflow::matches_origin(&configuration, repo, origin).unwrap());
        }
    }
    assert!(
        !workflow::matches_origin(
            &configuration(ForgeKind::Gitlab, "https://gitlab.example.test/prefix"),
            "group/repo",
            "git@gitlab.example.test:group/repo"
        )
        .unwrap()
    );
    assert!(
        !workflow::matches_origin(
            &configuration(ForgeKind::Azure, "https://dev.azure.com/org"),
            "project/repo",
            "git@ssh.dev.azure.com:v3/other-org/project/repo"
        )
        .unwrap()
    );
}
#[tokio::test]
async fn association_preserves_local_work_invalidates_changed_commits_and_requires_explicit_detach()
{
    let database = Database::open(std::path::Path::new(":memory:")).unwrap();
    database.call(|store|{
        store.save_settings(&document(json!({"forgeRevision":1,"forgeConnections":{"github":{"kind":"github","base":"https://github.example.test"}}})))?;
        let card:Card=serde_json::from_str(&json!({"id":"task","title":"Local title","description":"Keep description","instructions":"Keep instructions","status":"review","checklist":[{"text":"Human criterion","done":true}],"repo":"synthetic-source","worktree":{"path":"synthetic-worktree","source":"synthetic-source","branch":"task/synthetic","base":"main","baseCommit":"a".repeat(40),"state":"ready"},"automation":{"state":"ready"}}).to_string())?;store.put(Documents::Cards,&card)?;
        store.put(Documents::Runs,&document(json!({"id":"review","cardId":"task","kind":"pr-review","status":"completed"})))?;
        let mut input=PullRequestAttach{expected:card.clone(),revision:1,kind:ForgeKind::Github,repository:"owner/repo".into(),number:7,confirmed:true};
        let mut unconfirmed=input.clone();unconfirmed.confirmed=false;assert!(workflow::attach(store,&unconfirmed,pr()).is_err());
        for change in ["fork","branch","url"]{let mut invalid=pr();match change{"fork"=>invalid.fork=true,"branch"=>invalid.source_branch="wrong".into(),_=>invalid.url="https://untrusted.example.test/owner/repo/pull/7".into()};assert!(workflow::attach(store,&input,invalid).is_err());}
        let attached=workflow::attach(store,&input,pr())?;assert!(attached.fields.string_equals("status","review")?);assert!(attached.fields.string_equals("description","Keep description")?);assert!(attached.fields.get::<Vec<atelier_core::domain::checklist::Criterion>>("checklist")?.unwrap()[0].done);
        assert!(store.document::<Document>(Documents::Runs,"review")?.unwrap().raw("reviewStaleReason").is_some());assert!(attached.fields.get::<Document>("automation")?.unwrap().string_equals("state","interrupted")?);
        assert!(workflow::attach(store,&input,pr()).is_err());input.expected=attached;
        let mut review=store.document::<Document>(Documents::Runs,"review")?.unwrap();review.remove("reviewStaleReason");store.put(Documents::Runs,&review)?;
        let mut refreshed=pr();refreshed.checked_at="2026-10-25T11:00:00Z".into();input.expected=workflow::attach(store,&input,refreshed)?;assert!(store.document::<Document>(Documents::Runs,"review")?.unwrap().raw("reviewStaleReason").is_none());
        let mut advanced=pr();advanced.source_commit=Some("c".repeat(40));input.expected=workflow::attach(store,&input,advanced)?;assert!(store.document::<Document>(Documents::Runs,"review")?.unwrap().raw("reviewStaleReason").is_some());
        let mut different=input.clone();different.number=8;let mut other_pr=pr();other_pr.number=8;other_pr.url="https://github.example.test/owner/repo/pull/8".into();assert!(workflow::attach(store,&different,other_pr).is_err());
        let mut duplicate=card.clone();duplicate.id="other-task".into();duplicate.fields.set("pullRequest",&pr())?;store.put(Documents::Cards,&duplicate)?;assert!(workflow::attach(store,&input,pr()).is_err());store.delete(Documents::Cards,"other-task")?;
        store.put(Documents::Runs,&document(json!({"id":"busy","cardId":"task","status":"running"})))?;assert!(workflow::detach(store,PullRequestDetach{expected:input.expected.clone(),confirmed:true}).is_err());store.delete(Documents::Runs,"busy")?;
        assert!(workflow::detach(store,PullRequestDetach{expected:input.expected.clone(),confirmed:false}).is_err());
        let detached=workflow::detach(store,PullRequestDetach{expected:input.expected,confirmed:true})?;assert!(detached.fields.raw("pullRequest").is_none());assert!(detached.fields.raw("worktree").is_some());assert!(detached.fields.string_equals("instructions","Keep instructions")?);Ok(())
    }).await.unwrap();
    database.close().await.unwrap();
}

#[cfg(windows)]
#[tokio::test]
async fn native_association_verification_refuses_origin_or_branch_changes_without_fetching() {
    use atelier_app::infrastructure::{process_tree, worktrees};
    use cap_std::{ambient_authority, fs::Dir};
    async fn git(path: &std::path::Path, args: &[&str]) {
        let mut command = tokio::process::Command::new("git");
        command
            .current_dir(path)
            .env("GIT_CONFIG_GLOBAL", "NUL")
            .env("GIT_CONFIG_SYSTEM", "NUL")
            .args([
                "-c",
                "core.fsmonitor=false",
                "-c",
                "core.hooksPath=NUL",
                "-c",
                "user.name=Synthetic",
                "-c",
                "user.email=synthetic@example.invalid",
            ])
            .args(args);
        let output = process_tree::capture(
            &mut command,
            &[],
            std::time::Duration::from_secs(15),
            1024 * 1024,
        )
        .await
        .unwrap();
        assert!(output.status.success());
    }
    let temporary = tempfile::tempdir().unwrap();
    let path = std::fs::canonicalize(temporary.path()).unwrap();
    let source = path.join("repositories/source");
    std::fs::create_dir_all(&source).unwrap();
    git(&source, &["init", "-b", "main"]).await;
    git(
        &source,
        &[
            "commit",
            "--allow-empty",
            "-qm",
            "Synthetic association baseline",
        ],
    )
    .await;
    git(
        &source,
        &[
            "config",
            "remote.origin.url",
            "https://github.example.test/owner/repo.git",
        ],
    )
    .await;
    let root = Dir::open_ambient_dir(&path, ambient_authority()).unwrap();
    let roots = [path];
    let mut worktree = worktrees::prepare(
        &root,
        &roots,
        source.to_str().unwrap(),
        "task/synthetic".into(),
        "main".into(),
    )
    .await
    .unwrap();
    worktrees::create(&root, &roots, &worktree).await.unwrap();
    worktree.state = atelier_core::contracts::WorktreeState::Ready;
    let card: Card = serde_json::from_str(
        &json!({"id":"task","title":"Synthetic","repo":source,"worktree":worktree}).to_string(),
    )
    .unwrap();
    let configuration = configuration(ForgeKind::Github, "https://github.example.test");
    // canonicalize retains Windows' extended prefix; generated worktree paths use its plain spelling.
    workflow::verify(&root, &roots, &card, &configuration, "owner/repo")
        .await
        .unwrap();
    let mut foreign = card.clone();
    foreign
        .fields
        .set("repo", &source.with_file_name("other-repository"))
        .unwrap();
    assert!(
        workflow::verify(&root, &roots, &foreign, &configuration, "owner/repo")
            .await
            .is_err()
    );
    git(
        &source,
        &[
            "config",
            "remote.origin.url",
            "https://other.example.test/owner/repo.git",
        ],
    )
    .await;
    assert!(
        workflow::verify(&root, &roots, &card, &configuration, "owner/repo")
            .await
            .is_err()
    );
    git(
        &source,
        &[
            "config",
            "remote.origin.url",
            "https://github.example.test/owner/repo.git",
        ],
    )
    .await;
    git(
        std::path::Path::new(&worktree.path),
        &["checkout", "-b", "other-branch"],
    )
    .await;
    assert!(
        workflow::verify(&root, &roots, &card, &configuration, "owner/repo")
            .await
            .is_err()
    );
}
