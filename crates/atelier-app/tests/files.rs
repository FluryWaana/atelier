use atelier_app::infrastructure::{files::Repository, process_tree};
use atelier_core::domain::files::PREVIEW_LIMIT;
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
        .args(["-c", "core.fsmonitor=false"])
        .args(args);
    let output = process_tree::capture(&mut command, &[], Duration::from_secs(10), 1024 * 1024)
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
async fn source_fingerprints_detect_staging_disk_and_untracked_changes_but_preserve_the_index() {
    let fixture = tempfile::tempdir().unwrap();
    let root_path = fixture.path().canonicalize().unwrap();
    let path = root_path.join("repositories/fingerprint");
    std::fs::create_dir_all(&path).unwrap();
    git(&path, &["init", "-b", "main"]).await;
    git(&path, &["config", "user.name", "Synthetic"]).await;
    git(
        &path,
        &["config", "user.email", "synthetic@example.invalid"],
    )
    .await;
    std::fs::write(path.join("tracked"), "one").unwrap();
    std::fs::write(path.join(".gitignore"), "ignored\n").unwrap();
    git(&path, &["add", "."]).await;
    git(
        &path,
        &[
            "-c",
            &format!("core.hooksPath={}", null()),
            "commit",
            "-qm",
            "Synthetic baseline",
        ],
    )
    .await;
    let root = Dir::open_ambient_dir(&root_path, ambient_authority()).unwrap();
    let repository = Repository::synthetic(&root, &[root_path], path.to_str().unwrap()).unwrap();
    let index = std::fs::read(path.join(".git/index")).unwrap();
    let baseline = repository.fingerprint().await.unwrap();
    assert_eq!(baseline, repository.fingerprint().await.unwrap());
    std::fs::write(path.join("ignored"), "ignored build output").unwrap();
    assert_eq!(baseline, repository.fingerprint().await.unwrap());
    std::fs::write(path.join("tracked"), "two").unwrap();
    let disk = repository.fingerprint().await.unwrap();
    assert_ne!(baseline, disk);
    assert_eq!(std::fs::read(path.join(".git/index")).unwrap(), index);
    git(&path, &["add", "tracked"]).await;
    let staged = repository.fingerprint().await.unwrap();
    assert_ne!(disk, staged);
    std::fs::write(path.join("new"), "untracked source").unwrap();
    let untracked = repository.fingerprint().await.unwrap();
    assert_ne!(staged, untracked);
    std::fs::remove_file(path.join("tracked")).unwrap();
    assert_ne!(untracked, repository.fingerprint().await.unwrap());
}

