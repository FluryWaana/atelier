#[path = "../../crates/atelier-app/tests/support/tls.rs"]
mod tls;
use atelier_app::infrastructure::{
    automation,
    command_runs::{CommandRuns, RunDirectory, RunInput},
    database::Database,
    forge::Forge,
    forge_publication,
    git_transport::HttpsAuthentication,
    process_tree, reviews,
    store::Documents,
    worktrees,
};
use atelier_core::{
    application::{
        automation::{State as FlowState, Step},
        reviews::Context,
    },
    contracts::{
        Card, ForgeConfiguration, ForgeKind, ForgePublicationIntent, ForgePublicationRequest,
        ForgeWriteState, Worktree, WorktreeState,
    },
    document::Document,
};
use axum::{
    Json, Router,
    extract::{Request, State},
    http::{Method, StatusCode},
    response::{IntoResponse, Response},
};
use cap_std::{ambient_authority, fs::Dir};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

#[cfg(target_os = "macos")]
async fn verify_with_git_tls_library(certificates: &tls::Certificates) -> atelier_app::Result<()> {
    let executable = tls::openssl_executable()?;
    let root = certificates._directory.path();
    let empty = root.join("git-verify-empty-ca");
    std::fs::create_dir(&empty)?;
    let mut command = tokio::process::Command::new(executable);
    command
        .current_dir(root)
        .env_clear()
        .env("HOME", root)
        .env("OPENSSL_CONF", "/dev/null")
        .arg("verify")
        .arg("-CAfile")
        .arg(root.join("ca.pem"))
        .arg("-CApath")
        .arg(&empty)
        .args([
            "-no-CAstore",
            "-purpose",
            "sslserver",
            "-verify_ip",
            "127.0.0.1",
        ])
        .arg(root.join("server.pem"));
    let output =
        process_tree::capture(&mut command, &[], Duration::from_secs(10), 64 * 1024).await?;
    if !output.status.success() {
        let diagnostic = String::from_utf8_lossy(&output.stderr);
        let code = diagnostic.lines().find_map(|line| {
            let value = line
                .trim()
                .strip_prefix("error ")?
                .split_whitespace()
                .next()?;
            value.parse::<u16>().ok()
        });
        return Err(format!(
            "Synthetic Git TLS: Homebrew OpenSSL verification failed (certificate code {code:?})."
        )
        .into());
    }
    eprintln!(
        "Synthetic Git TLS: Homebrew OpenSSL verified the explicit CA chain, server purpose and IP."
    );
    Ok(())
}

