#![cfg(windows)]

use atelier_app::infrastructure::{process_tree, worktrees};
use cap_std::{ambient_authority, fs::Dir};
use std::{path::Path, time::Duration};

async fn git(repo: &Path, args: &[&str]) -> Vec<u8> {
    let mut command = tokio::process::Command::new("git");
    command
        .current_dir(repo)
        .env("GIT_CONFIG_GLOBAL", "NUL")
        .env("GIT_CONFIG_SYSTEM", "NUL")
        .args([
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.longpaths=true",
            "-c",
            "core.hooksPath=NUL",
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
    std::os::windows::fs::symlink_dir(&outside, linked_path.join(".claude")).unwrap();
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