#[tokio::test]
async fn diffs_cover_all_states_preserve_git_and_never_invoke_repository_programs() {
    let fixture = tempfile::tempdir().unwrap();
    let root_path = std::fs::canonicalize(fixture.path()).unwrap();
    let repo = root_path.join("repositories/diffs");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-b", "main"]).await;
    git(&repo, &["config", "user.name", "Synthetic"]).await;
    git(
        &repo,
        &["config", "user.email", "synthetic@example.invalid"],
    )
    .await;
    git(&repo, &["config", "core.autocrlf", "false"]).await;
    for (name, content) in [
        ("changed.txt", "before\n"),
        ("deleted.txt", "deleted\n"),
        ("replaced.txt", "original\n"),
        (".gitignore", "ignored.txt\n"),
    ] {
        std::fs::write(repo.join(name), content).unwrap();
    }
    git(&repo, &["add", "."]).await;
    git(
        &repo,
        &[
            "-c",
            &format!("core.hooksPath={}", null()),
            "commit",
            "-qm",
            "Synthetic baseline",
        ],
    )
    .await;
    let base = String::from_utf8(git(&repo, &["rev-parse", "HEAD"]).await)
        .unwrap()
        .trim()
        .to_owned();
    std::fs::write(repo.join("committed.txt"), "committed\n").unwrap();
    git(&repo, &["add", "."]).await;
    git(
        &repo,
        &[
            "-c",
            &format!("core.hooksPath={}", null()),
            "commit",
            "-qm",
            "Synthetic task commit",
        ],
    )
    .await;
    std::fs::write(repo.join("staged.txt"), "staged\n").unwrap();
    git(&repo, &["add", "staged.txt"]).await;
    git(&repo, &["rm", "replaced.txt"]).await;
    std::fs::write(repo.join("replaced.txt"), "replacement\n").unwrap();
    std::fs::write(repo.join("changed.txt"), "after\n").unwrap();
    std::fs::remove_file(repo.join("deleted.txt")).unwrap();
    std::fs::write(
        repo.join("new é # [x].txt"),
        "<script>synthetic</script>\nlast",
    )
    .unwrap();
    std::fs::write(repo.join("ignored.txt"), "ignored synthetic content").unwrap();
    std::fs::write(repo.join("binary.bin"), [0, 1]).unwrap();
    std::fs::write(
        repo.join(".gitattributes"),
        "*.txt filter=review diff=review\n",
    )
    .unwrap();
    let marker = repo.join("must-not-run");
    for key in [
        "filter.review.clean",
        "filter.review.smudge",
        "filter.review.process",
        "diff.review.command",
        "diff.review.textconv",
        "core.fsmonitor",
    ] {
        git(&repo, &["config", key, "echo invoked > must-not-run; cat"]).await;
    }
    git(&repo, &["config", "filter.review.required", "true"]).await;
    let index = std::fs::read(repo.join(".git/index")).unwrap();
    let config = std::fs::read(repo.join(".git/config")).unwrap();
    let root = Dir::open_ambient_dir(&root_path, ambient_authority()).unwrap();
    let roots = [root_path.clone()];
    let value = String::from(repo.to_str().unwrap());
    let open = || Repository::synthetic(&root, &roots, &value).unwrap();
    let changes = open().changes(Some(&base)).await.unwrap();
    assert_eq!(changes.base, base);
    for (path, status) in [
        ("changed.txt", "M"),
        ("committed.txt", "A"),
        ("staged.txt", "A"),
        ("deleted.txt", "D"),
        ("new é # [x].txt", "A"),
    ] {
        assert!(
            changes
                .files
                .iter()
                .any(|entry| entry.path == path && entry.status == status),
            "{path}"
        );
    }
    assert!(
        !changes
            .files
            .iter()
            .any(|entry| entry.path == "ignored.txt")
    );
    let replacements = changes
        .files
        .iter()
        .filter(|entry| entry.path == "replaced.txt")
        .collect::<Vec<_>>();
    assert_eq!(replacements.len(), 2);
    assert!(
        replacements
            .iter()
            .any(|entry| entry.untracked && entry.status == "A")
    );
    assert!(
        replacements
            .iter()
            .any(|entry| !entry.untracked && entry.status == "D")
    );
    for (path, untracked, expected) in [
        ("changed.txt", false, "-before\n+after"),
        ("deleted.txt", false, "-deleted"),
        ("replaced.txt", false, "-original"),
        ("replaced.txt", true, "+replacement"),
        (
            "new é # [x].txt",
            true,
            "+<script>synthetic</script>\n+last\n\\ No newline",
        ),
    ] {
        let patch = open()
            .file_diff(path.into(), Some(&base), Some(untracked))
            .await
            .unwrap();
        assert!(patch.patch.unwrap().contains(expected), "{path}");
    }
    assert!(
        open()
            .file_diff("binary.bin".into(), None, Some(true))
            .await
            .unwrap()
            .unavailable
            .unwrap()
            .contains("binaire")
    );
    assert!(
        open()
            .file_diff(".git/config".into(), None, None)
            .await
            .is_err()
    );
    assert!(
        open()
            .file_diff("ignored.txt".into(), None, None)
            .await
            .is_err()
    );
    assert!(open().changes(Some("--help")).await.is_err());
    assert!(
        !open()
            .changes(None)
            .await
            .unwrap()
            .files
            .iter()
            .any(|entry| entry.path == "committed.txt")
    );
    assert_eq!(std::fs::read(repo.join(".git/index")).unwrap(), index);
    assert_eq!(std::fs::read(repo.join(".git/config")).unwrap(), config);
    assert!(!marker.exists());
    // A linked directory cannot be traversed while computing a diff either.
    let outside = root_path.join("outside");
    std::fs::create_dir(&outside).unwrap();
    std::fs::write(outside.join("secret.txt"), "synthetic outside").unwrap();
    link_directory(&outside, repo.join("linked")).unwrap();
    assert!(open().changes(None).await.is_err());
}

