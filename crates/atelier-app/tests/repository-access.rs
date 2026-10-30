use atelier_app::infrastructure::{files::Repository, launch_detection, repository_access};

use cap_std::{ambient_authority, fs::Dir};
use std::fs;
#[test]
fn external_repositories_require_exact_local_authorization_and_keep_human_files() {
    let fixture = tempfile::tempdir().unwrap();
    let base = fs::canonicalize(fixture.path()).unwrap();
    let data = base.join("data");
    let source = base.join("external");
    let other = base.join("other");
    fs::create_dir_all(&data).unwrap();
    fs::create_dir_all(source.join(".git")).unwrap();
    fs::create_dir_all(other.join(".git")).unwrap();
    fs::write(source.join("human.txt"), b"unchanged human content").unwrap();
    let root = Dir::open_ambient_dir(&data, ambient_authority()).unwrap();
    let source_text = source.to_str().unwrap();
    assert!(repository_access::open(&root, &source).is_err());
    assert!(repository_access::register(&root, source_text, 0, false).is_err());
    assert_eq!(repository_access::read(&root).unwrap().revision, 0);
    let registry = repository_access::register(&root, source_text, 0, true).unwrap();
    assert_eq!(registry.revision, 1);
    assert_eq!(registry.repositories.len(), 1);
    assert!(repository_access::register(&root, other.to_str().unwrap(), 0, true).is_err());
    assert!(repository_access::open(&root, &other).is_err());
    let roots = [data.clone(), data];
    let repo = String::from(source_text);
    drop(Repository::synthetic(&root, &roots, &repo).unwrap());
    assert!(launch_detection::detect_synthetic(&root, &roots, &repo).is_ok());
    assert_eq!(
        fs::read(source.join("human.txt")).unwrap(),
        b"unchanged human content"
    );
    let old = base.join("old-source");
    fs::rename(&source, &old).unwrap();
    fs::create_dir_all(source.join(".git")).unwrap();
    assert!(repository_access::open(&root, &source).is_err());
    assert!(Repository::synthetic(&root, &roots, &repo).is_err());
    assert_eq!(
        fs::read(old.join("human.txt")).unwrap(),
        b"unchanged human content"
    );
    repository_access::register(&root, source_text, 1, true).unwrap();
    drop(repository_access::open(&root, &source).unwrap());
    // Joining onto a verbatim Windows PathBuf normalizes parent components before validation.
    let traversal = format!(
        "{}{}..{}other",
        source.display(),
        std::path::MAIN_SEPARATOR,
        std::path::MAIN_SEPARATOR
    );
    assert!(repository_access::register(&root, &traversal, 2, true).is_err());
    assert_eq!(repository_access::read(&root).unwrap().revision, 2);
    assert!(repository_access::references(
        &serde_json::json!({"project":{"frontRepo":source_text}}),
        source_text
    ));
    assert!(repository_access::revoke(&root, source_text, 2, true, true).is_err());
    assert!(repository_access::revoke(&root, source_text, 1, true, false).is_err());
    assert!(repository_access::revoke(&root, source_text, 2, false, false).is_err());
    let removed = repository_access::revoke(&root, source_text, 2, true, false).unwrap();
    assert!(removed.repositories.is_empty());
    assert!(source.join(".git").is_dir());
    assert!(repository_access::open(&root, &source).is_err());
}
#[cfg(unix)]
#[test]
fn repository_authorization_never_follows_directory_or_git_metadata_links() {
    let fixture = tempfile::tempdir().unwrap();
    let base = fs::canonicalize(fixture.path()).unwrap();
    let data = base.join("data");
    let source = base.join("external");
    let link = base.join("linked");
    fs::create_dir_all(&data).unwrap();
    fs::create_dir_all(source.join(".git")).unwrap();
    std::os::unix::fs::symlink(&source, &link).unwrap();
    let root = Dir::open_ambient_dir(&data, ambient_authority()).unwrap();
    assert!(repository_access::register(&root, link.to_str().unwrap(), 0, true).is_err());
    fs::remove_dir(source.join(".git")).unwrap();
    std::os::unix::fs::symlink(&data, source.join(".git")).unwrap();
    assert!(repository_access::register(&root, source.to_str().unwrap(), 0, true).is_err());
    assert_eq!(repository_access::read(&root).unwrap().revision, 0);
}
