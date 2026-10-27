use atelier_app::infrastructure::{
    automation,
    command_runs::{CommandRuns, RunDirectory, RunInput},
    connector_effects as effects,
    connector_repositories::{self as repositories, Guard, Selection},
    connectors,
    database::Database,
    extensions::Executable,
    process_tree,
    store::Documents,
    worktrees,
};
use atelier_core::{
    application::{
        automation::{PublicationPlan, State},
        extensions::{
            self as protocol, Installed, Invocation, Method, RemotePreview, Report,
            RepositoryProposal, RepositoryReviewContext, Request,
        },
    },
    contracts::{Card, ForgeWriteState, Worktree, WorktreeState},
    document::Document,
};
use cap_std::{ambient_authority, fs::Dir};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
fn doc(value: Value) -> Document {
    serde_json::from_str(&value.to_string()).unwrap()
}
async fn git(path: &Path, args: &[&str]) -> String {
    let mut command = tokio::process::Command::new("git");
    command.env_clear().current_dir(path);
    for key in ["PATH", "SystemRoot", "WINDIR"] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    let null = if cfg!(windows) { "NUL" } else { "/dev/null" };
    command
        .env("HOME", path)
        .env("USERPROFILE", path)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", null)
        .env("GIT_CONFIG_SYSTEM", null)
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "user.name=Synthetic",
            "-c",
            "user.email=synthetic@example.invalid",
        ])
        .args(args);
    let result = process_tree::capture(&mut command, &[], Duration::from_secs(20), 1024 * 1024)
        .await
        .unwrap();
    assert!(result.status.success());
    String::from_utf8(result.stdout).unwrap().trim().into()
}
async fn card(db: &Database) -> Card {
    db.call(|store| Ok(store.document(Documents::Cards, "task")?.unwrap()))
        .await
        .unwrap()
}
async fn dispatch(profile: &Installed, request: Request) -> atelier_app::Result<protocol::Reply> {
    Executable::open(
        PathBuf::from(&profile.binary.path),
        Some(profile.binary.sha256.clone()),
    )
    .await?
    .prepare(&profile.manifest, request, Duration::from_secs(20))?
    .dispatch()
    .await
}
async fn observe(db: &Database, root: &Dir, base: &Path, method: Method) -> RepositoryProposal {
    let expected = card(db).await;
    let invocation = Invocation {
        expected_revision: 1,
        method,
        action: None,
        input: json!({"expected":expected,"target":"main","title":"Synthetic publication","body":"Keep human criteria","draft":true}),
        confirmed: true,
    };
    let (profile, mut request) = db
        .call(move |store| {
            connectors::invocation(
                store,
                "synthetic-extension",
                &invocation,
                &mut |value, _| Ok(value.into()),
            )
        })
        .await
        .unwrap();
    let selection: Selection = serde_json::from_str(&request.input.to_string()).unwrap();
    let guard = Guard::before(root, &[base.to_owned()], profile.clone(), selection)
        .await
        .unwrap();
    request.input = guard.input().unwrap();
    let reply = dispatch(&profile, request).await.unwrap();
    guard
        .observe(&reply.output, method == Method::RepositoriesReview)
        .await
        .unwrap()
}
#[allow(clippy::too_many_arguments)]
async fn stage(
    db: &Arc<Database>,
    manager: &Arc<CommandRuns>,
    root: &Dir,
    base: &Path,
    worktree: &Worktree,
    operation: &str,
    id: &str,
    verification: bool,
    context: Option<&RepositoryReviewContext>,
) {
    let claimed = operation.to_owned();
    let selected = id.to_owned();
    let (expected, _, _) = db
        .call(move |store| automation::claim(store, "task", &claimed, selected))
        .await
        .unwrap();
    let repository = worktrees::inspect(root, &[base.to_owned()], worktree)
        .await
        .unwrap();
    let mut record = doc(
        json!({"id":id,"cardId":"task","automationId":operation,"repo":worktree.path,"agentType":"command","status":"starting","purpose":if verification{"verification"}else if context.is_some(){"review"}else{"work"}}),
    );
    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_native-fixture"));
    command.env_clear().current_dir(&worktree.path);
    for key in ["SystemRoot", "WINDIR"] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    let input = if verification {
        record
            .set("verificationCommand", &"synthetic-verification")
            .unwrap();
        record.set("verificationWorktree", worktree).unwrap();
        record.set("verificationState", &"running").unwrap();
        record
            .set(
                "sourceFingerprint",
                &repository.fingerprint().await.unwrap(),
            )
            .unwrap();
        record
            .set(
                "requirements",
                &atelier_core::application::preparation::requirements(&expected).unwrap(),
            )
            .unwrap();
        command.args(["verify", "success"]);
        RunInput::Verification
    } else {
        command.arg("review-json");
        if let Some(context) = context {
            record.set("kind", &"pr-review").unwrap();
            record.set("connectorReviewContext", context).unwrap();
            record
                .set(
                    "reviewStaleReason",
                    &atelier_app::infrastructure::reviews::UNCHECKED,
                )
                .unwrap();
        }
        RunInput::Command(String::new())
    };
    manager
        .start(
            db.clone(),
            record,
            command,
            input,
            RunDirectory::Worktree(repository),
        )
        .await
        .unwrap();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    while !manager.active_cards().is_empty() {
        assert!(tokio::time::Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}
#[tokio::test]
async fn compiled_repository_module_publication_recovers_once_and_reviews_real_commits_in_the_workflow()
 {
    let temp = tempfile::tempdir().unwrap();
    let base = PathBuf::from(
        temp.path()
            .canonicalize()
            .unwrap()
            .to_str()
            .unwrap()
            .trim_start_matches(r"\\?\")
            .replace('\\', "/"),
    );
    let repo = base.join("repositories/source");
    let work = base.join("repositories/atelier-worktrees/00000000-0000-4000-8000-000000000003");
    std::fs::create_dir_all(repo.join("src")).unwrap();
    std::fs::create_dir_all(work.parent().unwrap()).unwrap();
    std::fs::write(
        base.join(".extension-fixture"),
        "atelier-r04-synthetic-only",
    )
    .unwrap();
    git(&repo, &["init", "-b", "main"]).await;
    std::fs::write(
        repo.join("src/lib.rs"),
        "pub fn synthetic_answer() -> u8 { 0 }\n",
    )
    .unwrap();
    std::fs::write(
        repo.join(".review-fixture"),
        "atelier-review-synthetic-only",
    )
    .unwrap();
    std::fs::write(repo.join(".run-fixture"), "atelier-r03-synthetic-only").unwrap();
    git(&repo, &["add", "."]).await;
    git(&repo, &["commit", "-qm", "Synthetic target"]).await;
    let target = git(&repo, &["rev-parse", "HEAD"]).await;
    git(
        &repo,
        &["worktree", "add", "-b", "task", work.to_str().unwrap()],
    )
    .await;
    std::fs::write(
        work.join("src/lib.rs"),
        "pub fn synthetic_answer() -> u8 { 42 }\n",
    )
    .unwrap();
    git(&work, &["add", "."]).await;
    git(&work, &["commit", "-qm", "Synthetic source"]).await;
    let source = git(&work, &["rev-parse", "HEAD"]).await;
    let origin = "https://synthetic.example.test/owner/repo.git";
    git(&repo, &["remote", "add", "origin", origin]).await;
    let observation = json!({"reference":{"instance":"https://synthetic.example.test","repository":"owner/repo","origin":origin},"targetBranch":"main","targetCommit":target,"remoteSource":source,"pullRequest":null});
    std::fs::write(
        base.join("repository-observation.json"),
        observation.to_string(),
    )
    .unwrap();
    let (binary, manifest) =
        Executable::open(PathBuf::from(env!("CARGO_BIN_EXE_native-fixture")), None)
            .await
            .unwrap()
            .describe("synthetic-extension")
            .await
            .unwrap();
    let installed=Installed{binary,manifest,revision:1,values:serde_json::from_value(json!({"fault":"repository-lost","fixture-root":base,"site":"https://synthetic.example.test"})).unwrap(),report:None};
    let root = Dir::open_ambient_dir(&base, ambient_authority()).unwrap();
    let worktree = Worktree {
        path: work.to_str().unwrap().into(),
        source: repo.to_str().unwrap().into(),
        branch: "task".into(),
        base: "main".into(),
        base_commit: target.clone(),
        state: WorktreeState::Ready,
    };
    let initial:Card=serde_json::from_str(&json!({"id":"task","title":"Human title","description":"Human content","repo":repo,"agent":"codex","worktree":worktree,"checklist":[{"text":"src/lib.rs returns 42","done":false}]}).to_string()).unwrap();
    let project = doc(
        json!({"id":"project","repo":repo,"automationEnabled":true,"autoFix":true,"testCommand":"synthetic-verification","prTarget":"main"}),
    );
    let db = Arc::new(Database::open(&base.join("synthetic.sqlite")).unwrap());
    let stored = installed.clone();
    let saved = initial.clone();
    let selected = project.clone();
    db.call(move |store| {
        store.save_settings(&doc(
            json!({"connectorRevision":1,"processConnectors":[stored]}),
        ))?;
        store.put(Documents::Cards, &saved)?;
        store.put(Documents::Projects, &selected)
    })
    .await
    .unwrap();
    let start = doc(
        json!({"expected":initial,"project":project,"profile":atelier_core::domain::agent_options::providers(&Document::default()).unwrap()[0],"action":"start","confirmed":true}),
    );
    db.call(move |store| automation::change(store, "task", &start))
        .await
        .unwrap();
    let operation = automation::read(&card(&db).await).unwrap().unwrap().id;
    let manager = CommandRuns::new(tokio::sync::broadcast::channel(16).0);
    for (id, verify) in [("development", false), ("verification", true)] {
        stage(
            &db, &manager, &root, &base, &worktree, &operation, id, verify, None,
        )
        .await;
        let op = operation.clone();
        db.call(move |store| automation::settle(store, "task", &op))
            .await
            .unwrap();
    }
    let proposal = observe(&db, &root, &base, Method::RepositoriesPreview).await;
    assert_eq!(proposal.context.source, source);
    assert_eq!(proposal.context.target, target);
    assert_eq!(proposal.context.base, target);
    let report = Report {
        id: "synthetic-repository-preview".into(),
        method: Method::RepositoriesPreview,
        action: None,
        message: String::new(),
        output: serde_json::to_value(&proposal).unwrap(),
        observed_at: time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap(),
    };
    db.call(move |store| connectors::record(store, "synthetic-extension", 1, report))
        .await
        .unwrap();
    let approve = doc(json!({"expected":card(&db).await,"action":"publish","confirmed":true}));
    db.call(move |store| automation::change(store, "task", &approve))
        .await
        .unwrap();
    stage(
        &db,
        &manager,
        &root,
        &base,
        &worktree,
        &operation,
        "publication-verification",
        true,
        None,
    )
    .await;
    let op = operation.clone();
    db.call(move |store| automation::settle(store, "task", &op))
        .await
        .unwrap();
    let selected = card(&db).await;
    let owner = operation.clone();
    let approved = proposal.clone();
    let effect = db
        .call(move |store| {
            let (_, _, plan) = automation::publication(store, "task", &owner)?;
            assert!(matches!(plan, PublicationPlan::Module(_)));
            effects::preview(
                store,
                "synthetic-extension",
                &RemotePreview {
                    expected_revision: 1,
                    method: Method::RepositoriesPublish,
                    action: String::new(),
                    input: json!({"expected":selected,"approved":approved,"automationId":owner}),
                },
            )
        })
        .await
        .unwrap();
    let effect_id = effect.id.clone();
    let selected = effect_id.clone();
    let (profile, request) = db
        .call(move |store| {
            effects::prepare(
                store,
                "synthetic-extension",
                &selected,
                1,
                1,
                false,
                &mut |value, _| Ok(value.into()),
            )
        })
        .await
        .unwrap();
    assert!(request.input.get("context").is_none());
    assert!(request.input.get("requirements").is_none());
    assert!(!request.input.to_string().contains("Human content"));
    let request_id = request.request_id.clone();
    let selected = effect_id.clone();
    db.call(move |store| {
        effects::sending(store, "synthetic-extension", &selected, 1, 1, &request_id)
    })
    .await
    .unwrap();
    assert!(dispatch(&profile, request).await.is_err());
    let selected = effect_id.clone();
    let uncertain = db
        .call(move |store| effects::interrupted(store, &selected, 2))
        .await
        .unwrap();
    assert!(uncertain.state == ForgeWriteState::Uncertain);
    let marker = base.join(format!("{effect_id}.repository-effect"));
    let modified = std::fs::metadata(&marker).unwrap().modified().unwrap();
    db.call(|store| {
        assert!(automation::running(store)?.is_none());
        Ok(())
    })
    .await
    .unwrap();
    let selected = effect_id.clone();
    let (profile, request) = db
        .call(move |store| {
            effects::prepare(
                store,
                "synthetic-extension",
                &selected,
                3,
                1,
                true,
                &mut |value, _| Ok(value.into()),
            )
        })
        .await
        .unwrap();
    let request_id = request.request_id.clone();
    let selected = effect_id.clone();
    db.call(move |store| {
        effects::observing(store, "synthetic-extension", &selected, 3, 1, &request_id)
    })
    .await
    .unwrap();
    let reply = dispatch(&profile, request).await.unwrap();
    let selected = effect_id.clone();
    let reconciled = db
        .call(move |store| {
            effects::record(store, "synthetic-extension", &selected, 4, 1, reply, true)
        })
        .await
        .unwrap();
    assert!(reconciled.state == ForgeWriteState::Reconciled);
    assert_eq!(
        std::fs::metadata(&marker).unwrap().modified().unwrap(),
        modified
    );
    let selected = card(&db).await;
    let op = operation.clone();
    db.call(move |store| automation::publication_complete(store, &selected, &op))
        .await
        .unwrap();
    let context = observe(&db, &root, &base, Method::RepositoriesReview)
        .await
        .context;
    stage(
        &db,
        &manager,
        &root,
        &base,
        &worktree,
        &operation,
        "review",
        false,
        Some(&context),
    )
    .await;
    let current = observe(&db, &root, &base, Method::RepositoriesReview)
        .await
        .context;
    assert_eq!(context, current);
    let selected = card(&db).await;
    let op = operation.clone();
    db.call(move |store| {
        repositories::validate_review(store, &selected, "review", &current, "synthetic-verified")?;
        automation::settle(store, "task", &op)
    })
    .await
    .unwrap();
    let retained = card(&db).await;
    assert!(automation::read(&retained).unwrap().unwrap().state == State::Ready);
    assert_eq!(retained.title.utf8().unwrap(), "Human title");
    assert_eq!(
        retained.fields.get::<Value>("checklist").unwrap(),
        initial.fields.get::<Value>("checklist").unwrap()
    );
    assert!(retained.fields.raw("pullRequest").is_none());
    // The module cannot turn its own worktree mutation into an accepted observation.
    let mut malicious = installed;
    malicious
        .values
        .insert("fault".into(), json!("repository-dirty"));
    let selection = Selection {
        expected: retained,
        target: "main".into(),
        title: "Synthetic".into(),
        body: String::new(),
        draft: true,
    };
    let guard = Guard::before(
        &root,
        std::slice::from_ref(&base),
        malicious.clone(),
        selection,
    )
    .await
    .unwrap();
    let request = Request {
        protocol: protocol::PROTOCOL.into(),
        request_id: "request-dirty".into(),
        module: "synthetic-extension".into(),
        method: Method::RepositoriesReview,
        operation_id: None,
        action: None,
        settings: malicious.values.clone(),
        secrets: Default::default(),
        input: guard.input().unwrap(),
    };
    let reply = dispatch(&malicious, request).await.unwrap();
    assert!(guard.observe(&reply.output, true).await.is_err());
    db.call(|store| {
        connectors::configure(
            store,
            "synthetic-extension",
            &protocol::Configuration {
                expected_revision: 1,
                values: serde_json::from_value(json!({"fault":"success"}))?,
                forget_values: Default::default(),
                secrets: Default::default(),
                forget_secrets: Default::default(),
            },
            &mut |value, _| Ok(value.into()),
        )?;
        let run = store
            .document::<Document>(Documents::Runs, "review")?
            .unwrap();
        assert!(run.truthy("reviewStaleReason")?);
        let card = store.document::<Card>(Documents::Cards, "task")?.unwrap();
        assert!(automation::read(&card)?.unwrap().state == State::Interrupted);
        Ok(())
    })
    .await
    .unwrap();
    manager.close().await.unwrap();
    db.close().await.unwrap();
}
