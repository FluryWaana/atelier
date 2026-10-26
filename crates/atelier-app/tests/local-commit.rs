#![cfg(windows)]
use atelier_app::infrastructure::{files::Repository, local_commit, process_tree};
use atelier_core::document::JsonString;
use cap_std::{ambient_authority, fs::Dir};
use std::{path::Path, time::Duration};

async fn git(path: &Path, args: &[&str]) -> Vec<u8> {
    let mut command = tokio::process::Command::new("git");
    command
        .current_dir(path)
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
    let output = process_tree::capture(&mut command, &[], Duration::from_secs(15), 4 * 1024 * 1024)
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
async fn committed_disk_can_be_reviewed_while_preserving_a_partially_staged_human_index() {
    let home = tempfile::tempdir().unwrap();
    let source = home.path().join("repo");
    std::fs::create_dir(&source).unwrap();
    git(&source, &["init", "-b", "main"]).await;
    std::fs::write(source.join("file.txt"), "baseline\n").unwrap();
    git(&source, &["add", "."]).await;
    git(&source, &["commit", "-m", "Synthetic baseline"]).await;
    std::fs::write(source.join("file.txt"), "human staged content\n").unwrap();
    git(&source, &["add", "file.txt"]).await;
    std::fs::write(source.join("file.txt"), "selected disk content\n").unwrap();
    let index = std::fs::read(source.join(".git/index")).unwrap();
    std::fs::write(source.join(".git/FETCH_HEAD"), "synthetic fetch marker").unwrap();
    let root = Dir::open_ambient_dir(home.path(), ambient_authority()).unwrap();
    let repository = Repository::synthetic(
        &root,
        &[home.path().to_owned()],
        &JsonString::from(source.to_string_lossy().replace('\\', "/")),
    )
    .unwrap();
    let preview = local_commit::preview(&repository, vec!["file.txt".into()])
        .await
        .unwrap();
    let commit = local_commit::prepare(
        &repository,
        &preview.snapshot,
        "Synthetic",
        "synthetic@example.invalid",
        "Synthetic selected content",
    )
    .await
    .unwrap();
    local_commit::publish(&repository, &preview.snapshot, &commit)
        .await
        .unwrap();
    assert!(
        local_commit::disk_matches_commit(&repository, &commit)
            .await
            .unwrap()
    );
    assert_eq!(std::fs::read(source.join(".git/index")).unwrap(), index);
    std::fs::write(source.join("file.txt"), "unpublished human change\n").unwrap();
    assert!(
        !local_commit::disk_matches_commit(&repository, &commit)
            .await
            .unwrap()
    );
    assert_eq!(std::fs::read(source.join(".git/index")).unwrap(), index);
    assert_eq!(
        std::fs::read_to_string(source.join(".git/FETCH_HEAD")).unwrap(),
        "synthetic fetch marker"
    );
    assert_eq!(
        local_commit::current(&repository, "main").await.unwrap(),
        commit
    );
}

#[tokio::test]
async fn selected_snapshot_preserves_human_index_disk_and_fetch_head_and_rejects_stale_preview() {
    let home = tempfile::tempdir().unwrap();
    let source = home.path().join("repo");
    std::fs::create_dir(&source).unwrap();
    git(&source, &["init", "-b", "main"]).await;
    for name in ["selected.txt", "unrelated.txt"] {
        std::fs::write(source.join(name), "base\n").unwrap();
    }
    git(&source, &["add", "--all"]).await;
    git(&source, &["commit", "-m", "Synthetic initial"]).await;
    std::fs::write(source.join("selected.txt"), "partially staged\n").unwrap();
    std::fs::write(source.join("unrelated.txt"), "unrelated staged\n").unwrap();
    git(&source, &["add", "--all"]).await;
    std::fs::write(source.join("selected.txt"), "selected working content\n").unwrap();
    std::fs::write(source.join("untracked.txt"), "human untracked\n").unwrap();
    std::fs::write(source.join(".git/FETCH_HEAD"), "synthetic fetch marker\n").unwrap();
    std::fs::write(
        source.join(".git/hooks/reference-transaction"),
        "#!/bin/sh\nexit 99\n",
    )
    .unwrap();
    std::fs::write(source.join(".git/hooks/pre-commit"), "#!/bin/sh\nexit 99\n").unwrap();
    // A configured clean filter must never replace the human content in the snapshot.
    git(&source, &["config", "filter.synthetic.clean", "exit 99"]).await;
    std::fs::write(
        source.join(".gitattributes"),
        "selected.txt filter=synthetic\n",
    )
    .unwrap();
    let index = std::fs::read(source.join(".git/index")).unwrap();
    let config = std::fs::read(source.join(".git/config")).unwrap();
    let fetch = std::fs::read(source.join(".git/FETCH_HEAD")).unwrap();
    let root = Dir::open_ambient_dir(home.path(), ambient_authority()).unwrap();
    let repository = Repository::synthetic(
        &root,
        &[home.path().to_owned()],
        &JsonString::from(source.to_string_lossy().replace('\\', "/")),
    )
    .unwrap();
    let preview = local_commit::preview(&repository, vec!["selected.txt".into()])
        .await
        .unwrap();
    assert!(preview.patch.contains("selected working content"));
    assert!(!preview.patch.contains("unrelated staged"));
    assert_eq!(std::fs::read(source.join(".git/index")).unwrap(), index);
    std::fs::write(source.join("selected.txt"), "changed since preview\n").unwrap();
    assert!(
        local_commit::prepare(
            &repository,
            &preview.snapshot,
            "Synthetic",
            "synthetic@example.invalid",
            "Synthetic selected commit"
        )
        .await
        .is_err()
    );
    std::fs::write(source.join("selected.txt"), "selected working content\n").unwrap();
    let commit = local_commit::prepare(
        &repository,
        &preview.snapshot,
        "Synthetic",
        "synthetic@example.invalid",
        "Synthetic selected commit",
    )
    .await
    .unwrap();
    assert_eq!(
        local_commit::current(&repository, "main").await.unwrap(),
        preview.snapshot.head
    );
    assert!(
        local_commit::publish(&repository, &preview.snapshot, &preview.snapshot.head)
            .await
            .is_err()
    );
    let competing = String::from_utf8(
        git(
            &source,
            &[
                "commit-tree",
                &preview.snapshot.tree,
                "-p",
                &preview.snapshot.head,
                "-m",
                "Synthetic concurrent commit",
            ],
        )
        .await,
    )
    .unwrap();
    git(
        &source,
        &[
            "update-ref",
            "refs/heads/main",
            competing.trim(),
            &preview.snapshot.head,
        ],
    )
    .await;
    assert!(
        local_commit::publish(&repository, &preview.snapshot, &commit)
            .await
            .is_err()
    );
    assert_eq!(
        local_commit::current(&repository, "main").await.unwrap(),
        competing.trim()
    );
    // Reset only this isolated synthetic reference to exercise the accepted CAS separately.
    git(
        &source,
        &[
            "update-ref",
            "refs/heads/main",
            &preview.snapshot.head,
            competing.trim(),
        ],
    )
    .await;
    // The reviewed immutable tree is committed; later edits are retained on disk.
    std::fs::write(source.join("selected.txt"), "human edit after prepare\n").unwrap();
    local_commit::publish(&repository, &preview.snapshot, &commit)
        .await
        .unwrap();
    assert_eq!(
        local_commit::current(&repository, "main").await.unwrap(),
        commit
    );
    assert_eq!(
        git(&source, &["show", "HEAD:selected.txt"]).await,
        b"selected working content\n"
    );
    assert_eq!(
        git(&source, &["show", "HEAD:unrelated.txt"]).await,
        b"base\n"
    );
    assert_eq!(
        std::fs::read(source.join("selected.txt")).unwrap(),
        b"human edit after prepare\n"
    );
    assert_eq!(
        std::fs::read(source.join("untracked.txt")).unwrap(),
        b"human untracked\n"
    );
    assert_eq!(std::fs::read(source.join(".git/index")).unwrap(), index);
    assert_eq!(std::fs::read(source.join(".git/config")).unwrap(), config);
    assert_eq!(
        std::fs::read(source.join(".git/FETCH_HEAD")).unwrap(),
        fetch
    );
    // The old compare-and-swap cannot rewrite a branch which has already moved.
    assert!(
        local_commit::publish(&repository, &preview.snapshot, &commit)
            .await
            .is_err()
    );
    assert_eq!(
        local_commit::current(&repository, "main").await.unwrap(),
        commit
    );
    assert!(
        local_commit::preview(&repository, vec![".git/config".into()])
            .await
            .is_err()
    );
    assert!(
        local_commit::preview(
            &repository,
            vec!["unrelated.txt".into(), "unrelated.txt".into()]
        )
        .await
        .is_err()
    );
}
