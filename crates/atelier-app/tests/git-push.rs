#![cfg(windows)]
use atelier_app::infrastructure::{
    files::Repository, git_transport::HttpsAuthentication, process_tree,
};
use atelier_core::{
    contracts::{ForgeConfiguration, ForgeKind},
    document::JsonString,
};
use axum::{
    Router,
    extract::{Request, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use cap_std::{ambient_authority, fs::Dir};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

async fn git(directory: &Path, args: &[&str]) -> Vec<u8> {
    let mut command = tokio::process::Command::new("git");
    command
        .current_dir(directory)
        .env("GIT_CONFIG_GLOBAL", "NUL")
        .env("GIT_CONFIG_SYSTEM", "NUL")
        .args([
            "-c",
            "core.hooksPath=NUL",
            "-c",
            "user.name=Synthetic",
            "-c",
            "user.email=synthetic@example.invalid",
        ])
        .args(args);
    let output = process_tree::capture(&mut command, &[], Duration::from_secs(15), 8 * 1024 * 1024)
        .await
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

#[tokio::test]
async fn durable_push_reservations_recover_without_replay_and_protect_backup_history() {
    use atelier_app::infrastructure::{
        backups::BackupDirectory, database::Database, forge_draft, git_push as workflow,
        store::Documents,
    };
    use atelier_core::{
        contracts::{Card, ForgeWriteState as Phase, GitPushIntent, GitPushRequest},
        document::Document,
    };
    use serde_json::json;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().to_owned();
    let database = Database::open(Path::new(":memory:")).unwrap();
    database.call(move|store|{
        let settings:Document=serde_json::from_str(&json!({"forgeRevision":1,"forgeConnections":{"github":{"kind":"github","base":"https://github.example.test"}}}).to_string())?;store.save_settings(&settings)?;
        let worktree=json!({"path":"synthetic/worktree","source":"synthetic/source","branch":"task","base":"main","baseCommit":"a".repeat(40),"state":"ready"});
        let pr=json!({"revision":1,"kind":"github","repository":"owner/repo","number":7,"title":"Synthetic title","url":"https://github.example.test/owner/repo/pull/7","status":"active","draft":true,"sourceBranch":"task","targetBranch":"main","sourceCommit":"a".repeat(40),"targetCommit":"b".repeat(40),"fork":false,"baseUrl":"https://github.example.test","checkedAt":"2026-10-25T10:00:00Z"});
        let card:Card=serde_json::from_str(&json!({"id":"task","title":"Human title","description":"Human text","repo":"synthetic/source","worktree":worktree,"pullRequest":pr,"gitPushPreview":{"configuration":{"kind":"github","base":"https://github.example.test"},"revision":1,"repository":"owner/repo","worktree":worktree,"pullRequest":pr,"origin":"https://github.example.test/owner/repo.git","commit":"c".repeat(40),"previous":"a".repeat(40),"localChanges":true}}).to_string())?;
        store.put(Documents::Cards,&card)?;
        let backups=BackupDirectory::open(&path)?;let old=backups.create(store)?;
        let mut input=GitPushRequest{expected:card,revision:1,confirmed:false};let id=uuid::Uuid::new_v4().to_string();
        assert!(workflow::reserve(store,&input,&id).is_err());input.confirmed=true;
        let reserved=workflow::reserve(store,&input,&id)?;assert!(atelier_core::application::forge::unresolved(&reserved.fields)?);
        assert!(backups.restore(store,&old.name).is_err());assert!(workflow::reserve(store,&input,&id).is_err());
        forge_draft::recover(store)?;let failed=store.document::<Card>(Documents::Cards,"task")?.unwrap();assert_eq!(failed.fields.get::<GitPushIntent>("gitPush")?.unwrap().state,Phase::Failed);
        input.expected=failed;let reserved=workflow::reserve(store,&input,&id)?;workflow::sending(store,&reserved,&id)?;forge_draft::recover(store)?;
        let uncertain=store.document::<Card>(Documents::Cards,"task")?.unwrap();assert_eq!(uncertain.fields.get::<GitPushIntent>("gitPush")?.unwrap().state,Phase::Uncertain);
        assert!(forge_draft::blocks(store,"/api/cards/task/forge/detach")?);assert!(!forge_draft::blocks(store,"/api/cards/task/forge/push-reconcile")?);
        let unchanged=workflow::record(store,"task",&id,"a".repeat(40),1,true)?;assert_eq!(unchanged.fields.get::<GitPushIntent>("gitPush")?.unwrap().state,Phase::Uncertain);
        let resolved=workflow::record(store,"task",&id,"c".repeat(40),1,true)?;assert_eq!(resolved.fields.get::<GitPushIntent>("gitPush")?.unwrap().state,Phase::Reconciled);assert!(resolved.fields.raw("gitPushPreview").is_none());assert!(resolved.fields.raw("gitPushEffect").is_some());
        assert!(resolved.fields.string_equals("description","Human text")?);assert!(!atelier_core::application::forge::unresolved(&resolved.fields)?);assert!(backups.restore(store,&old.name).is_err());assert!(workflow::sending(store,&reserved,&id).is_err());
        let mut competing=resolved.clone();competing.id="other-task".into();
        let mut competing_intent=competing.fields.get::<GitPushIntent>("gitPush")?.unwrap();competing_intent.state=Phase::Uncertain;competing_intent.observed=None;competing.fields.set("gitPush",&competing_intent)?;store.put(Documents::Cards,&competing)?;
        let observed=workflow::record(store,"other-task",&id,"d".repeat(40),1,true)?;
        let intent=observed.fields.get::<GitPushIntent>("gitPush")?.unwrap();assert_eq!(intent.state,Phase::Reconciled);assert_ne!(intent.observed.as_deref(),Some(intent.preview.commit.as_str()));
        Ok(())
    }).await.unwrap();
    database.close().await.unwrap();
}
fn path(path: &Path) -> String {
    path.to_string_lossy()
        .trim_start_matches(r"\\?\")
        .replace('\\', "/")
}
#[derive(Clone)]
struct Remote {
    root: PathBuf,
    writes: Arc<AtomicUsize>,
    lose_response: Arc<AtomicBool>,
}
async fn serve(State(remote): State<Remote>, request: Request) -> Response {
    use base64::{Engine, engine::general_purpose::STANDARD};
    assert_eq!(
        request.headers()[header::AUTHORIZATION].to_str().unwrap(),
        format!("Basic {}", STANDARD.encode("atelier:synthetic-push-token"))
    );
    let uri = request.uri().clone();
    let method = request.method().clone();
    assert!(uri.path().starts_with("/owner/repo.git/"));
    assert!(!uri.path().contains(".."));
    let content_type = request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_owned();
    let body = axum::body::to_bytes(request.into_body(), 8 * 1024 * 1024)
        .await
        .unwrap();
    let mut command = tokio::process::Command::new("git");
    command.env_clear();
    for key in ["PATH", "SystemRoot", "WINDIR"] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    command
        .current_dir(&remote.root)
        .env("HOME", &remote.root)
        .env("USERPROFILE", &remote.root)
        .env("GIT_CONFIG_GLOBAL", "NUL")
        .env("GIT_CONFIG_SYSTEM", "NUL")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_PROJECT_ROOT", &remote.root)
        .env("GIT_HTTP_EXPORT_ALL", "1")
        .env("REMOTE_USER", "synthetic")
        .env("REQUEST_METHOD", method.as_str())
        .env("PATH_INFO", uri.path())
        .env("QUERY_STRING", uri.query().unwrap_or(""))
        .env("CONTENT_TYPE", content_type)
        .env("CONTENT_LENGTH", body.len().to_string())
        .args(["http-backend"]);
    let output = process_tree::capture(
        &mut command,
        &body,
        Duration::from_secs(15),
        8 * 1024 * 1024,
    )
    .await
    .unwrap();
    assert!(output.status.success());
    if method == axum::http::Method::POST {
        remote.writes.fetch_add(1, Ordering::SeqCst);
        if remote.lose_response.load(Ordering::SeqCst) {
            return (StatusCode::BAD_GATEWAY, "synthetic-push-token").into_response();
        }
    }
    let split = output
        .stdout
        .windows(4)
        .position(|part| part == b"\r\n\r\n")
        .map(|index| (index, 4))
        .or_else(|| {
            output
                .stdout
                .windows(2)
                .position(|part| part == b"\n\n")
                .map(|index| (index, 2))
        })
        .unwrap();
    let headers = String::from_utf8_lossy(&output.stdout[..split.0]);
    let mut response = Response::builder();
    for line in headers.lines() {
        if let Some((key, value)) = line.trim_end_matches('\r').split_once(':') {
            if key.eq_ignore_ascii_case("content-type") {
                response = response.header(header::CONTENT_TYPE, value.trim());
            }
            if key.eq_ignore_ascii_case("status") {
                response = response.status(
                    value
                        .trim()
                        .split(' ')
                        .next()
                        .unwrap()
                        .parse::<u16>()
                        .unwrap(),
                );
            }
        }
    }
    response
        .body(axum::body::Body::from(
            output.stdout[split.0 + split.1..].to_vec(),
        ))
        .unwrap()
}
#[tokio::test]
async fn isolated_push_preserves_human_state_rejects_remote_races_and_observes_lost_responses() {
    let fixture = tempfile::tempdir().unwrap();
    let root_path = std::fs::canonicalize(fixture.path()).unwrap();
    let source_path = root_path.join("repositories/source");
    let remote_root = root_path.join("remote");
    std::fs::create_dir_all(&source_path).unwrap();
    std::fs::create_dir_all(remote_root.join("owner")).unwrap();
    git(&source_path, &["init", "-b", "task"]).await;
    std::fs::write(source_path.join("file.txt"), "initial").unwrap();
    git(&source_path, &["add", "."]).await;
    git(&source_path, &["commit", "-qm", "Synthetic initial"]).await;
    let initial = String::from_utf8(git(&source_path, &["rev-parse", "HEAD"]).await)
        .unwrap()
        .trim()
        .to_owned();
    git(
        &remote_root,
        &[
            "clone",
            "--bare",
            "--no-hardlinks",
            &path(&source_path),
            "owner/repo.git",
        ],
    )
    .await;
    let bare = remote_root.join("owner/repo.git");
    git(&bare, &["config", "http.receivepack", "true"]).await;
    std::fs::write(source_path.join("file.txt"), "second").unwrap();
    git(&source_path, &["add", "."]).await;
    git(&source_path, &["commit", "-qm", "Synthetic second"]).await;
    let second = String::from_utf8(git(&source_path, &["rev-parse", "HEAD"]).await)
        .unwrap()
        .trim()
        .to_owned();
    let remote = Remote {
        root: remote_root,
        writes: Arc::new(AtomicUsize::new(0)),
        lose_response: Arc::new(AtomicBool::new(false)),
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let app = Router::new()
        .fallback(axum::routing::any(serve))
        .with_state(remote.clone());
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let origin = format!("{base}/owner/repo.git");
    git(&source_path, &["remote", "add", "origin", &origin]).await;
    std::fs::write(
        source_path.join(".git/hooks/pre-push"),
        "#!/bin/sh\nexit 99\n",
    )
    .unwrap();
    std::fs::write(source_path.join("file.txt"), "human staged").unwrap();
    git(&source_path, &["add", "file.txt"]).await;
    std::fs::write(source_path.join("file.txt"), "human unstaged").unwrap();
    std::fs::write(source_path.join("untracked.txt"), "human untracked").unwrap();
    std::fs::write(
        source_path.join(".git/FETCH_HEAD"),
        "synthetic fetch sentinel",
    )
    .unwrap();
    let index = std::fs::read(source_path.join(".git/index")).unwrap();
    let config = std::fs::read(source_path.join(".git/config")).unwrap();
    let refs = git(&source_path, &["show-ref"]).await;
    let root = Dir::open_ambient_dir(&root_path, ambient_authority()).unwrap();
    let roots = vec![root_path.clone()];
    let source = Repository::synthetic(
        &root,
        &roots,
        &JsonString::from(path(&source_path).as_str()),
    )
    .unwrap();
    let auth = HttpsAuthentication::synthetic(
        ForgeConfiguration {
            kind: ForgeKind::Github,
            base,
        },
        "owner/repo".into(),
        "synthetic-push-token".into(),
    )
    .unwrap();
    let prepared = auth
        .prepare_push(&source, &origin, "refs/heads/task", &second, Some(&initial))
        .await
        .unwrap();
    assert_eq!(remote.writes.load(Ordering::SeqCst), 0);
    // A human/server update between preflight and send must not be overwritten by the lease.
    let tree = String::from_utf8(git(&bare, &["rev-parse", "HEAD^{tree}"]).await)
        .unwrap()
        .trim()
        .to_owned();
    let competing = String::from_utf8(
        git(
            &bare,
            &[
                "commit-tree",
                &tree,
                "-p",
                &initial,
                "-m",
                "Synthetic competing commit",
            ],
        )
        .await,
    )
    .unwrap()
    .trim()
    .to_owned();
    git(
        &bare,
        &["update-ref", "refs/heads/task", &competing, &initial],
    )
    .await;
    assert!(auth.push(prepared).await.is_err());
    assert_eq!(
        auth.remote_head(&origin, "refs/heads/task").await.unwrap(),
        competing
    );
    // Reset only the disposable synthetic remote to exercise a real accepted push with a lost response.
    git(
        &bare,
        &["update-ref", "refs/heads/task", &initial, &competing],
    )
    .await;
    let prepared = auth
        .prepare_push(&source, &origin, "refs/heads/task", &second, Some(&initial))
        .await
        .unwrap();
    remote.lose_response.store(true, Ordering::SeqCst);
    let before = remote.writes.load(Ordering::SeqCst);
    let error = auth.push(prepared).await.err().unwrap().to_string();
    assert!(!error.contains("synthetic-push-token"));
    assert_eq!(remote.writes.load(Ordering::SeqCst), before + 1);
    assert_eq!(
        auth.remote_head(&origin, "refs/heads/task").await.unwrap(),
        second
    );
    assert_eq!(remote.writes.load(Ordering::SeqCst), before + 1);
    git(&bare, &["update-ref", "refs/heads/task", &initial, &second]).await;
    remote.lose_response.store(false, Ordering::SeqCst);
    let prepared = auth
        .prepare_push(&source, &origin, "refs/heads/task", &second, Some(&initial))
        .await
        .unwrap();
    auth.push(prepared).await.unwrap();
    assert!(
        auth.remote_head_optional(&origin, "refs/heads/first")
            .await
            .unwrap()
            .is_none()
    );
    let absent = auth
        .prepare_push(&source, &origin, "refs/heads/first", &second, None)
        .await
        .unwrap();
    git(&bare, &["update-ref", "refs/heads/first", &competing]).await;
    assert!(auth.push(absent).await.is_err());
    assert_eq!(
        auth.remote_head(&origin, "refs/heads/first").await.unwrap(),
        competing
    );
    // Delete only the competing disposable fixture ref, then exercise creation under an empty lease.
    git(&bare, &["update-ref", "-d", "refs/heads/first", &competing]).await;
    let absent = auth
        .prepare_push(&source, &origin, "refs/heads/first", &second, None)
        .await
        .unwrap();
    auth.push(absent).await.unwrap();
    assert_eq!(
        auth.remote_head(&origin, "refs/heads/first").await.unwrap(),
        second
    );
    assert_eq!(
        std::fs::read(source_path.join(".git/index")).unwrap(),
        index
    );
    assert_eq!(
        std::fs::read(source_path.join(".git/config")).unwrap(),
        config
    );
    assert_eq!(git(&source_path, &["show-ref"]).await, refs);
    assert_eq!(
        std::fs::read_to_string(source_path.join(".git/FETCH_HEAD")).unwrap(),
        "synthetic fetch sentinel"
    );
    assert_eq!(
        std::fs::read_to_string(source_path.join("file.txt")).unwrap(),
        "human unstaged"
    );
    assert_eq!(
        std::fs::read_to_string(source_path.join("untracked.txt")).unwrap(),
        "human untracked"
    );
    assert!(
        auth.prepare_push(&source, &origin, "refs/heads/task", &initial, Some(&second))
            .await
            .is_err()
    );
    server.abort();
    let _ = server.await;
}
