use atelier_app::{
    http,
    infrastructure::{files::Repository, launch_detection, repository_access, session},
};

use cap_std::{ambient_authority, fs::Dir};
use reqwest::Method;
use serde_json::{Value, json};
use std::{fs, time::Duration};
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
    assert!(repository_access::revoke(&root, source_text, 1, true).is_err());
    assert!(repository_access::revoke(&root, source_text, 2, false).is_err());
    assert_eq!(repository_access::read(&root).unwrap().revision, 2);
    let removed = repository_access::revoke(&root, source_text, 2, true).unwrap();
    assert!(removed.repositories.is_empty());
    assert!(source.join(".git").is_dir());
    assert!(repository_access::open(&root, &source).is_err());
}

#[tokio::test]
async fn native_revocation_preserves_references_and_requires_authorization_for_future_access() {
    let fixture = tempfile::tempdir().unwrap();
    let base = fs::canonicalize(fixture.path()).unwrap();
    #[cfg(windows)]
    let base = std::path::PathBuf::from(base.to_string_lossy().trim_start_matches(r"\\?\"));
    let data = base.join("data");
    let assets = base.join("assets");
    let source = base.join("external");
    fs::create_dir(&assets).unwrap();
    fs::write(assets.join("index.html"), "synthetic authorization journey").unwrap();
    fs::create_dir_all(source.join(".git")).unwrap();
    fs::write(source.join("human.txt"), b"unchanged human content").unwrap();
    let manifest = br#"{"scripts":{"dev":"synthetic-service"}}"#;
    fs::write(source.join("package.json"), manifest).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(http::serve_native(listener, data.clone(), assets));
    let client = reqwest::Client::new();
    let token = tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            if let Ok(token) = session::read(&data)
                && client
                    .get(format!("{address}/api/state"))
                    .bearer_auth(&token)
                    .send()
                    .await
                    .is_ok_and(|response| response.status().is_success())
            {
                break token;
            }
            assert!(!server.is_finished(), "Native server startup failed");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    let request = |method, path: &str| {
        client
            .request(method, format!("{address}{path}"))
            .bearer_auth(&token)
            .header("x-local-request", "1")
    };
    let registered: Value = request(Method::POST, "/api/repositories")
        .json(&json!({"path":source,"expectedRevision":0,"confirmed":true}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(registered["revision"], 1);
    let project: Value = request(Method::POST, "/api/projects")
        .json(&json!({"name":"Referenced external repository","repo":source}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let card: Value = request(Method::POST, "/api/cards")
        .json(&json!({"title":"Keep this local task","repo":source}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    request(Method::PUT, "/api/settings")
        .json(&json!({"repo":source}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    let detection = format!(
        "/api/projects/{}/detect-launch",
        project["id"].as_str().unwrap()
    );
    request(Method::GET, &detection)
        .json(&json!({}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    let before: Value = request(Method::GET, "/api/state")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(before["projects"], json!([project]));
    assert_eq!(before["cards"], json!([card]));
    assert_eq!(before["settings"]["repo"], project["repo"]);
    for (revision, confirmed) in [(0, true), (1, false)] {
        assert!(
            request(Method::DELETE, "/api/repositories")
                .json(&json!({"path":source,"expectedRevision":revision,"confirmed":confirmed}))
                .send()
                .await
                .unwrap()
                .status()
                .is_client_error()
        );
    }

    // Hold a real external mutation at a local HTTP double before testing the exclusive guard.
    let parent = data.join("repositories");
    fs::create_dir_all(&parent).unwrap();
    let remote = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let cloning = request(Method::POST, "/api/repositories/clone").json(&json!({
        "url":format!("https://{}/team/repo.git", remote.local_addr().unwrap()),
        "parent":parent,"directory":"pending-clone","connection":null,
        "operation":"synthetic-revocation-clone","confirmed":true
    }));
    let cloning = tokio::spawn(async move { cloning.send().await.unwrap() });
    let (_connection, _) = tokio::time::timeout(Duration::from_secs(15), remote.accept())
        .await
        .unwrap()
        .unwrap();
    let blocked = request(Method::DELETE, "/api/repositories")
        .json(&json!({"path":source,"expectedRevision":1,"confirmed":true}))
        .send()
        .await
        .unwrap();
    assert_eq!(blocked.status(), 409);
    assert_eq!(
        blocked.json::<Value>().await.unwrap()["error"],
        "message.a_change_is_in_progress_try_again_shortly"
    );
    request(
        Method::POST,
        "/api/repositories/clone/synthetic-revocation-clone/cancel",
    )
    .json(&json!({}))
    .send()
    .await
    .unwrap()
    .error_for_status()
    .unwrap();
    assert!(
        !tokio::time::timeout(Duration::from_secs(15), cloning)
            .await
            .unwrap()
            .unwrap()
            .status()
            .is_success()
    );
    assert!(!parent.join("pending-clone").exists());
    let unchanged: Value = request(Method::GET, "/api/repositories")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(unchanged, registered);
    let removed: Value = request(Method::DELETE, "/api/repositories")
        .json(&json!({"path":source,"expectedRevision":1,"confirmed":true}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(removed["revision"], 2);
    assert_eq!(removed["repositories"], json!([]));
    let after: Value = request(Method::GET, "/api/state")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(after, before);
    assert_eq!(
        fs::read(source.join("human.txt")).unwrap(),
        b"unchanged human content"
    );
    assert_eq!(fs::read(source.join("package.json")).unwrap(), manifest);
    assert!(source.join(".git").is_dir());
    let refused = request(Method::GET, &detection)
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert!(refused.status().is_client_error());
    assert_eq!(
        refused.json::<Value>().await.unwrap()["error"],
        "message.explicitly_authorize_this_repository_in_settings_before_using_it"
    );
    let restored: Value = request(Method::POST, "/api/repositories")
        .json(&json!({"path":source,"expectedRevision":2,"confirmed":true}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(restored["revision"], 3);
    assert_eq!(restored["repositories"], registered["repositories"]);
    request(Method::GET, &detection)
        .json(&json!({}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    request(Method::POST, "/api/shutdown")
        .json(&json!({}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    server.await.unwrap().unwrap();
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