fn doc(value: Value) -> Document {
    serde_json::from_str(&value.to_string()).unwrap()
}
fn null() -> &'static str {
    if cfg!(windows) { "NUL" } else { "/dev/null" }
}
fn git_process(path: &Path) -> tokio::process::Command {
    let mut command = tokio::process::Command::new("git");
    command.env_clear();
    for key in ["PATH", "SystemRoot", "WINDIR"] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    command
        .current_dir(path)
        .env("HOME", path)
        .env("USERPROFILE", path)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", null())
        .env("GIT_CONFIG_SYSTEM", null())
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "user.name=Synthetic",
            "-c",
            "user.email=synthetic@example.invalid",
        ]);
    command
}
async fn git(path: &Path, args: &[&str]) -> String {
    let mut command = git_process(path);
    command.args(args);
    let output = process_tree::capture(&mut command, &[], Duration::from_secs(20), 8 * 1024 * 1024)
        .await
        .unwrap();
    assert!(
        output.status.success(),
        "Synthetic Git step failed: {args:?}"
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}
#[derive(Clone)]
struct Remote {
    root: PathBuf,
    source: String,
    target: String,
    body: Arc<Mutex<Option<String>>>,
    writes: Arc<AtomicUsize>,
    git_reads: Arc<AtomicUsize>,
}
impl Remote {
    fn row(&self) -> Value {
        json!({"number":7,"title":"Synthetic publication","body":self.body.lock().unwrap().clone().unwrap_or_default(),"draft":true,"state":"open","merged":false,"head":{"ref":"task","sha":self.source,"repo":{"full_name":"owner/repo"}},"base":{"ref":"main","sha":self.target,"repo":{"full_name":"owner/repo"}}})
    }
}
async fn remote(State(state): State<Remote>, request: Request) -> Response {
    let uri = request.uri().clone();
    let method = request.method().clone();
    assert!(!request.headers().contains_key("cookie"));
    if uri.path().starts_with("/owner/repo.git/") {
        // Basic atelier:synthetic-forge-token, generated without passing a token to a child.
        assert_eq!(
            request.headers()["authorization"],
            "Basic YXRlbGllcjpzeW50aGV0aWMtZm9yZ2UtdG9rZW4="
        );
        assert!(
            uri.path() == "/owner/repo.git/info/refs"
                || uri.path() == "/owner/repo.git/git-upload-pack"
        );
        state.git_reads.fetch_add(1, Ordering::SeqCst);
        let content_type = request
            .headers()
            .get("content-type")
            .and_then(|h| h.to_str().ok())
            .unwrap_or("")
            .to_owned();
        let body = axum::body::to_bytes(request.into_body(), 8 * 1024 * 1024)
            .await
            .unwrap();
        let mut command = git_process(&state.root);
        command
            .env("GIT_PROJECT_ROOT", &state.root)
            .env("GIT_HTTP_EXPORT_ALL", "1")
            .env("REMOTE_USER", "synthetic")
            .env("REQUEST_METHOD", method.as_str())
            .env("PATH_INFO", uri.path())
            .env("QUERY_STRING", uri.query().unwrap_or(""))
            .env("CONTENT_TYPE", content_type)
            .env("CONTENT_LENGTH", body.len().to_string())
            .arg("http-backend");
        let output = process_tree::capture(
            &mut command,
            &body,
            Duration::from_secs(20),
            8 * 1024 * 1024,
        )
        .await
        .unwrap();
        assert!(output.status.success());
        let (index, width) = output
            .stdout
            .windows(4)
            .position(|b| b == b"\r\n\r\n")
            .map(|i| (i, 4))
            .or_else(|| {
                output
                    .stdout
                    .windows(2)
                    .position(|b| b == b"\n\n")
                    .map(|i| (i, 2))
            })
            .unwrap();
        let mut response = Response::builder();
        for line in String::from_utf8_lossy(&output.stdout[..index]).lines() {
            if let Some((key, value)) = line.split_once(':') {
                if key.eq_ignore_ascii_case("content-type") {
                    response = response.header("content-type", value.trim());
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
        return response
            .body(axum::body::Body::from(
                output.stdout[index + width..].to_vec(),
            ))
            .unwrap();
    }
    assert_eq!(
        request.headers()["authorization"],
        "Bearer synthetic-forge-token"
    );
    match (method, uri.path()) {
        (Method::POST, "/api/v3/repos/owner/repo/pulls") => {
            assert_eq!(
                state.writes.fetch_add(1, Ordering::SeqCst),
                0,
                "A lost response must never cause replay"
            );
            let body = axum::body::to_bytes(request.into_body(), 32768)
                .await
                .unwrap();
            let body: Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(body["head"], "task");
            assert_eq!(body["base"], "main");
            assert_eq!(body["draft"], true);
            assert_eq!(body["maintainer_can_modify"], false);
            *state.body.lock().unwrap() = Some(body["body"].as_str().unwrap().to_owned());
            // Remote effect exists, but its identity cannot be read from this failed response.
            (
                StatusCode::BAD_GATEWAY,
                "Synthetic response lost after creation",
            )
                .into_response()
        }
        (Method::GET, "/api/v3/repos/owner/repo/pulls") => {
            let url = reqwest::Url::parse(&format!("https://127.0.0.1{uri}")).unwrap();
            let first = url.query_pairs().any(|(k, v)| k == "page" && v == "1");
            let exists = state.body.lock().unwrap().is_some();
            Json(if first && exists {
                json!([state.row()])
            } else {
                json!([])
            })
            .into_response()
        }
        (Method::GET, "/api/v3/repos/owner/repo/pulls/7") => Json(state.row()).into_response(),
        _ => panic!("Unexpected synthetic forge request: {uri}"),
    }
}

async fn card(database: &Database) -> Card {
    database
        .call(|store| Ok(store.document(Documents::Cards, "task")?.unwrap()))
        .await
        .unwrap()
}
#[allow(clippy::too_many_arguments)]
async fn stage(
    database: &Arc<Database>,
    manager: &Arc<CommandRuns>,
    root: &Dir,
    base: &Path,
    worktree: &Worktree,
    operation: &str,
    id: &str,
    verification: bool,
    context: Option<&Context>,
) {
    let operation = operation.to_owned();
    let claimed = operation.clone();
    let selected = id.to_owned();
    let (card, _, _) = database
        .call(move |store| automation::claim(store, "task", &claimed, selected))
        .await
        .unwrap();
    let repository = worktrees::inspect(root, &[base.to_owned()], worktree)
        .await
        .unwrap();
    let mut record = doc(
        json!({"id":id,"cardId":"task","automationId":operation,"repo":worktree.path,"status":"starting","purpose":if verification{"verification"}else if context.is_some(){"review"}else{"work"},"agentType":"command"}),
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
                &atelier_core::application::preparation::requirements(&card).unwrap(),
            )
            .unwrap();
        command.args(["verify", "success"]);
        RunInput::Verification
    } else {
        command.arg("review-json");
        if let Some(context) = context {
            record.set("kind", &"pr-review").unwrap();
            record.set("reviewContext", context).unwrap();
            record
                .set("reviewStaleReason", &reviews::UNCHECKED)
                .unwrap();
        }
        RunInput::Command(String::new())
    };
    manager
        .start(
            database.clone(),
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
#[ignore = "requires the isolated final CI campaign and native TLS tools"]
async fn publication_recovery_real_commit_review_and_automation_keep_human_criteria() {
    tls::require_final();
    let certificates = tls::Certificates::new().await.unwrap();
    #[cfg(target_os = "macos")]
    verify_with_git_tls_library(&certificates).await.unwrap();
    let temporary = tempfile::tempdir().unwrap();
    let base = PathBuf::from(
        temporary
            .path()
            .canonicalize()
            .unwrap()
            .to_str()
            .unwrap()
            .trim_start_matches(r"\\?\")
            .replace('\\', "/"),
    );
    let repo = base.join("repositories/source");
    let work = base.join("repositories/atelier-worktrees/00000000-0000-4000-8000-000000000001");
    let remote_root = base.join("remote");
    std::fs::create_dir_all(repo.join("src")).unwrap();
    std::fs::create_dir_all(work.parent().unwrap()).unwrap();
    std::fs::create_dir_all(remote_root.join("owner")).unwrap();
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
    git(
        &remote_root,
        &["clone", "--bare", repo.to_str().unwrap(), "owner/repo.git"],
    )
    .await;
    let state = Remote {
        root: remote_root,
        source: source.clone(),
        target: target.clone(),
        body: Arc::default(),
        writes: Arc::default(),
        git_reads: Arc::default(),
    };
    let server = tls::Server::start(
        certificates.valid.clone(),
        Router::new().fallback(remote).with_state(state.clone()),
    )
    .await
    .unwrap();
    let configuration = ForgeConfiguration {
        kind: ForgeKind::Github,
        base: server.base.clone(),
    };
    let foreign = Forge::synthetic_tls(
        configuration.clone(),
        "synthetic-forge-token".into(),
        1,
        &certificates.foreign,
    )
    .unwrap();
    assert!(foreign.pull_request("owner/repo", 7).await.is_err());
    let wrong_host = tls::Server::start(
        certificates.wrong_host.clone(),
        Router::new().fallback(remote).with_state(state.clone()),
    )
    .await
    .unwrap();
    let wrong = Forge::synthetic_tls(
        ForgeConfiguration {
            kind: ForgeKind::Github,
            base: wrong_host.base.clone(),
        },
        "synthetic-forge-token".into(),
        1,
        &certificates.authority,
    )
    .unwrap();
    assert!(wrong.pull_request("owner/repo", 7).await.is_err());
    let origin = format!("{}/owner/repo.git", server.base);
    git(&repo, &["remote", "add", "origin", &origin]).await;
    let root = Dir::open_ambient_dir(&base, ambient_authority()).unwrap();
    let worktree = Worktree {
        path: work.to_str().unwrap().into(),
        source: repo.to_str().unwrap().into(),
        branch: "task".into(),
        base: "main".into(),
        base_commit: target.clone(),
        state: WorktreeState::Ready,
    };
    let database = Arc::new(Database::open(&base.join("synthetic.sqlite")).unwrap());
    let initial:Card=serde_json::from_str(&json!({"id":"task","title":"Human title","description":"Retain human content","repo":repo,"agent":"codex","worktree":worktree,"checklist":[{"text":"src/lib.rs returns 42","done":false}]}).to_string()).unwrap();
    let project = doc(
        json!({"id":"project","repo":repo,"automationEnabled":true,"autoFix":true,"testCommand":"synthetic-verification","prTarget":"main"}),
    );
    let saved = initial.clone();
    let settings = doc(json!({"forgeRevision":1,"forgeConnections":{"github":configuration}}));
    let selected = project.clone();
    database
        .call(move |store| {
            store.save_settings(&settings)?;
            store.put(Documents::Cards, &saved)?;
            store.put(Documents::Projects, &selected)
        })
        .await
        .unwrap();
    let start = doc(
        json!({"expected":initial,"project":project,"profile":atelier_core::domain::agent_options::providers(&Document::default()).unwrap()[0],"action":"start","confirmed":true}),
    );
    database
        .call(move |store| automation::change(store, "task", &start))
        .await
        .unwrap();
    let flow = automation::read(&card(&database).await).unwrap().unwrap();
    let operation = flow.id;
    let manager = CommandRuns::new(tokio::sync::broadcast::channel(16).0);
    for (id, verify) in [("development", false), ("verification", true)] {
        stage(
            &database, &manager, &root, &base, &worktree, &operation, id, verify, None,
        )
        .await;
        let op = operation.clone();
        database
            .call(move |store| automation::settle(store, "task", &op))
            .await
            .unwrap();
    }
    let forge = Forge::synthetic_tls(
        configuration.clone(),
        "synthetic-forge-token".into(),
        1,
        &certificates.authority,
    )
    .unwrap();
    let authentication = HttpsAuthentication::synthetic_tls(
        configuration.clone(),
        "owner/repo".into(),
        "synthetic-forge-token".into(),
        &certificates.authority,
    )
    .unwrap();
    let preview_input = ForgePublicationRequest {
        expected: card(&database).await,
        revision: 1,
        kind: ForgeKind::Github,
        repository: "owner/repo".into(),
        target: "main".into(),
        title: "Synthetic publication".into(),
        body: "Synthetic reviewed change".into(),
        draft: true,
        confirmed: true,
    };
    let preview = forge_publication::preview_with_adapters(
        &root,
        std::slice::from_ref(&base),
        &preview_input,
        configuration.clone(),
        &forge,
        &authentication,
    )
    .await
    .unwrap();
    assert_eq!(preview.source_commit, source);
    assert_eq!(preview.target_commit, target);
    assert_eq!(preview.remote_source.as_ref(), Some(&source));
    assert!(!preview.local_changes);
    let expected = card(&database).await;
    let selected = preview.clone();
    let prepared_card = database
        .call(move |store| forge_publication::save_preview(store, &expected, selected))
        .await
        .unwrap();
    let approve = doc(json!({"expected":prepared_card,"action":"publish","confirmed":true}));
    database
        .call(move |store| automation::change(store, "task", &approve))
        .await
        .unwrap();
    stage(
        &database,
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
    database
        .call(move |store| automation::settle(store, "task", &op))
        .await
        .unwrap();
    let publication = "00000000-0000-4000-8000-000000000002";
    let expected = card(&database).await;
    let input = ForgePublicationRequest {
        expected,
        revision: 1,
        kind: ForgeKind::Github,
        repository: "owner/repo".into(),
        target: "main".into(),
        title: preview.title.clone(),
        body: preview.body.clone(),
        draft: true,
        confirmed: true,
    };
    let reserved = database
        .call(move |store| forge_publication::reserve(store, &input, publication))
        .await
        .unwrap();
    let prepared = forge
        .prepare_publication(&preview, publication)
        .await
        .unwrap();
    database
        .call(move |store| forge_publication::sending(store, &reserved, publication))
        .await
        .unwrap();
    assert!(forge.create_publication(prepared).await.is_err());
    database
        .call(move |store| forge_publication::interrupted(store, "task", publication))
        .await
        .unwrap();
    assert!(
        card(&database)
            .await
            .fields
            .get::<ForgePublicationIntent>("forgePublication")
            .unwrap()
            .is_some_and(|intent| intent.state == ForgeWriteState::Uncertain)
    );
    let observed = forge
        .reconcile_publication(&preview, publication, None)
        .await
        .unwrap()
        .unwrap();
    let linked = database
        .call(move |store| forge_publication::record(store, "task", publication, observed))
        .await
        .unwrap();
    let op = operation.clone();
    database
        .call(move |store| automation::publication_complete(store, &linked, &op))
        .await
        .unwrap();
    assert!(
        authentication
            .origin(&origin.replace("repo.git", "other.git"))
            .is_err()
    );
    let before_head = git(&repo, &["rev-parse", "HEAD"]).await;
    let before_index = std::fs::read(repo.join(".git/index")).unwrap();
    let before_fetch = std::fs::read(repo.join(".git/FETCH_HEAD")).ok();
    let expected = card(&database).await;
    let foreign_git = HttpsAuthentication::synthetic_tls(
        configuration.clone(),
        "owner/repo".into(),
        "synthetic-forge-token".into(),
        &certificates.foreign,
    )
    .unwrap();
    let before_reads = state.git_reads.load(Ordering::SeqCst);
    assert!(
        reviews::prepare_with_adapters(
            &root,
            std::slice::from_ref(&base),
            &expected,
            configuration.clone(),
            &forge,
            &foreign_git
        )
        .await
        .is_err()
    );
    assert_eq!(state.git_reads.load(Ordering::SeqCst), before_reads);
    let context = reviews::prepare_with_adapters(
        &root,
        std::slice::from_ref(&base),
        &expected,
        configuration.clone(),
        &forge,
        &authentication,
    )
    .await
    .unwrap();
    assert_eq!(context.source, source);
    assert_eq!(context.target, target);
    assert_eq!(context.base, target);
    assert_eq!(git(&repo, &["rev-parse", "HEAD"]).await, before_head);
    assert_eq!(
        std::fs::read(repo.join(".git/index")).unwrap(),
        before_index
    );
    assert_eq!(
        std::fs::read(repo.join(".git/FETCH_HEAD")).ok(),
        before_fetch
    );
    stage(
        &database,
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
    // Re-observe both remote commits, worktree and criteria before accepting the process report.
    let expected = card(&database).await;
    let verified = reviews::prepare_with_adapters(
        &root,
        std::slice::from_ref(&base),
        &expected,
        configuration,
        &forge,
        &authentication,
    )
    .await
    .unwrap();
    assert_eq!(context, verified);
    let op = operation.clone();
    database
        .call(move |store| {
            reviews::validate(store, &expected, "review", &verified, "synthetic-verified")?;
            automation::settle(store, "task", &op)
        })
        .await
        .unwrap();
    let retained = card(&database).await;
    let flow = automation::read(&retained).unwrap().unwrap();
    assert!(flow.state == FlowState::Ready && flow.step == Step::Review);
    assert_eq!(retained.title.utf8().unwrap(), "Human title");
    assert_eq!(
        retained.fields.get::<Value>("checklist").unwrap(),
        initial.fields.get::<Value>("checklist").unwrap()
    );
    assert_eq!(state.writes.load(Ordering::SeqCst), 1);
    assert!(state.git_reads.load(Ordering::SeqCst) > 0);
    // A real source modification prevents a new validated context; no forged completion is inserted.
    std::fs::write(
        work.join("src/lib.rs"),
        "pub fn synthetic_answer() -> u8 { 7 }\n",
    )
    .unwrap();
    assert!(
        reviews::prepare_with_adapters(
            &root,
            &[base],
            &retained,
            ForgeConfiguration {
                kind: ForgeKind::Github,
                base: server.base.clone()
            },
            &forge,
            &authentication
        )
        .await
        .is_err()
    );
    manager.close().await.unwrap();
    database.close().await.unwrap();
}