#[tokio::test]
async fn git_previews_preserve_the_index_refuse_escapes_and_bound_text_reads() {
    let fixture = tempfile::tempdir().unwrap();
    let root_path = std::fs::canonicalize(fixture.path()).unwrap();
    let repo = root_path.join("repositories/project");
    std::fs::create_dir_all(repo.join("src")).unwrap();
    git(&repo, &["init", "-b", "main"]).await;
    std::fs::write(repo.join(".gitignore"), "ignored.txt\n").unwrap();
    std::fs::write(repo.join("src/tracked.txt"), "tracked\n").unwrap();
    git(&repo, &["add", "--", ".gitignore", "src/tracked.txt"]).await;
    std::fs::write(repo.join("new file é.txt"), "<script>synthetic</script>\n").unwrap();
    std::fs::write(repo.join("ignored.txt"), "synthetic ignored secret").unwrap();
    std::fs::write(repo.join("large.txt"), vec![b'a'; PREVIEW_LIMIT + 1]).unwrap();
    std::fs::write(repo.join("binary.bin"), [1, 0, 2]).unwrap();
    std::fs::write(repo.join("legacy.txt"), [0xff, 0xfe]).unwrap();
    let marker = repo.join("must-not-run.txt");
    git(
        &repo,
        &[
            "config",
            "core.fsmonitor",
            &if cfg!(windows) {
                format!("cmd /c echo invoked>\"{}\"", marker.display())
            } else {
                "printf invoked > must-not-run.txt".into()
            },
        ],
    )
    .await;
    let index = std::fs::read(repo.join(".git/index")).unwrap();
    let root = Dir::open_ambient_dir(&root_path, ambient_authority()).unwrap();
    let roots = [root_path.clone()];
    let value = String::from(repo.to_str().unwrap());
    let open = || Repository::synthetic(&root, &roots, &value).unwrap();
    let listing = open().list().await.unwrap();
    assert!(listing.files.contains(&"src/tracked.txt".into()));
    assert!(listing.files.contains(&"new file é.txt".into()));
    assert!(!listing.files.contains(&"ignored.txt".into()));
    assert!(!listing.files.iter().any(|path| path.starts_with(".git/")));
    assert_eq!(
        open()
            .preview("new file é.txt".into())
            .await
            .unwrap()
            .content
            .as_deref(),
        Some("<script>synthetic</script>\n")
    );
    for (path, message) in [
        ("large.txt", "512 Kio"),
        ("binary.bin", "binaire"),
        ("legacy.txt", "UTF-8"),
    ] {
        let preview = open().preview(path.into()).await.unwrap();
        assert!(preview.content.is_none());
        assert!(preview.unavailable.unwrap().contains(message));
    }
    let outside = root_path.join("outside");
    std::fs::create_dir(&outside).unwrap();
    std::fs::write(outside.join("secret.txt"), "synthetic outside sentinel").unwrap();
    link_directory(&outside, repo.join("linked")).unwrap();
    for path in [
        "ignored.txt",
        "../outside/secret.txt",
        ".git/config",
        "src/../.git/config",
        "src/tracked.txt:stream",
        "linked/secret.txt",
    ] {
        assert!(open().preview(path.into()).await.is_err(), "{path}");
    }
    assert!(Repository::synthetic(&root, &roots, outside.to_str().unwrap()).is_err());
    assert_eq!(std::fs::read(repo.join(".git/index")).unwrap(), index);
    assert!(!marker.exists());
    assert_eq!(
        std::fs::read_to_string(outside.join("secret.txt")).unwrap(),
        "synthetic outside sentinel"
    );
}
