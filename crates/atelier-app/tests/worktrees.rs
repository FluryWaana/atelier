use atelier_app::infrastructure::{process_tree, worktrees};
use cap_std::{ambient_authority, fs::Dir};
use std::{path::Path, time::Duration};

fn null() -> &'static str {
    if cfg!(windows) { "NUL" } else { "/dev/null" }
}
fn link_directory(target: &Path, link: impl AsRef<Path>) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_dir(target, link)
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link)
    }
}
async fn git(repo: &Path, args: &[&str]) -> Vec<u8> {
    let mut command = tokio::process::Command::new("git");
    command
        .current_dir(repo)
        .env("GIT_CONFIG_GLOBAL", null())
        .env("GIT_CONFIG_SYSTEM", null())
        .args([
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.longpaths=true",
            "-c",
            &format!("core.hooksPath={}", null()),
            "-c",
            "filter.review.clean=",
            "-c",
            "filter.review.smudge=",
            "-c",
            "filter.review.process=",
            "-c",
            "filter.review.required=false",
            "-c",
            "user.name=Synthetic",
            "-c",
            "user.email=synthetic@example.invalid",
        ])
        .args(args);
    let output = process_tree::capture(&mut command, &[], Duration::from_secs(15), 1024 * 1024)
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
async fn authenticated_fetch_uses_only_the_selected_repository_and_preserves_local_state() {
    use atelier_app::infrastructure::git_transport::HttpsAuthentication;
    use atelier_core::contracts::{ForgeConfiguration, ForgeKind};
    use axum::{
        Router,
        extract::{Request, State},
        http::{StatusCode, header},
        response::{IntoResponse, Response},
    };
    use base64::{Engine, engine::general_purpose::STANDARD};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    #[derive(Clone)]
    struct Remote {
        directory: std::path::PathBuf,
        requests: Arc<AtomicUsize>,
        fault: Arc<AtomicUsize>,
    }
    async fn serve(State(remote): State<Remote>, request: Request) -> Response {
        remote.requests.fetch_add(1, Ordering::SeqCst);
        assert_eq!(request.method(), "GET");
        assert_eq!(
            request.headers()[header::AUTHORIZATION].to_str().unwrap(),
            format!("Basic {}", STANDARD.encode("atelier:synthetic-git-token"))
        );
        let Some(path) = request.uri().path().strip_prefix("/owner/repo.git/") else {
            panic!("Git requested a different repository");
        };
        assert!(!path.split('/').any(|part| part == ".."));
        match remote.fault.load(Ordering::SeqCst) {
            1 => {
                return (
                    StatusCode::FOUND,
                    [(header::LOCATION, "/unselected/repository")],
                )
                    .into_response();
            }
            2 => {
                return (
                    StatusCode::UNAUTHORIZED,
                    "synthetic-git-token must never reach the caller",
                )
                    .into_response();
            }
            _ => {}
        }
        match std::fs::read(remote.directory.join(path)) {
            Ok(bytes) => ([(header::CONTENT_TYPE, "text/plain")], bytes).into_response(),
            Err(_) => StatusCode::NOT_FOUND.into_response(),
        }
    }
    let fixture = tempfile::tempdir().unwrap();
    let root_path = std::fs::canonicalize(fixture.path()).unwrap();
    let remote_path = root_path.join("repositories/remote");
    let repo = root_path.join("repositories/source");
    std::fs::create_dir_all(&remote_path).unwrap();
    git(&remote_path, &["init", "-b", "main"]).await;
    std::fs::write(remote_path.join("version.txt"), "one").unwrap();
    git(&remote_path, &["add", "."]).await;
    git(&remote_path, &["commit", "-qm", "Synthetic initial"]).await;
    git(
        &root_path,
        &[
            "clone",
            "--no-hardlinks",
            "--",
            remote_path.to_str().unwrap(),
            repo.to_str().unwrap(),
        ],
    )
    .await;
    std::fs::write(remote_path.join("version.txt"), "two").unwrap();
    git(&remote_path, &["commit", "-am", "Synthetic remote change"]).await;
    git(&remote_path, &["update-server-info"]).await;
    let expected = String::from_utf8(git(&remote_path, &["rev-parse", "HEAD"]).await)
        .unwrap()
        .trim()
        .to_owned();
    let requests = Arc::new(AtomicUsize::new(0));
    let fault = Arc::new(AtomicUsize::new(0));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    let app = Router::new().fallback(serve).with_state(Remote {
        directory: remote_path.join(".git"),
        requests: requests.clone(),
        fault: fault.clone(),
    });
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let origin = format!("{address}/owner/repo.git");
    git(&repo, &["remote", "set-url", "origin", &origin]).await;
    std::fs::write(repo.join("version.txt"), "human staged edit").unwrap();
    git(&repo, &["add", "version.txt"]).await;
    std::fs::write(repo.join("version.txt"), "human unstaged edit").unwrap();
    std::fs::write(repo.join(".git/FETCH_HEAD"), "preserve synthetic fetch\n").unwrap();
    let head = git(&repo, &["rev-parse", "HEAD"]).await;
    let refs = git(&repo, &["show-ref"]).await;
    let index = std::fs::read(repo.join(".git/index")).unwrap();
    let config = std::fs::read(repo.join(".git/config")).unwrap();
    let authentication = HttpsAuthentication::synthetic(
        ForgeConfiguration {
            kind: ForgeKind::Github,
            base: address,
        },
        "owner/repo".into(),
        "synthetic-git-token".into(),
    )
    .unwrap();
    let root = Dir::open_ambient_dir(&root_path, ambient_authority()).unwrap();
    let roots = [root_path];
    let plan = worktrees::prepare_from_forge_origin(
        &root,
        &roots,
        repo.to_str().unwrap(),
        "task/authenticated".into(),
        "main".into(),
        &authentication,
    )
    .await
    .unwrap();
    assert_eq!(plan.base_commit, expected);
    assert!(requests.load(Ordering::SeqCst) > 0);
    for value in [1, 2] {
        fault.store(value, Ordering::SeqCst);
        let error = worktrees::prepare_from_forge_origin(
            &root,
            &roots,
            repo.to_str().unwrap(),
            "task/refused".into(),
            "main".into(),
            &authentication,
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(!error.contains("synthetic-git-token"));
    }
    let before = requests.load(Ordering::SeqCst);
    git(
        &repo,
        &[
            "config",
            "http.extraHeader",
            "Authorization: synthetic-forbidden",
        ],
    )
    .await;
    let overridden = std::fs::read(repo.join(".git/config")).unwrap();
    assert!(
        worktrees::prepare_from_forge_origin(
            &root,
            &roots,
            repo.to_str().unwrap(),
            "task/overridden".into(),
            "main".into(),
            &authentication
        )
        .await
        .is_err()
    );
    assert_eq!(requests.load(Ordering::SeqCst), before);
    assert_eq!(std::fs::read(repo.join(".git/config")).unwrap(), overridden);
    std::fs::write(repo.join(".git/config"), &config).unwrap();
    assert_eq!(git(&repo, &["rev-parse", "HEAD"]).await, head);
    assert_eq!(git(&repo, &["show-ref"]).await, refs);
    assert_eq!(std::fs::read(repo.join(".git/index")).unwrap(), index);
    assert_eq!(std::fs::read(repo.join(".git/config")).unwrap(), config);
    assert_eq!(
        std::fs::read_to_string(repo.join(".git/FETCH_HEAD")).unwrap(),
        "preserve synthetic fetch\n"
    );
    assert_eq!(
        std::fs::read_to_string(repo.join("version.txt")).unwrap(),
        "human unstaged edit"
    );
    assert!(
        git(
            &repo,
            &["for-each-ref", "--format=%(refname)", "refs/atelier-fetch/"]
        )
        .await
        .is_empty()
    );
    server.abort();
    let _ = server.await;
}

#[tokio::test]
async fn fetched_bases_are_pinned_without_moving_source_refs_or_fetch_head() {
    let fixture = tempfile::tempdir().unwrap();
    let root_path = std::fs::canonicalize(fixture.path()).unwrap();
    let remote = root_path.join("repositories/origin");
    let repo = root_path.join("repositories/source");
    std::fs::create_dir_all(&remote).unwrap();
    git(&remote, &["init", "-b", "main"]).await;
    std::fs::write(remote.join("version.txt"), "one").unwrap();
    git(&remote, &["add", "."]).await;
    git(&remote, &["commit", "-qm", "Synthetic initial version"]).await;
    git(
        &root_path,
        &[
            "clone",
            "--no-hardlinks",
            "--",
            remote.to_str().unwrap(),
            repo.to_str().unwrap(),
        ],
    )
    .await;
    let local_head = git(&repo, &["rev-parse", "HEAD"]).await;
    let tracking = git(&repo, &["rev-parse", "refs/remotes/origin/main"]).await;
    let index = std::fs::read(repo.join(".git/index")).unwrap();
    let config = std::fs::read(repo.join(".git/config")).unwrap();
    std::fs::write(repo.join(".git/FETCH_HEAD"), "preserve synthetic fetch\n").unwrap();
    std::fs::write(remote.join("version.txt"), "two").unwrap();
    git(&remote, &["commit", "-am", "Synthetic new version"]).await;
    let expected = String::from_utf8(git(&remote, &["rev-parse", "HEAD"]).await)
        .unwrap()
        .trim()
        .to_owned();
    let root = Dir::open_ambient_dir(&root_path, ambient_authority()).unwrap();
    let roots = [root_path.clone()];
    let mut plans = Vec::new();
    for (index, base) in [
        "main",
        "origin/main",
        "refs/remotes/origin/main",
        "refs/heads/main",
        "HEAD",
    ]
    .into_iter()
    .enumerate()
    {
        let plan = worktrees::prepare_from_origin(
            &root,
            &roots,
            repo.to_str().unwrap(),
            format!("task/{index}"),
            base.into(),
        )
        .await
        .unwrap();
        assert_eq!(plan.base_commit, expected);
        assert_eq!(plan.base, base);
        plans.push(plan);
    }
    assert_eq!(git(&repo, &["rev-parse", "HEAD"]).await, local_head);
    assert_eq!(
        git(&repo, &["rev-parse", "refs/remotes/origin/main"]).await,
        tracking
    );
    assert_eq!(std::fs::read(repo.join(".git/index")).unwrap(), index);
    assert_eq!(std::fs::read(repo.join(".git/config")).unwrap(), config);
    assert_eq!(
        std::fs::read_to_string(repo.join(".git/FETCH_HEAD")).unwrap(),
        "preserve synthetic fetch\n"
    );
    assert_eq!(
        std::fs::read_to_string(repo.join("version.txt")).unwrap(),
        "one"
    );
    for base in [
        "missing",
        "main:refs/heads/main",
        "refs/tags/main",
        "*",
        "--all",
    ] {
        assert!(
            worktrees::prepare_from_origin(
                &root,
                &roots,
                repo.to_str().unwrap(),
                "task/invalid".into(),
                base.into()
            )
            .await
            .is_err()
        );
    }
    assert!(
        git(
            &repo,
            &["for-each-ref", "--format=%(refname)", "refs/atelier-fetch/"]
        )
        .await
        .is_empty()
    );
    // Simulate restart after persisting the plan, while origin advances again.
    let plan: atelier_core::contracts::Worktree =
        serde_json::from_slice(&serde_json::to_vec(&plans[0]).unwrap()).unwrap();
    std::fs::write(remote.join("version.txt"), "three").unwrap();
    git(&remote, &["commit", "-am", "Synthetic later version"]).await;
    worktrees::create(&root, &roots, &plan).await.unwrap();
    assert_eq!(
        std::fs::read_to_string(Path::new(&plan.path).join("version.txt")).unwrap(),
        "two"
    );
    worktrees::create(&root, &roots, &plan).await.unwrap();
    assert_eq!(
        git(Path::new(&plan.path), &["rev-parse", "HEAD"]).await,
        format!("{expected}\n").as_bytes()
    );
    worktrees::remove(&root, &roots, &plan, false)
        .await
        .unwrap();
    git(
        &repo,
        &[
            "config",
            "url.https://example.invalid/.insteadOf",
            remote.to_str().unwrap(),
        ],
    )
    .await;
    assert!(
        worktrees::prepare_from_origin(
            &root,
            &roots,
            repo.to_str().unwrap(),
            "task/unsafe".into(),
            "main".into()
        )
        .await
        .unwrap_err()
        .to_string()
        .contains("réécritures")
    );
    git(
        &repo,
        &["config", "--remove-section", "url.https://example.invalid/"],
    )
    .await;
    git(
        &repo,
        &[
            "config",
            "remote.origin.url",
            "https://example.invalid/synthetic.git",
        ],
    )
    .await;
    assert!(
        worktrees::prepare_from_origin(
            &root,
            &roots,
            repo.to_str().unwrap(),
            "task/network".into(),
            "main".into()
        )
        .await
        .unwrap_err()
        .to_string()
        .contains("repositories")
    );
}

#[tokio::test]
async fn shared_dependencies_resume_and_survive_worktree_removal_and_refusal() {
    let fixture = tempfile::tempdir().unwrap();
    let root_path = std::fs::canonicalize(fixture.path()).unwrap();
    let repo = root_path.join("repositories/dependencies");
    std::fs::create_dir_all(repo.join("client")).unwrap();
    git(&repo, &["init", "-b", "main"]).await;
    git(&repo, &["config", "core.autocrlf", "false"]).await;
    for (file, contents) in [
        (".gitignore", "node_modules/\npackages/\n"),
        ("client/package.json", "{}"),
        ("project.csproj", "<Project/>"),
        ("packages/tracked.txt", "tracked source"),
    ] {
        std::fs::create_dir_all(repo.join(file).parent().unwrap()).unwrap();
        std::fs::write(repo.join(file), contents).unwrap();
        git(&repo, &["add", "-f", "--", file]).await;
    }
    git(&repo, &["commit", "-qm", "Synthetic dependencies baseline"]).await;
    let shared = [
        ("node_modules/keep.txt", "root dependency"),
        ("client/node_modules/keep.txt", "client dependency"),
        (
            "client/packages/library/library.nupkg",
            "synthetic NuGet archive",
        ),
    ];
    for (name, contents) in shared {
        std::fs::create_dir_all(repo.join(name).parent().unwrap()).unwrap();
        std::fs::write(repo.join(name), contents).unwrap();
    }
    let root = Dir::open_ambient_dir(&root_path, ambient_authority()).unwrap();
    let roots = [root_path.clone()];
    let plan = worktrees::prepare(
        &root,
        &roots,
        repo.to_str().unwrap(),
        "feature/dependencies".into(),
        "main".into(),
    )
    .await
    .unwrap();
    worktrees::create(&root, &roots, &plan).await.unwrap();
    let target = Path::new(&plan.path);
    for (name, contents) in shared {
        assert_eq!(
            std::fs::read_to_string(target.join(name)).unwrap(),
            contents
        );
    }
    assert!(std::fs::read_link(target.join("packages")).is_err());
    assert!(git(target, &["status", "--porcelain"]).await.is_empty());
    let gitdir = std::fs::read_to_string(target.join(".git")).unwrap();
    let metadata = Path::new(gitdir.trim().strip_prefix("gitdir: ").unwrap());
    let inventory = std::fs::read(metadata.join("atelier-dependencies.json")).unwrap();
    // An interrupted detach is recoverable from the intent retained in Git metadata.
    std::fs::remove_dir(target.join("client/node_modules")).unwrap();
    worktrees::create(&root, &roots, &plan).await.unwrap();
    assert_eq!(
        std::fs::read(metadata.join("atelier-dependencies.json")).unwrap(),
        inventory
    );
    std::fs::write(target.join("project.csproj"), "local edit").unwrap();
    assert!(
        worktrees::remove(&root, &roots, &plan, false)
            .await
            .is_err()
    );
    for (name, contents) in shared {
        assert_eq!(
            std::fs::read_to_string(target.join(name)).unwrap(),
            contents
        );
    }
    // Replacing an owned link with a real directory refuses even a forced removal.
    std::fs::remove_dir(target.join("node_modules")).unwrap();
    std::fs::create_dir(target.join("node_modules")).unwrap();
    std::fs::write(target.join("node_modules/personal.txt"), "preserve").unwrap();
    assert!(worktrees::remove(&root, &roots, &plan, true).await.is_err());
    assert_eq!(
        std::fs::read_to_string(target.join("node_modules/personal.txt")).unwrap(),
        "preserve"
    );
    std::fs::remove_file(target.join("node_modules/personal.txt")).unwrap();
    std::fs::remove_dir(target.join("node_modules")).unwrap();
    worktrees::create(&root, &roots, &plan).await.unwrap();
    worktrees::remove(&root, &roots, &plan, true).await.unwrap();
    assert!(!target.exists());
    for (name, contents) in shared {
        assert_eq!(std::fs::read_to_string(repo.join(name)).unwrap(), contents);
    }
    let clean = worktrees::prepare(
        &root,
        &roots,
        repo.to_str().unwrap(),
        "feature/dependencies-clean".into(),
        "main".into(),
    )
    .await
    .unwrap();
    worktrees::create(&root, &roots, &clean).await.unwrap();
    worktrees::remove(&root, &roots, &clean, false)
        .await
        .unwrap();
    assert!(repo.join("node_modules/keep.txt").is_file());
}

#[tokio::test]
async fn local_agent_context_refreshes_owned_files_preserves_task_edits_and_rejects_links() {
    let fixture = tempfile::tempdir().unwrap();
    let root_path = std::fs::canonicalize(fixture.path()).unwrap();
    let repo = root_path.join("repositories/context");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-b", "main"]).await;
    git(&repo, &["config", "core.autocrlf", "false"]).await;
    std::fs::write(repo.join("CLAUDE.md"), "tracked instructions").unwrap();
    std::fs::write(repo.join(".gitignore"), ".claude/\n.mcp.json\n").unwrap();
    git(&repo, &["add", "."]).await;
    git(&repo, &["commit", "-qm", "Synthetic context baseline"]).await;
    std::fs::write(repo.join("CLAUDE.md"), "uncommitted source edit").unwrap();
    let local = [
        ("AGENTS.md", "local agent instructions"),
        (".mcp.json", "{\"syntheticToken\":\"private-fixture\"}"),
        (".claude/settings.local.json", "{\"fixture\":true}"),
        (".claude/skills/review/SKILL.md", "synthetic review skill"),
        (".claude/rules/task.md", "original rule"),
        (".claude/commands/check.md", "original command"),
    ];
    for (name, bytes) in local {
        std::fs::create_dir_all(repo.join(name).parent().unwrap()).unwrap();
        std::fs::write(repo.join(name), bytes).unwrap();
    }
    std::fs::write(repo.join(".env"), "uncopied synthetic secret").unwrap();
    std::fs::create_dir_all(repo.join(".claude/worktrees/other")).unwrap();
    std::fs::write(
        repo.join(".claude/worktrees/other/private.txt"),
        "uncopied other workspace",
    )
    .unwrap();
    let root = Dir::open_ambient_dir(&root_path, ambient_authority()).unwrap();
    let roots = [root_path.clone()];
    let plan = worktrees::prepare(
        &root,
        &roots,
        repo.to_str().unwrap(),
        "feature/context".into(),
        "main".into(),
    )
    .await
    .unwrap();
    worktrees::create(&root, &roots, &plan).await.unwrap();
    let target = Path::new(&plan.path);
    for (name, bytes) in local {
        assert_eq!(std::fs::read_to_string(target.join(name)).unwrap(), bytes);
    }
    assert_eq!(
        std::fs::read_to_string(target.join("CLAUDE.md")).unwrap(),
        "tracked instructions"
    );
    assert!(!target.join(".env").exists());
    assert!(!target.join(".claude/worktrees").exists());
    assert!(git(target, &["status", "--porcelain"]).await.is_empty());
    std::fs::write(
        repo.join(".claude/settings.local.json"),
        "{\"updated\":true}",
    )
    .unwrap();
    std::fs::write(target.join(".claude/rules/task.md"), "task-specific rule").unwrap();
    std::fs::write(repo.join(".claude/rules/task.md"), "changed source rule").unwrap();
    std::fs::remove_file(repo.join(".claude/commands/check.md")).unwrap();
    worktrees::create(&root, &roots, &plan).await.unwrap();
    assert_eq!(
        std::fs::read_to_string(target.join(".claude/settings.local.json")).unwrap(),
        "{\"updated\":true}"
    );
    assert_eq!(
        std::fs::read_to_string(target.join(".claude/rules/task.md")).unwrap(),
        "task-specific rule"
    );
    assert!(!target.join(".claude/commands/check.md").exists());
    assert!(
        worktrees::remove(&root, &roots, &plan, false)
            .await
            .is_err()
    );
    let outside = root_path.join("outside");
    std::fs::create_dir(&outside).unwrap();
    std::fs::write(
        outside.join("settings.json"),
        "outside synthetic configuration",
    )
    .unwrap();
    std::fs::remove_file(target.join(".claude/settings.local.json")).unwrap();
    std::fs::hard_link(
        outside.join("settings.json"),
        target.join(".claude/settings.local.json"),
    )
    .unwrap();
    assert!(worktrees::create(&root, &roots, &plan).await.is_err());
    assert_eq!(
        std::fs::read_to_string(outside.join("settings.json")).unwrap(),
        "outside synthetic configuration"
    );
    std::fs::remove_file(target.join(".claude/settings.local.json")).unwrap();
    // Ownership is relinquished after a task edit, even when a later source copy happens to match it.
    std::fs::write(repo.join(".claude/rules/task.md"), "task-specific rule").unwrap();
    worktrees::create(&root, &roots, &plan).await.unwrap();
    std::fs::write(
        repo.join(".claude/rules/task.md"),
        "do not replace task ownership",
    )
    .unwrap();
    worktrees::create(&root, &roots, &plan).await.unwrap();
    assert_eq!(
        std::fs::read_to_string(target.join(".claude/rules/task.md")).unwrap(),
        "task-specific rule"
    );
    worktrees::remove(&root, &roots, &plan, true).await.unwrap();
    assert_eq!(
        std::fs::read_to_string(repo.join("CLAUDE.md")).unwrap(),
        "uncommitted source edit"
    );
    let clean = worktrees::prepare(
        &root,
        &roots,
        repo.to_str().unwrap(),
        "feature/context-clean".into(),
        "main".into(),
    )
    .await
    .unwrap();
    worktrees::create(&root, &roots, &clean).await.unwrap();
    worktrees::remove(&root, &roots, &clean, false)
        .await
        .unwrap();
    let linked = worktrees::prepare(
        &root,
        &roots,
        repo.to_str().unwrap(),
        "feature/context-link".into(),
        "main".into(),
    )
    .await
    .unwrap();
    worktrees::create(&root, &roots, &linked).await.unwrap();
    let linked_path = Path::new(&linked.path);
    std::fs::rename(
        linked_path.join(".claude"),
        linked_path.join("saved-context"),
    )
    .unwrap();
    link_directory(&outside, linked_path.join(".claude")).unwrap();
    assert!(worktrees::create(&root, &roots, &linked).await.is_err());
    assert!(!outside.join("settings.local.json").exists());
    worktrees::remove(&root, &roots, &linked, true)
        .await
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(outside.join("settings.json")).unwrap(),
        "outside synthetic configuration"
    );
}

#[tokio::test]
async fn worktrees_resume_verify_ownership_and_require_explicit_discard_without_deleting_branches()
{
    let fixture = tempfile::tempdir().unwrap();
    let root_path = std::fs::canonicalize(fixture.path()).unwrap();
    let repo = root_path.join("repositories/project");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-b", "main"]).await;
    git(&repo, &["config", "core.autocrlf", "false"]).await;
    std::fs::write(repo.join("source.txt"), "baseline\n").unwrap();
    git(&repo, &["config", "core.longpaths", "false"]).await;
    let long_path = Path::new("legacy")
        .join("segment-".repeat(10))
        .join("nested-".repeat(12))
        .join("long-tracked-file.txt");
    std::fs::create_dir_all(repo.join(&long_path).parent().unwrap()).unwrap();
    std::fs::write(repo.join(&long_path), "long path synthetic content").unwrap();
    std::fs::write(repo.join(".gitignore"), "ignored.txt\n").unwrap();
    std::fs::write(repo.join(".gitattributes"), "*.txt filter=review\n").unwrap();
    git(&repo, &["add", "."]).await;
    git(&repo, &["commit", "-qm", "Synthetic baseline"]).await;
    std::fs::write(repo.join("source-only.txt"), "source working copy").unwrap();
    std::fs::write(
        repo.join(".git/hooks/post-checkout"),
        "#!/bin/sh\necho invoked > hook-must-not-run\n",
    )
    .unwrap();
    for key in [
        "filter.review.clean",
        "filter.review.smudge",
        "filter.review.process",
    ] {
        git(
            &repo,
            &["config", key, "echo invoked > filter-must-not-run; cat"],
        )
        .await;
    }
    git(&repo, &["config", "filter.review.required", "true"]).await;
    let head = std::fs::read(repo.join(".git/HEAD")).unwrap();
    let index = std::fs::read(repo.join(".git/index")).unwrap();
    let config = std::fs::read(repo.join(".git/config")).unwrap();
    let root = Dir::open_ambient_dir(&root_path, ambient_authority()).unwrap();
    let roots = [root_path.clone()];
    let plan = worktrees::prepare(
        &root,
        &roots,
        repo.to_str().unwrap(),
        "feature/synthetic".into(),
        "main".into(),
    )
    .await
    .unwrap();
    assert!(!Path::new(&plan.path).exists());
    worktrees::create(&root, &roots, &plan).await.unwrap();
    worktrees::create(&root, &roots, &plan).await.unwrap();
    let target = Path::new(&plan.path);
    assert!(target.join(&long_path).as_os_str().len() > 260);
    assert_eq!(
        std::fs::read_to_string(target.join(&long_path)).unwrap(),
        "long path synthetic content"
    );
    assert_eq!(
        worktrees::inspect(&root, &roots, &plan)
            .await
            .unwrap()
            .preview("source.txt".into())
            .await
            .unwrap()
            .content
            .as_deref(),
        Some("baseline\n")
    );
    assert!(!target.join("hook-must-not-run").exists());
    assert!(!target.join("filter-must-not-run").exists());
    assert!(!repo.join("hook-must-not-run").exists());
    assert!(!repo.join("filter-must-not-run").exists());
    assert_eq!(std::fs::read(repo.join(".git/HEAD")).unwrap(), head);
    assert_eq!(std::fs::read(repo.join(".git/index")).unwrap(), index);
    assert_eq!(std::fs::read(repo.join(".git/config")).unwrap(), config);
    assert_eq!(
        std::fs::read_to_string(repo.join("source-only.txt")).unwrap(),
        "source working copy"
    );
    assert!(
        worktrees::prepare(
            &root,
            &roots,
            repo.to_str().unwrap(),
            "feature/synthetic".into(),
            "main".into()
        )
        .await
        .is_err()
    );
    for branch in ["--force", "@{-1}", "feature/../escape", "bad branch"] {
        assert!(
            worktrees::prepare(
                &root,
                &roots,
                repo.to_str().unwrap(),
                branch.into(),
                "main".into()
            )
            .await
            .is_err()
        );
    }
    let mut forged = plan.clone();
    forged.path = repo.to_str().unwrap().into();
    assert!(
        worktrees::remove(&root, &roots, &forged, true)
            .await
            .is_err()
    );
    let metadata = std::fs::read(target.join(".git")).unwrap();
    std::fs::write(target.join(".git"), "gitdir: C:/synthetic-outside\n").unwrap();
    assert!(worktrees::inspect(&root, &roots, &plan).await.is_err());
    std::fs::write(target.join(".git"), metadata).unwrap();
    git(target, &["switch", "-c", "unexpected-branch"]).await;
    assert!(worktrees::remove(&root, &roots, &plan, true).await.is_err());
    git(target, &["switch", "feature/synthetic"]).await;
    git(&repo, &["worktree", "lock", &plan.path]).await;
    assert!(worktrees::remove(&root, &roots, &plan, true).await.is_err());
    git(&repo, &["worktree", "unlock", &plan.path]).await;
    std::fs::write(target.join("task-commit.txt"), "task commit\n").unwrap();
    git(target, &["add", "task-commit.txt"]).await;
    git(target, &["commit", "-qm", "Synthetic task commit"]).await;
    let changes = worktrees::inspect(&root, &roots, &plan)
        .await
        .unwrap()
        .changes(Some(&plan.base_commit))
        .await
        .unwrap();
    assert!(
        changes
            .files
            .iter()
            .any(|entry| entry.path == "task-commit.txt")
    );
    std::fs::write(
        target.join("ignored.txt"),
        "ignored work must survive refusal",
    )
    .unwrap();
    assert!(
        worktrees::remove(&root, &roots, &plan, false)
            .await
            .is_err()
    );
    assert_eq!(
        std::fs::read_to_string(target.join("ignored.txt")).unwrap(),
        "ignored work must survive refusal"
    );
    worktrees::remove(&root, &roots, &plan, true).await.unwrap();
    assert!(!target.exists());
    worktrees::remove(&root, &roots, &plan, true).await.unwrap();
    assert!(
        !git(
            &repo,
            &["show-ref", "--verify", "refs/heads/feature/synthetic"]
        )
        .await
        .is_empty()
    );
    let collision = worktrees::prepare(
        &root,
        &roots,
        repo.to_str().unwrap(),
        "feature/collision".into(),
        "main".into(),
    )
    .await
    .unwrap();
    std::fs::create_dir(&collision.path).unwrap();
    std::fs::write(
        Path::new(&collision.path).join("preserved.txt"),
        "preserved",
    )
    .unwrap();
    assert!(worktrees::create(&root, &roots, &collision).await.is_err());
    assert!(
        worktrees::remove(&root, &roots, &collision, true)
            .await
            .is_err()
    );
    assert_eq!(
        std::fs::read_to_string(Path::new(&collision.path).join("preserved.txt")).unwrap(),
        "preserved"
    );
}
