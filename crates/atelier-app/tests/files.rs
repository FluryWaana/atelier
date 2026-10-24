#![cfg(windows)]

use atelier_app::infrastructure::{files::Repository, process_tree};
use atelier_core::{document::JsonString, domain::files::PREVIEW_LIMIT};
use cap_std::{ambient_authority, fs::Dir};
use std::{path::Path, time::Duration};

async fn git(repo: &Path, args: &[&str]) {
    let mut command = tokio::process::Command::new("git");
    command
        .current_dir(repo)
        .env("GIT_CONFIG_GLOBAL", "NUL")
        .env("GIT_CONFIG_SYSTEM", "NUL")
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
    let marker = root_path.join("must-not-run.txt");
    git(
        &repo,
        &[
            "config",
            "core.fsmonitor",
            &format!("cmd /c echo invoked>\"{}\"", marker.display()),
        ],
    )
    .await;
    let index = std::fs::read(repo.join(".git/index")).unwrap();
    let root = Dir::open_ambient_dir(&root_path, ambient_authority()).unwrap();
    let roots = [root_path.clone()];
    let value = JsonString::from(repo.to_str().unwrap());
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
    std::os::windows::fs::symlink_dir(&outside, repo.join("linked")).unwrap();
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
    assert!(
        Repository::synthetic(&root, &roots, &JsonString::from(outside.to_str().unwrap())).is_err()
    );
    assert_eq!(std::fs::read(repo.join(".git/index")).unwrap(), index);
    assert!(!marker.exists());
    assert_eq!(
        std::fs::read_to_string(outside.join("secret.txt")).unwrap(),
        "synthetic outside sentinel"
    );
}
