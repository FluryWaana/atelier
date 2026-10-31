use atelier_app::{http, infrastructure::session};
use serde_json::json;
use std::time::Duration;

#[tokio::test]
async fn clone_http_boundary_requires_confirmation_and_cancels_without_registering_a_project() {
    let fixture = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(fixture.path()).unwrap();
    #[cfg(windows)]
    let root = std::path::PathBuf::from(root.to_string_lossy().trim_start_matches(r"\\?\"));
    let data = root.join("store");
    let assets = root.join("assets");
    std::fs::create_dir(&assets).unwrap();
    std::fs::write(assets.join("index.html"), "synthetic clone journey").unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    let selected = data.clone();
    let server = tokio::spawn(http::serve(listener, selected, assets));
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
            assert!(!server.is_finished());
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    let parent = data.join("repositories");
    std::fs::create_dir_all(&parent).unwrap();
    let mut input = json!({"url":"https://forge.example.test/team/repo.git", "parent":parent, "directory":"cloned", "connection":null, "operation":"synthetic-http-clone", "confirmed":true});
    for (field, value) in [
        ("confirmed", json!(false)),
        ("url", json!("file:///synthetic")),
        ("directory", json!("../outside")),
        ("parent", json!(root)),
    ] {
        let mut invalid = input.clone();
        invalid[field] = value;
        let response = client
            .post(format!("{address}/api/repositories/clone"))
            .bearer_auth(&token)
            .header("X-Local-Request", "1")
            .json(&invalid)
            .send()
            .await
            .unwrap();
        assert!(response.status().is_client_error(), "{field}");
    }
    assert_eq!(std::fs::read_dir(&parent).unwrap().count(), 0);
    let remote = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    input["url"] = json!(format!(
        "https://{}/team/repo.git",
        remote.local_addr().unwrap()
    ));
    let clone_request = client
        .post(format!("{address}/api/repositories/clone"))
        .bearer_auth(&token)
        .header("X-Local-Request", "1")
        .json(&input);
    let cloning = tokio::spawn(async move { clone_request.send().await.unwrap() });
    let (_connection, _) = tokio::time::timeout(Duration::from_secs(15), remote.accept())
        .await
        .unwrap()
        .unwrap();
    let cancel = client
        .post(format!(
            "{address}/api/repositories/clone/synthetic-http-clone/cancel"
        ))
        .bearer_auth(&token)
        .header("X-Local-Request", "1")
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert!(cancel.status().is_success());
    let response = tokio::time::timeout(Duration::from_secs(15), cloning)
        .await
        .unwrap()
        .unwrap();
    assert!(!response.status().is_success());
    assert_eq!(std::fs::read_dir(&parent).unwrap().count(), 0);
    let state: serde_json::Value = client
        .get(format!("{address}/api/state"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(state["projects"].as_array().unwrap().is_empty());
    assert!(!root.join("outside").exists());
    client
        .post(format!("{address}/api/shutdown"))
        .bearer_auth(&token)
        .header("X-Local-Request", "1")
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    server.await.unwrap().unwrap();
}
