use atelier_app::{
    http,
    infrastructure::{database::Database, prototype::PrototypeDirectory, session},
};
use atelier_core::contracts::Card;
use serde_json::{Value, json};
use std::{path::Path, time::Duration};

async fn start(
    data: &Path,
    assets: &Path,
) -> (
    String,
    String,
    tokio::task::JoinHandle<atelier_app::Result<()>>,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(http::serve(listener, data.to_owned(), assets.to_owned()));
    let client = reqwest::Client::new();
    for _ in 0..100 {
        if let Ok(token) = session::read(data)
            && client
                .get(format!("{address}/api/state"))
                .bearer_auth(&token)
                .send()
                .await
                .is_ok_and(|response| response.status().is_success())
        {
            return (address, token, task);
        }
        assert!(!task.is_finished(), "Server startup failed");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    task.abort();
    panic!("Server did not become ready");
}

#[tokio::test]
async fn brainstorms_remain_in_their_own_collection_after_restart() {
    let data = tempfile::tempdir().unwrap();
    let assets = tempfile::tempdir().unwrap();
    std::fs::write(assets.path().join("index.html"), "synthetic").unwrap();
    let client = reqwest::Client::new();
    let (address, token, server) = start(data.path(), assets.path()).await;
    let idea: Value = client
        .post(format!("{address}/api/brainstorms"))
        .bearer_auth(&token)
        .header("x-local-request", "1")
        .json(&json!({"title":"Synthetic persistent idea"}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let first_token = token.clone();
    stop(&address, &token, server).await;
    let (address, token, server) = start(data.path(), assets.path()).await;
    assert_ne!(token, first_token);
    let state: Value = client
        .get(format!("{address}/api/state"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(state["brainstorms"], json!([idea]));
    assert_eq!(state["cards"], json!([]));
    stop(&address, &token, server).await;
}

#[tokio::test]
async fn invalid_text_and_field_types_never_create_cards() {
    let data = tempfile::tempdir().unwrap();
    let assets = tempfile::tempdir().unwrap();
    std::fs::write(assets.path().join("index.html"), "synthetic").unwrap();
    let (address, token, task) = start(data.path(), assets.path()).await;
    let client = reqwest::Client::new();
    let state = || {
        client
            .get(format!("{address}/api/state"))
            .bearer_auth(&token)
    };
    let before: Value = state().send().await.unwrap().json().await.unwrap();
    for body in [
        r#"{"title":"\ud800"}"#,
        r#"{"title":"Synthetic","future":{"text":"\udc00"}}"#,
        r#"{"title":true}"#,
        r#"{"title":"Synthetic","repo":false}"#,
        r#"{"title":"Synthetic","agent":[]}"#,
    ] {
        let response = client
            .post(format!("{address}/api/cards"))
            .bearer_auth(&token)
            .header("x-local-request", "1")
            .header("content-type", "application/json")
            .body(body)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 400, "{body}");
    }
    let after: Value = state().send().await.unwrap().json().await.unwrap();
    assert_eq!(before, after);
    stop(&address, &token, task).await;
}

#[tokio::test]
async fn unavailable_credentials_do_not_block_local_cards_or_time_and_are_never_erased() {
    use atelier_app::infrastructure::{store::Documents, token_vault};
    let data = tempfile::tempdir().unwrap();
    let assets = tempfile::tempdir().unwrap();
    std::fs::write(assets.path().join("index.html"), "synthetic").unwrap();
    let directory = PrototypeDirectory::open(data.path()).unwrap();
    let database = directory.database().unwrap();
    database
        .call(|store| {
            store.save_settings(&json!({"processConnectors":[],"baseBranch":"local"}))?;
            // Invalid envelopes fail before any operating-system credential lookup.
            token_vault::save_encrypted(store, Some("synthetic-unavailable-envelope"))
        })
        .await
        .unwrap();
    database.close().await.unwrap();
    drop(database);
    drop(directory);
    let (address, token, task) = start(data.path(), assets.path()).await;
    let client = reqwest::Client::new();
    let post = |path: &str| {
        client
            .post(format!("{address}{path}"))
            .bearer_auth(&token)
            .header("x-local-request", "1")
    };
    let response = post("/api/cards")
        .json(&json!({"title":"Synthetic local task"}))
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success());
    let card: Card = response.json().await.unwrap();
    let response = post("/api/time").json(&json!({"cardId":card.id,"seconds":60,"date":"2026-10-30","startTime":"09:00:00","description":"Synthetic local time"})).send().await.unwrap();
    assert!(response.status().is_success());
    let board: Value = client
        .get(format!("{address}/api/state"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(board["cards"].as_array().unwrap().len(), 1);
    assert_eq!(board["times"].as_array().unwrap().len(), 1);
    assert_eq!(board["settings"]["tokenSet"], Value::Null);
    assert_eq!(
        board["settings"]["credentialError"],
        "message.credentials_unavailable_local_data_remain_available"
    );
    let response = client
        .put(format!("{address}/api/settings"))
        .bearer_auth(&token)
        .header("x-local-request", "1")
        .json(&json!({"baseBranch":"changed"}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 503);
    stop(&address, &token, task).await;
    let directory = PrototypeDirectory::open(data.path()).unwrap();
    let database = directory.database().unwrap();
    database
        .call(move |store| {
            assert_eq!(
                token_vault::read_encrypted(store)?.as_deref(),
                Some("synthetic-unavailable-envelope")
            );
            assert_eq!(store.settings::<Value>()?["baseBranch"], "local");
            assert!(
                store
                    .document::<Card>(Documents::Cards, &card.id)?
                    .is_some()
            );
            Ok(())
        })
        .await
        .unwrap();
    database.close().await.unwrap();
}

#[tokio::test]
async fn folder_selection_requires_the_local_session_and_a_native_launcher() {
    let data = tempfile::tempdir().unwrap();
    let assets = tempfile::tempdir().unwrap();
    std::fs::write(assets.path().join("index.html"), "synthetic").unwrap();
    let (address, token, task) = start(data.path(), assets.path()).await;
    let client = reqwest::Client::new();
    let url = format!("{address}/api/folders/choose");
    assert_eq!(
        client
            .post(&url)
            .json(&json!({"confirmed":true}))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    assert_eq!(
        client
            .post(&url)
            .bearer_auth(&token)
            .json(&json!({"confirmed":true}))
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    let result = client
        .post(&url)
        .bearer_auth(&token)
        .header("x-local-request", "1")
        .json(&json!({"confirmed":true}))
        .send()
        .await
        .unwrap();
    assert_eq!(result.status(), 409);
    assert!(
        result
            .text()
            .await
            .unwrap()
            .contains("message.the_folder_picker_is_available_through_the_native_launcher")
    );
    stop(&address, &token, task).await;
}

#[tokio::test]
async fn general_settings_reject_stale_fields_and_preserve_independent_edits_after_restart() {
    let data = tempfile::tempdir().unwrap();
    let assets = tempfile::tempdir().unwrap();
    std::fs::write(assets.path().join("index.html"), "synthetic").unwrap();
    let (address, token, task) = start(data.path(), assets.path()).await;
    let client = reqwest::Client::new();
    let edit = || {
        client
            .put(format!("{address}/api/settings"))
            .bearer_auth(&token)
            .header("x-local-request", "1")
    };
    let first = edit()
        .json(&json!({"baseBranch":"main", "expected":{"baseBranch":"HEAD"}}))
        .send();
    let second = edit()
        .json(&json!({"baseBranch":"develop", "expected":{"baseBranch":"HEAD"}}))
        .send();
    let (first, second) = tokio::join!(first, second);
    let mut statuses = [
        first.unwrap().status().as_u16(),
        second.unwrap().status().as_u16(),
    ];
    statuses.sort();
    assert_eq!(statuses, [200, 409]);
    let response = edit()
        .json(&json!({"agent":"claude", "expected":{"agent":"codex"}}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let saved: Value = response.json().await.unwrap();
    assert_eq!(saved["agent"], "claude");
    assert!(matches!(
        saved["baseBranch"].as_str(),
        Some("main" | "develop")
    ));
    for body in [
        json!({"baseBranch":"lost", "expected":{}}),
        json!({"baseBranch":"lost", "expected":{"unknown":"HEAD"}}),
        json!({"expected":{"agent":"claude"}}),
        json!({"agent":"codex", "expected":{"agent":null}}),
        json!({"agent":"codex", "expected":null}),
    ] {
        assert_eq!(edit().json(&body).send().await.unwrap().status(), 400);
    }
    stop(&address, &token, task).await;
    let (address, token, task) = start(data.path(), assets.path()).await;
    let board: Value = client
        .get(format!("{address}/api/state"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(board["settings"], saved);
    stop(&address, &token, task).await;
}

#[cfg(windows)]
#[tokio::test]
async fn worktree_intents_survive_restart_and_block_destructive_card_and_backup_actions() {
    use atelier_app::infrastructure::{process_tree, store::Documents};
    use atelier_core::application::worktrees::ServiceWorktrees;
    use atelier_core::contracts::{Worktree, WorktreeState};
    async fn git(repo: &Path, args: &[&str]) {
        let mut command = tokio::process::Command::new("git");
        command
            .current_dir(repo)
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
        let output = process_tree::capture(&mut command, &[], Duration::from_secs(15), 1024 * 1024)
            .await
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let data = tempfile::tempdir().unwrap();
    let assets = tempfile::tempdir().unwrap();
    let (address, token, task) = start(data.path(), assets.path()).await;
    let repo = std::fs::canonicalize(data.path())
        .unwrap()
        .join("repositories/worktree-http");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-b", "main"]).await;
    std::fs::write(repo.join("source.txt"), "source\n").unwrap();
    std::fs::write(repo.join(".gitignore"), "ignored.txt\n").unwrap();
    git(&repo, &["add", "."]).await;
    git(&repo, &["commit", "-qm", "Synthetic baseline"]).await;
    let service_repo = repo.with_file_name("worktree-service-http");
    std::fs::create_dir_all(&service_repo).unwrap();
    git(&service_repo, &["init", "-b", "main"]).await;
    std::fs::write(service_repo.join("source.txt"), "service source\n").unwrap();
    git(&service_repo, &["add", "."]).await;
    git(
        &service_repo,
        &["commit", "-qm", "Synthetic service baseline"],
    )
    .await;
    let client = reqwest::Client::new();
    let project: Value = client
        .post(format!("{address}/api/projects"))
        .bearer_auth(&token)
        .header("x-local-request", "1")
        .json(&json!({"name":"Synthetic services", "repo":repo}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let project_id = project["id"].as_str().unwrap();
    client.patch(format!("{address}/api/projects/{project_id}")).bearer_auth(&token).header("x-local-request","1").json(&json!({"frontRepo":service_repo,"backRepo":service_repo,"frontCommand":"synthetic-front","backCommand":"synthetic-back"})).send().await.unwrap().error_for_status().unwrap();
    let card: Card = client
        .post(format!("{address}/api/cards"))
        .bearer_auth(&token)
        .header("x-local-request", "1")
        .json(&json!({"title":"Synthetic worktree", "repo":repo}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let id = card.id;
    let post = |suffix: &str, body: Value| {
        client
            .post(format!("{address}/api/cards/{id}{suffix}"))
            .bearer_auth(&token)
            .header("x-local-request", "1")
            .json(&body)
    };
    let created: Card = post(
        "/worktree",
        json!({"branch":"feature/http-synthetic","base":"main"}),
    )
    .send()
    .await
    .unwrap()
    .error_for_status()
    .unwrap()
    .json()
    .await
    .unwrap();
    let worktree = created.fields.get::<Worktree>("worktree").unwrap().unwrap();
    let services = created
        .fields
        .get::<ServiceWorktrees>("serviceWorktrees")
        .unwrap()
        .unwrap();
    assert_eq!(services["front"], services["back"]);
    assert_eq!(services["front"].branch, worktree.branch);
    assert_eq!(services["front"].state, WorktreeState::Ready);
    assert_eq!(
        client
            .patch(format!("{address}/api/projects/{project_id}"))
            .bearer_auth(&token)
            .header("x-local-request", "1")
            .json(&json!({"frontRepo":""}))
            .send()
            .await
            .unwrap()
            .status(),
        409
    );
    assert_eq!(worktree.state, WorktreeState::Ready);
    assert_eq!(
        post("/worktree/remove", json!({}))
            .send()
            .await
            .unwrap()
            .status(),
        400
    );
    assert_eq!(
        client
            .delete(format!("{address}/api/cards/{id}"))
            .bearer_auth(&token)
            .header("x-local-request", "1")
            .json(&json!({}))
            .send()
            .await
            .unwrap()
            .status(),
        409
    );
    stop(&address, &token, task).await;
    // Simulate a process interruption after Git succeeds but before the ready-state transaction.
    let database = Database::open(&data.path().join("kanban.sqlite")).unwrap();
    let key = id.clone();
    database
        .call(move |store| {
            let mut card = store.document::<Card>(Documents::Cards, &key)?.unwrap();
            let mut intent = card.fields.get::<Worktree>("worktree")?.unwrap();
            intent.state = WorktreeState::Creating;
            card.fields.set("worktree", &intent)?;
            let mut services = card
                .fields
                .get::<ServiceWorktrees>("serviceWorktrees")?
                .unwrap();
            for service in services.values_mut() {
                service.state = WorktreeState::Creating;
            }
            card.fields.set("serviceWorktrees", &services)?;
            store.put(Documents::Cards, &card)
        })
        .await
        .unwrap();
    database.close().await.unwrap();
    drop(database);
    let (address, token, task) = start(data.path(), assets.path()).await;
    assert_eq!(
        client
            .post(format!("{address}/api/backups"))
            .bearer_auth(&token)
            .header("x-local-request", "1")
            .json(&json!({}))
            .send()
            .await
            .unwrap()
            .status(),
        409
    );
    assert_eq!(
        client
            .get(format!("{address}/api/cards/{id}/files"))
            .bearer_auth(&token)
            .send()
            .await
            .unwrap()
            .status(),
        409
    );
    let post = |suffix: &str, body: Value| {
        client
            .post(format!("{address}/api/cards/{id}{suffix}"))
            .bearer_auth(&token)
            .header("x-local-request", "1")
            .json(&body)
    };
    let resumed: Card = post("/worktree", json!({}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let resumed = resumed.fields.get::<Worktree>("worktree").unwrap().unwrap();
    assert_eq!(resumed, worktree);
    std::fs::write(
        Path::new(&services["front"].path).join("source.txt"),
        "service edit",
    )
    .unwrap();
    assert_eq!(
        client
            .get(format!("{address}/api/cards/{id}/files"))
            .bearer_auth(&token)
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    std::fs::write(
        Path::new(&worktree.path).join("ignored.txt"),
        "preserved ignored content",
    )
    .unwrap();
    assert_eq!(
        post("/worktree/remove", json!({"confirmed":true}))
            .send()
            .await
            .unwrap()
            .status(),
        400
    );
    assert!(Path::new(&worktree.path).join("ignored.txt").exists());
    assert_eq!(
        std::fs::read_to_string(Path::new(&services["front"].path).join("source.txt")).unwrap(),
        "service edit"
    );
    let restored: Card = post("/worktree", json!({}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        restored
            .fields
            .get::<Worktree>("worktree")
            .unwrap()
            .unwrap()
            .state,
        WorktreeState::Ready
    );
    let removed: Card = post(
        "/worktree/remove",
        json!({"confirmed":true,"discardChanges":true}),
    )
    .send()
    .await
    .unwrap()
    .error_for_status()
    .unwrap()
    .json()
    .await
    .unwrap();
    assert!(
        removed
            .fields
            .get::<Worktree>("worktree")
            .unwrap()
            .is_none()
    );
    assert!(!Path::new(&worktree.path).exists());
    assert!(!Path::new(&services["front"].path).exists());
    assert!(
        removed
            .fields
            .get::<ServiceWorktrees>("serviceWorktrees")
            .unwrap()
            .is_none()
    );
    assert_eq!(
        std::fs::read_to_string(service_repo.join("source.txt")).unwrap(),
        "service source\n"
    );
    git(
        &service_repo,
        &["show-ref", "--verify", "refs/heads/feature/http-synthetic"],
    )
    .await;
    git(
        &repo,
        &["show-ref", "--verify", "refs/heads/feature/http-synthetic"],
    )
    .await;
    assert_eq!(
        std::fs::read_to_string(repo.join("source.txt")).unwrap(),
        "source\n"
    );
    stop(&address, &token, task).await;
}

#[tokio::test]
async fn service_settings_guard_concurrent_edits_and_preserve_linked_projects() {
    let data = tempfile::tempdir().unwrap();
    let assets = tempfile::tempdir().unwrap();
    let (address, token, task) = start(data.path(), assets.path()).await;
    let client = reqwest::Client::new();
    let mut projects = Vec::<Value>::new();
    for name in ["service-main", "service-linked"] {
        let repo = data.path().join("repositories").join(name);
        std::fs::create_dir_all(&repo).unwrap();
        projects.push(
            client
                .post(format!("{address}/api/projects"))
                .bearer_auth(&token)
                .header("x-local-request", "1")
                .json(&json!({"name":name,"repo":repo}))
                .send()
                .await
                .unwrap()
                .error_for_status()
                .unwrap()
                .json()
                .await
                .unwrap(),
        );
    }
    let id = projects[0]["id"].as_str().unwrap();
    let linked = projects[1]["id"].as_str().unwrap();
    let edit = || {
        client
            .patch(format!("{address}/api/projects/{id}"))
            .bearer_auth(&token)
            .header("x-local-request", "1")
    };
    let saved: Value = edit().json(&json!({
        "frontCommand":"synthetic-front --port {port}", "frontDirectory":"client", "frontPort":"4300",
        "backRepo":projects[1]["repo"], "backCommand":"synthetic-back --port {port}", "backPort":"0", "linkedProjectId":linked,
        "expected":{"frontCommand":"","frontDirectory":"","frontPort":"0","backRepo":"","backCommand":"","backPort":"0","linkedProjectId":""}
    })).send().await.unwrap().error_for_status().unwrap().json().await.unwrap();
    assert_eq!(saved["frontPort"], 4300);
    assert_eq!(saved["backPort"], 0);
    assert_eq!(saved["backRepo"], projects[1]["repo"]);
    assert_eq!(
        edit()
            .json(&json!({"frontPort":"4301","expected":{"frontPort":"0"}}))
            .send()
            .await
            .unwrap()
            .status(),
        409
    );
    for invalid in [
        json!({"frontCommand":"overwrite","expected":{}}),
        json!({"backDirectory":"../outside","expected":{"backDirectory":""}}),
        json!({"frontCommand":"no placeholder","expected":{"frontCommand":"synthetic-front --port {port}"}}),
        json!({"backPort":"65536","expected":{"backPort":"0"}}),
        json!({"linkedProjectId":id,"expected":{"linkedProjectId":linked}}),
        json!({"linkedProjectId":"missing","expected":{"linkedProjectId":linked}}),
    ] {
        assert_eq!(edit().json(&invalid).send().await.unwrap().status(), 400);
    }
    assert_eq!(
        client
            .delete(format!("{address}/api/projects/{linked}"))
            .bearer_auth(&token)
            .header("x-local-request", "1")
            .json(&json!({"archiveTasks":true}))
            .send()
            .await
            .unwrap()
            .status(),
        409
    );
    let renamed: Value = edit()
        .json(&json!({"name":"independent rename","expected":{"name":"service-main"}}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(renamed["frontCommand"], saved["frontCommand"]);
    assert_eq!(renamed["linkedProjectId"], saved["linkedProjectId"]);
    let id = id.to_owned();
    stop(&address, &token, task).await;
    let (address, token, task) = start(data.path(), assets.path()).await;
    let board: Value = client
        .get(format!("{address}/api/state"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        board["projects"]
            .as_array()
            .unwrap()
            .iter()
            .find(|project| project["id"] == id)
            .unwrap(),
        &renamed
    );
    stop(&address, &token, task).await;
}

#[tokio::test]
async fn project_field_guards_reject_stale_relocations_before_creating_backups() {
    let data = tempfile::tempdir().unwrap();
    let assets = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    let (address, token, task) = start(data.path(), assets.path()).await;
    let client = reqwest::Client::new();
    let project: Value = client
        .post(format!("{address}/api/projects"))
        .bearer_auth(&token)
        .header("x-local-request", "1")
        .json(&json!({"name":"Original", "repo":repo.path()}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let id = project["id"].as_str().unwrap().to_owned();
    let edit = || {
        client
            .patch(format!("{address}/api/projects/{id}"))
            .bearer_auth(&token)
            .header("x-local-request", "1")
    };
    let a = edit()
        .json(&json!({"name":"First", "expected":{"name":"Original"}}))
        .send();
    let b = edit()
        .json(&json!({"name":"Second", "expected":{"name":"Original"}}))
        .send();
    let (a, b) = tokio::join!(a, b);
    let mut statuses = [a.unwrap().status().as_u16(), b.unwrap().status().as_u16()];
    statuses.sort();
    assert_eq!(statuses, [200, 409]);
    let response: Value = edit()
        .json(&json!({"agent":"claude", "expected":{"agent":""}}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(response["agent"], "claude");
    assert!(matches!(
        response["name"].as_str(),
        Some("First" | "Second")
    ));
    let replacement = tempfile::tempdir().unwrap();
    let refused = edit().json(&json!({"repo":replacement.path(),"name":"Overwrite", "expected":{"repo":project["repo"],"name":"Original"}})).send().await.unwrap();
    assert_eq!(refused.status(), 409);
    for body in [
        json!({"name":"Lost","expected":{}}),
        json!({"expected":{"name":"Original"}}),
        json!({"agent":"codex","expected":null}),
    ] {
        assert_eq!(edit().json(&body).send().await.unwrap().status(), 400);
    }
    stop(&address, &token, task).await;
    assert!(
        atelier_app::infrastructure::backups::BackupDirectory::open(data.path())
            .unwrap()
            .list()
            .unwrap()
            .is_empty()
    );
    let database = Database::open(&data.path().join("kanban.sqlite")).unwrap();
    let saved: Value = database
        .call(move |store| {
            store.document(atelier_app::infrastructure::store::Documents::Projects, &id)
        })
        .await
        .unwrap()
        .unwrap();
    assert_eq!(saved, response);
    database.close().await.unwrap();
}

async fn stop(address: &str, token: &str, task: tokio::task::JoinHandle<atelier_app::Result<()>>) {
    assert!(
        reqwest::Client::new()
            .post(format!("{address}/api/shutdown"))
            .bearer_auth(token)
            .header("x-local-request", "1")
            .json(&json!({}))
            .send()
            .await
            .unwrap()
            .status()
            .is_success()
    );
    tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn guarded_task_edits_reject_lost_updates_and_merge_independent_fields() {
    let data = tempfile::tempdir().unwrap();
    let assets = tempfile::tempdir().unwrap();
    std::fs::write(assets.path().join("index.html"), "synthetic").unwrap();
    let (address, token, task) = start(data.path(), assets.path()).await;
    let client = reqwest::Client::new();
    let initial: atelier_core::contracts::Board = client
        .get(format!("{address}/api/state"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let card: Card = client
        .post(format!("{address}/api/cards"))
        .bearer_auth(&token)
        .header("x-local-request", "1")
        .json(&json!({"title":"Original"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let edit = || {
        client
            .patch(format!("{address}/api/cards/{}", card.id))
            .bearer_auth(&token)
            .header("x-local-request", "1")
    };
    let (first, second) = tokio::join!(
        edit()
            .json(&json!({"title":"First", "expected":{"title":"Original"}}))
            .send(),
        edit()
            .json(&json!({"title":"Second", "expected":{"title":"Original"}}))
            .send()
    );
    let mut statuses = [
        first.unwrap().status().as_u16(),
        second.unwrap().status().as_u16(),
    ];
    statuses.sort();
    assert_eq!(statuses, [200, 409]);
    let (description, instructions) = tokio::join!(
        edit()
            .json(&json!({"description":"Description", "expected":{"description":""}}))
            .send(),
        edit()
            .json(&json!({"instructions":"Instructions", "expected":{"instructions":""}}))
            .send()
    );
    assert_eq!(description.unwrap().status(), 200);
    assert_eq!(instructions.unwrap().status(), 200);
    for body in [
        json!({"title":"Bad", "expected":{}}),
        json!({"title":"Bad", "expected":true}),
        json!({"title":"Bad", "expected":{"title":"First", "extra":0}}),
    ] {
        assert_eq!(edit().json(&body).send().await.unwrap().status(), 400);
    }
    let updated: atelier_core::contracts::Board = client
        .get(format!("{address}/api/state"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(updated.revision > initial.revision);
    stop(&address, &token, task).await;
    let database = Database::open(&data.path().join("kanban.sqlite")).unwrap();
    let cards = database.cards().await.unwrap();
    assert_eq!(cards.len(), 1);
    assert!(cards[0].title == "First" || cards[0].title == "Second");
    assert!(
        cards[0]
            .fields
            .string_equals("description", "Description")
            .unwrap()
    );
    assert!(
        cards[0]
            .fields
            .string_equals("instructions", "Instructions")
            .unwrap()
    );
    database.close().await.unwrap();
}

#[tokio::test]
async fn public_assets_are_allowlisted_captured_once_and_never_expose_private_paths() {
    let data = tempfile::tempdir().unwrap();
    let assets = tempfile::tempdir().unwrap();
    for name in [
        "index.html",
        "atelier-loader.js",
        "atelier-ui-synthetic.js",
        "atelier-ui-synthetic.wasm",
        "style-synthetic.css",
    ] {
        let path = assets.path().join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, format!("synthetic {name}")).unwrap();
    }
    let (address, token, task) = start(data.path(), assets.path()).await;
    std::fs::write(data.path().join("private.txt"), "private synthetic bytes").unwrap();
    let client = reqwest::Client::new();
    std::fs::write(assets.path().join("index.html"), "replaced synthetic bytes").unwrap();
    let response = client.get(&address).send().await.unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(
        response.headers()["content-type"],
        "text/html; charset=utf-8"
    );
    assert_eq!(response.text().await.unwrap(), "synthetic index.html");
    for path in [
        "/overview",
        "/board",
        "/brainstorm",
        "/agents",
        "/processes",
        "/worktrees",
        "/time",
        "/archives",
        "/settings?section=tickets",
        "/settings?section=time",
        "/updates",
        "/tasks/synthetic-card",
    ] {
        let response = client.get(format!("{address}{path}")).send().await.unwrap();
        assert_eq!(response.status(), 200, "{path}");
        assert_eq!(response.text().await.unwrap(), "synthetic index.html");
    }
    for (name, mime) in [
        ("atelier-loader.js", "text/javascript; charset=utf-8"),
        ("atelier-ui-synthetic.js", "text/javascript; charset=utf-8"),
        ("atelier-ui-synthetic.wasm", "application/wasm"),
        ("style-synthetic.css", "text/css; charset=utf-8"),
    ] {
        let response = client
            .get(format!("{address}/{name}"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(response.headers()["content-type"], mime);
        assert_eq!(response.text().await.unwrap(), format!("synthetic {name}"));
    }
    for path in [
        "/unknown-page",
        "/tasks/synthetic-card/private.txt",
        "/private.txt",
        "/lib/store.mjs",
        "/data/local-session",
        "/app.js",
        "/local-session.mjs",
        "/views/card.mjs",
        "/lib/domain/tasks.mjs",
    ] {
        assert_eq!(
            client
                .get(format!("{address}{path}"))
                .send()
                .await
                .unwrap()
                .status(),
            404
        );
    }
    assert_eq!(
        client
            .get(format!("{address}/api/agents"))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    let response = client
        .get(format!("{address}/api/agents"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 404);
    stop(&address, &token, task).await;
}

#[cfg(windows)]
#[tokio::test]
async fn backup_http_guards_restore_rescue_credentials_and_queue_in_one_transaction() {
    use atelier_app::infrastructure::token_vault;
    for fault in [false, true] {
        let data = tempfile::tempdir().unwrap();
        let assets = tempfile::tempdir().unwrap();
        let directory = PrototypeDirectory::open(data.path()).unwrap();
        let database = Database::open(&directory.path.join("kanban.sqlite")).unwrap();
        database
            .call(|store| {
                store.save_settings(
                    &json!({"taskQueue":{"ids":[],"paused":true,"future":"preserved"}}),
                )
            })
            .await
            .unwrap();
        database.close().await.unwrap();
        drop(database);
        drop(directory);
        if fault {
            rusqlite::Connection::open(data.path().join("kanban.sqlite")).unwrap().execute_batch("CREATE TRIGGER refuse_queue BEFORE UPDATE ON settings WHEN json_extract(NEW.data,'$.taskQueue.message')='message.queue_paused_the_active_task_may_finish' BEGIN SELECT RAISE(ABORT, 'private synthetic queue failure'); END;").unwrap();
        }
        let (address, token, task) = start(data.path(), assets.path()).await;
        let client = reqwest::Client::new();
        let request = |method, path: &str| {
            client
                .request(method, format!("{address}{path}"))
                .bearer_auth(&token)
                .header("x-local-request", "1")
        };
        let post = |path: &str| request(reqwest::Method::POST, path);
        assert_eq!(
            request(reqwest::Method::PUT, "/api/settings")
                .json(&json!({"token":"synthetic-old","baseBranch":"saved"}))
                .send()
                .await
                .unwrap()
                .status(),
            200
        );
        let card: Value = post("/api/cards")
            .json(&json!({"title":"Saved task"}))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let id = card["id"].as_str().unwrap();
        let saved: Value = post("/api/backups")
            .json(&json!({}))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let name = saved["name"].as_str().unwrap();
        let catalog: Value = request(reqwest::Method::GET, "/api/backups")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(catalog["backups"].as_array().unwrap().len(), 1);
        for body in [
            json!({"name":name,"confirmed":"true"}),
            json!({"name":"../kanban.sqlite","confirmed":true}),
            json!({"name":null,"confirmed":true}),
        ] {
            assert_eq!(
                post("/api/backups/restore")
                    .json(&body)
                    .send()
                    .await
                    .unwrap()
                    .status(),
                400
            );
        }
        assert_eq!(
            request(reqwest::Method::PATCH, &format!("/api/cards/{id}"))
                .json(&json!({"title":"Current task"}))
                .send()
                .await
                .unwrap()
                .status(),
            200
        );
        assert_eq!(
            request(reqwest::Method::PUT, "/api/settings")
                .json(&json!({"token":"synthetic-current","baseBranch":"current"}))
                .send()
                .await
                .unwrap()
                .status(),
            200
        );
        let timer: Value = post("/api/time").json(&json!({"cardId":id,"start":true,"seconds":60,"date":"2026-10-24","startTime":"09:00:00","description":"Synthetic"})).send().await.unwrap().json().await.unwrap();
        let timer_id = timer["id"].as_str().unwrap();
        for path in ["/api/backups", "/api/backups/restore"] {
            let response = post(path).json(&json!({})).send().await.unwrap();
            assert_eq!(response.status(), 409);
            assert!(response.text().await.unwrap().contains(
                "message.pause_the_queue_stop_agents_and_timers_then_complete_or_verify_t"
            ));
        }
        assert_eq!(
            post(&format!("/api/time/{timer_id}/stop"))
                .json(&json!({}))
                .send()
                .await
                .unwrap()
                .status(),
            200
        );
        let restored = post("/api/backups/restore")
            .json(&json!({"name":name,"confirmed":true}))
            .send()
            .await
            .unwrap();
        assert_eq!(restored.status(), if fault { 500 } else { 200 });
        let restored: Value = restored.json().await.unwrap();
        let state: Value = request(reqwest::Method::GET, "/api/state")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        if fault {
            assert_eq!(
                restored,
                json!({"error":"message.the_backup_operation_could_not_be_completed"})
            );
            assert_eq!(state["cards"][0]["title"], "Current task");
            assert_eq!(state["settings"]["baseBranch"], "current");
            assert_eq!(state["settings"]["tokenSet"], true);
            assert!(state["settings"].get("credentialsReset").is_none());
            assert!(state["settings"]["taskQueue"].get("message").is_none());
            assert_eq!(state["times"].as_array().unwrap().len(), 1);
        } else {
            assert_eq!(restored["restored"], name);
            assert_ne!(restored["rescue"], name);
            assert_eq!(state["cards"][0]["title"], "Saved task");
            assert_eq!(state["settings"]["baseBranch"], "saved");
            assert_eq!(state["settings"]["tokenSet"], false);
            assert_eq!(state["settings"]["credentialsReset"], true);
            assert_eq!(
                state["settings"]["taskQueue"],
                json!({"ids":[],"paused":true,"future":"preserved","message":"message.queue_paused_the_active_task_may_finish"})
            );
            assert_eq!(state["times"].as_array().unwrap().len(), 0);
            let rescue = rusqlite::Connection::open_with_flags(
                data.path()
                    .join("backups")
                    .join(restored["rescue"].as_str().unwrap()),
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )
            .unwrap();
            let source: String = rescue
                .query_row("SELECT data FROM cards WHERE id=?", [id], |row| row.get(0))
                .unwrap();
            assert_eq!(
                serde_json::from_str::<Value>(&source).unwrap()["title"],
                "Current task"
            );
        }
        stop(&address, &token, task).await;
        let (address, token, task) = start(data.path(), assets.path()).await;
        let state: Value = client
            .get(format!("{address}/api/state"))
            .bearer_auth(&token)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(state["settings"]["tokenSet"], fault);
        stop(&address, &token, task).await;
        let database = Database::open(&data.path().join("kanban.sqlite")).unwrap();
        let encrypted = database.call(token_vault::read_encrypted).await.unwrap();
        assert_eq!(encrypted.is_some(), fault);
        if let Some(encrypted) = encrypted {
            assert!(
                session::protect(&encrypted, true)
                    .unwrap()
                    .contains("synthetic-current")
            );
        }
        database.close().await.unwrap();
    }
}

#[cfg(windows)]
#[tokio::test]
async fn provider_profiles_preserve_keys_references_and_rollback_on_storage_failure() {
    use atelier_app::infrastructure::token_vault;
    use atelier_core::document::Document;
    for fault in [false, true] {
        let data = tempfile::tempdir().unwrap();
        let assets = tempfile::tempdir().unwrap();
        let directory = PrototypeDirectory::open(data.path()).unwrap();
        let database = Database::open(&directory.path.join("kanban.sqlite")).unwrap();
        database
            .call(|store| store.save_settings(&json!({"future":"keep"})))
            .await
            .unwrap();
        database.close().await.unwrap();
        drop(database);
        drop(directory);
        if fault {
            rusqlite::Connection::open(data.path().join("kanban.sqlite")).unwrap().execute_batch("CREATE TRIGGER refuse_profiles BEFORE UPDATE ON settings BEGIN SELECT RAISE(ABORT, 'synthetic failure'); END;").unwrap();
        }
        let (address, token, task) = start(data.path(), assets.path()).await;
        let client = reqwest::Client::new();
        let request = |method, path: &str| {
            client
                .request(method, format!("{address}{path}"))
                .bearer_auth(&token)
                .header("x-local-request", "1")
        };
        let response = request(reqwest::Method::POST, "/api/agent-providers").header("content-type", "application/json").body(r#"{"id":"api","name":"Synthetic API","type":"codex","auth":"api","apiKey":"synthetic-key","model":"custom/model","expected":null}"#).send().await.unwrap();
        if fault {
            assert_eq!(response.status(), 500);
            let catalog: Value = request(reqwest::Method::GET, "/api/agent-providers")
                .send()
                .await
                .unwrap()
                .json()
                .await
                .unwrap();
            assert_eq!(catalog.as_array().unwrap().len(), 2);
            stop(&address, &token, task).await;
            let database = Database::open(&data.path().join("kanban.sqlite")).unwrap();
            assert!(
                database
                    .call(token_vault::read_encrypted)
                    .await
                    .unwrap()
                    .is_none()
            );
            assert_eq!(
                database
                    .call(|store| store.settings::<Value>())
                    .await
                    .unwrap(),
                json!({"future":"keep"})
            );
            database.close().await.unwrap();
            continue;
        }
        assert_eq!(response.status(), 200);
        let response = response.text().await.unwrap();
        assert!(!response.contains("synthetic-key"));
        let mut catalog: Vec<Document> = serde_json::from_str(&response).unwrap();
        assert_eq!(catalog.len(), 3);
        assert_eq!(catalog[2].get::<bool>("apiKeySet").unwrap(), Some(true));
        let update = json!({"id":"api","name":"Renamed synthetic API","type":"codex","auth":"api","model":"custom/model","expected":catalog[2]});
        let saved = request(reqwest::Method::POST, "/api/agent-providers")
            .json(&update)
            .send()
            .await
            .unwrap();
        assert_eq!(saved.status(), 200);
        catalog = saved.json().await.unwrap();
        assert_eq!(
            request(reqwest::Method::POST, "/api/agent-providers")
                .json(&update)
                .send()
                .await
                .unwrap()
                .status(),
            409
        );
        assert_eq!(
            request(reqwest::Method::DELETE, "/api/agent-providers/api")
                .json(&json!({"expected":update["expected"]}))
                .send()
                .await
                .unwrap()
                .status(),
            409
        );
        assert_eq!(
            request(reqwest::Method::DELETE, "/api/agent-providers/api")
                .json(&json!({}))
                .send()
                .await
                .unwrap()
                .status(),
            400
        );
        assert_eq!(
            request(reqwest::Method::PUT, "/api/settings")
                .json(&json!({"agent":"api"}))
                .send()
                .await
                .unwrap()
                .status(),
            200
        );
        assert_eq!(
            request(reqwest::Method::DELETE, "/api/agent-providers/api")
                .json(&json!({"expected":catalog[2]}))
                .send()
                .await
                .unwrap()
                .status(),
            409
        );
        let card: Value = request(reqwest::Method::POST, "/api/cards")
            .json(&json!({"title":"provider reference"}))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(card["agent"], "api");
        assert_eq!(
            request(reqwest::Method::PUT, "/api/settings")
                .json(&json!({"agent":"codex"}))
                .send()
                .await
                .unwrap()
                .status(),
            200
        );
        assert_eq!(
            request(reqwest::Method::DELETE, "/api/agent-providers/api")
                .json(&json!({"expected":catalog[2]}))
                .send()
                .await
                .unwrap()
                .status(),
            409
        );
        let changed = request(
            reqwest::Method::PATCH,
            &format!("/api/cards/{}", card["id"].as_str().unwrap()),
        )
        .json(&json!({"agent":"codex"}))
        .send()
        .await
        .unwrap();
        assert_eq!(changed.status(), 200);
        assert_eq!(
            request(reqwest::Method::POST, "/api/agent-providers")
                .json(&json!({"id":"api","name":"API","type":"claude","expected":catalog[2]}))
                .send()
                .await
                .unwrap()
                .status(),
            400
        );
        stop(&address, &token, task).await;
        let (address, token, task) = start(data.path(), assets.path()).await;
        let catalog = client
            .get(format!("{address}/api/agent-providers"))
            .bearer_auth(&token)
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        assert!(!catalog.contains("synthetic-key"));
        let catalog: Vec<Document> = serde_json::from_str(&catalog).unwrap();
        assert_eq!(catalog[2].get::<bool>("apiKeySet").unwrap(), Some(true));
        let response = client
            .delete(format!("{address}/api/agent-providers/api"))
            .bearer_auth(&token)
            .header("x-local-request", "1")
            .json(&json!({"expected":catalog[2]}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        stop(&address, &token, task).await;
        let database = Database::open(&data.path().join("kanban.sqlite")).unwrap();
        assert!(
            database
                .call(token_vault::read_encrypted)
                .await
                .unwrap()
                .is_none()
        );
        let stored = database
            .call(|store| store.settings::<Document>())
            .await
            .unwrap();
        assert!(stored.string_equals("future", "keep").unwrap());
        assert!(stored.string_equals("apiModel", "custom/model").unwrap());
        database.close().await.unwrap();
    }
}

#[cfg(windows)]
#[tokio::test]
async fn global_settings_commit_with_dpapi_preserve_concurrent_edits_and_roll_back() {
    use atelier_app::infrastructure::{settings, token_vault};
    use atelier_core::document::Document;
    for fault in [false, true] {
        let data = tempfile::tempdir().unwrap();
        let assets = tempfile::tempdir().unwrap();
        let repo = tempfile::tempdir().unwrap();
        let directory = PrototypeDirectory::open(data.path()).unwrap();
        let database = Database::open(&directory.path.join("kanban.sqlite")).unwrap();
        database.call(|store| {
            let settings: Document = serde_json::from_str(r#"{"baseBranch":"old","taskQueue":{"ids":["card"]},"future":{"\u00e9":"\u00e8"}}"#)?;
            store.save_settings(&settings)
        }).await.unwrap();
        let old: Document =
            serde_json::from_str(r#"{"jira":"old-synthetic","future":"synthetic-\u00e8"}"#)
                .unwrap();
        token_vault::save(&database, &old).await.unwrap();
        let encrypted = database
            .call(token_vault::read_encrypted)
            .await
            .unwrap()
            .unwrap();
        database.close().await.unwrap();
        drop(database);
        drop(directory);
        if fault {
            rusqlite::Connection::open(data.path().join("kanban.sqlite")).unwrap().execute_batch("CREATE TRIGGER refuse_settings BEFORE UPDATE ON settings BEGIN SELECT RAISE(ABORT, 'synthetic failure'); END;").unwrap();
        }
        let (address, token, task) = start(data.path(), assets.path()).await;
        let client = reqwest::Client::new();
        let request = |method, path: &str| {
            client
                .request(method, format!("{address}{path}"))
                .bearer_auth(&token)
                .header("x-local-request", "1")
        };
        let state = request(reqwest::Method::GET, "/api/state")
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        assert!(!state.contains("old-synthetic"));
        let state: Document = serde_json::from_str(&state).unwrap();
        let settings: Document = state.get("settings").unwrap().unwrap();
        assert_eq!(settings.get::<bool>("tokenSet").unwrap(), Some(true));
        assert_eq!(
            request(reqwest::Method::PUT, "/api/settings")
                .json(&json!({"jiraUrl":"http://invalid.test","token":"secret"}))
                .send()
                .await
                .unwrap()
                .status(),
            400
        );
        assert_eq!(
            request(reqwest::Method::DELETE, "/api/settings/tokens/unknown")
                .json(&json!({}))
                .send()
                .await
                .unwrap()
                .status(),
            404
        );
        let mut body: Document = serde_json::from_str(r#"{"baseBranch":"new","jiraEmail":"\u00e9","token":"new-synthetic-\u00e8","taskQueue":null,"future":false}"#).unwrap();
        body.set("repo", &repo.path().to_string_lossy()).unwrap();
        let response = request(reqwest::Method::PUT, "/api/settings")
            .header("content-type", "application/json")
            .body(serde_json::to_string(&body).unwrap())
            .send()
            .await
            .unwrap();
        if fault {
            assert_eq!(response.status(), 500);
            assert_eq!(
                response.json::<Value>().await.unwrap()["error"],
                "message.the_settings_could_not_be_updated"
            );
            stop(&address, &token, task).await;
            let database = Database::open(&data.path().join("kanban.sqlite")).unwrap();
            assert_eq!(
                database
                    .call(token_vault::read_encrypted)
                    .await
                    .unwrap()
                    .as_deref(),
                Some(encrypted.as_str())
            );
            let config: Document = database
                .call(|store| settings::public_config(store, &mut session::protect))
                .await
                .unwrap();
            assert!(config.string_equals("baseBranch", "old").unwrap());
            assert_eq!(config.get::<bool>("tokenSet").unwrap(), Some(true));
            database.close().await.unwrap();
            continue;
        }
        assert_eq!(response.status(), 200);
        let config: Document = serde_json::from_str(&response.text().await.unwrap()).unwrap();
        assert_eq!(
            config.get::<String>("jiraEmail").unwrap(),
            Some("é".to_owned())
        );
        assert_eq!(
            config.raw("future").unwrap().get(),
            r#"{"\u00e9":"\u00e8"}"#
        );
        assert!(config.raw("taskQueue").is_some());
        assert!(
            !serde_json::to_string(&config)
                .unwrap()
                .contains("new-synthetic")
        );
        let a = request(reqwest::Method::PUT, "/api/settings")
            .json(&json!({"azureToken":"azure-synthetic","baseBranch":"concurrent"}))
            .send();
        let b = request(reqwest::Method::PUT, "/api/settings")
            .json(&json!({"tempoToken":"tempo-synthetic","jql":"updated"}))
            .send();
        let (a, b) = tokio::join!(a, b);
        assert_eq!(a.unwrap().status(), 200);
        assert_eq!(b.unwrap().status(), 200);
        let created: Value = request(reqwest::Method::POST, "/api/cards")
            .json(&json!({"title":"settings inheritance"}))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(
            std::fs::canonicalize(created["repo"].as_str().unwrap()).unwrap(),
            std::fs::canonicalize(repo.path()).unwrap()
        );
        let template = request(reqwest::Method::POST, "/api/templates")
            .json(&json!({"cardId":created["id"],"name":"concurrent template"}))
            .send();
        let changed = request(reqwest::Method::PUT, "/api/settings")
            .json(&json!({"jql":"with template"}))
            .send();
        let (template, changed) = tokio::join!(template, changed);
        assert_eq!(template.unwrap().status(), 200);
        assert_eq!(changed.unwrap().status(), 200);
        stop(&address, &token, task).await;
        let database = Database::open(&data.path().join("kanban.sqlite")).unwrap();
        let tokens = token_vault::load(&database).await.unwrap();
        assert!(tokens.string_equals("azure", "azure-synthetic").unwrap());
        assert!(tokens.string_equals("tempo", "tempo-synthetic").unwrap());
        assert!(tokens.raw("future").is_some());
        let stored = database
            .call(|store| store.settings::<Document>())
            .await
            .unwrap();
        assert!(stored.string_equals("baseBranch", "concurrent").unwrap());
        assert!(stored.string_equals("jql", "with template").unwrap());
        assert_eq!(
            stored
                .get::<Vec<Document>>("taskTemplates")
                .unwrap()
                .unwrap()
                .len(),
            1
        );
        assert!(
            !serde_json::to_string(&stored)
                .unwrap()
                .contains("synthetic")
        );
        database.close().await.unwrap();
        drop(database);
        let (address, token, task) = start(data.path(), assets.path()).await;
        let state: Document = serde_json::from_str(
            &client
                .get(format!("{address}/api/state"))
                .bearer_auth(&token)
                .send()
                .await
                .unwrap()
                .text()
                .await
                .unwrap(),
        )
        .unwrap();
        let config: Document = state.get("settings").unwrap().unwrap();
        assert_eq!(config.get::<bool>("tokenSet").unwrap(), Some(true));
        assert_eq!(config.get::<bool>("azureTokenSet").unwrap(), Some(true));
        for service in ["jira", "azure", "tempo"] {
            let response = client
                .delete(format!("{address}/api/settings/tokens/{service}"))
                .bearer_auth(&token)
                .header("x-local-request", "1")
                .json(&json!({}))
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), 200);
        }
        stop(&address, &token, task).await;
        let database = Database::open(&data.path().join("kanban.sqlite")).unwrap();
        let tokens = token_vault::load(&database).await.unwrap();
        assert!(tokens.raw("future").is_some());
        for service in ["jira", "azure", "tempo"] {
            assert!(!tokens.has_text(service).unwrap());
        }
        database.close().await.unwrap();
    }
}

#[tokio::test]
async fn empty_repository_paths_resolve_the_synthetic_working_directory() {
    const CHILD: &str = "ATELIER_TEST_EMPTY_REPOSITORY";
    if let Some(directory) = std::env::var_os(CHILD) {
        let expected = std::fs::canonicalize(&directory).unwrap();
        assert_eq!(
            std::fs::canonicalize(std::env::current_dir().unwrap()).unwrap(),
            expected
        );
        let data = tempfile::tempdir().unwrap();
        let assets = tempfile::tempdir().unwrap();
        let (address, token, task) = start(data.path(), assets.path()).await;
        let client = reqwest::Client::new();
        let request = |method, path: &str| {
            client
                .request(method, format!("{address}{path}"))
                .bearer_auth(&token)
                .header("x-local-request", "1")
        };
        let project = request(reqwest::Method::POST, "/api/projects")
            .json(&json!({"name":"Synthetic current directory", "repo":""}))
            .send()
            .await
            .unwrap();
        assert_eq!(project.status(), 200);
        let project: Value = project.json().await.unwrap();
        let repo = project["repo"].as_str().unwrap();
        assert_eq!(std::fs::canonicalize(repo).unwrap(), expected);
        assert!(!repo.starts_with(r"\\?\"));
        let card = request(reqwest::Method::POST, "/api/cards")
            .json(&json!({"title":"Synthetic current directory", "repo":" \t "}))
            .send()
            .await
            .unwrap();
        assert_eq!(card.status(), 200);
        let card: Value = card.json().await.unwrap();
        assert_eq!(card["repo"], repo);
        let edited = request(
            reqwest::Method::PATCH,
            &format!("/api/cards/{}", card["id"].as_str().unwrap()),
        )
        .json(&json!({"repo":" \t "}))
        .send()
        .await
        .unwrap();
        assert_eq!(edited.status(), 200);
        assert_eq!(edited.json::<Value>().await.unwrap()["repo"], repo);
        stop(&address, &token, task).await;
    } else {
        // Isolate cwd in a child so concurrent tests cannot observe a process-wide directory change.
        let directory = tempfile::tempdir().unwrap();
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "empty_repository_paths_resolve_the_synthetic_working_directory",
                "--nocapture",
            ])
            .current_dir(directory.path())
            .env(CHILD, directory.path());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

async fn next_event(response: &mut reqwest::Response, buffered: &mut String) -> String {
    tokio::time::timeout(Duration::from_secs(17), async {
        loop {
            if let Some(end) = buffered.find("\n\n") {
                return buffered.drain(..end + 2).collect();
            }
            let chunk = response.chunk().await.unwrap().expect("event stream ended");
            buffered.push_str(std::str::from_utf8(&chunk).unwrap());
        }
    })
    .await
    .expect("event did not arrive")
}

#[tokio::test]
async fn local_time_routes_serialize_timer_starts_preserve_updates_and_close_cleanly() {
    let data = tempfile::tempdir().unwrap();
    let assets = tempfile::tempdir().unwrap();
    let (address, token, task) = start(data.path(), assets.path()).await;
    let client = reqwest::Client::new();
    let request = |method, path: &str| {
        client
            .request(method, format!("{address}{path}"))
            .bearer_auth(&token)
            .header("x-local-request", "1")
    };
    let card: Value = request(reqwest::Method::POST, "/api/cards")
        .json(&json!({"title":"Synthetic timer"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let fields = json!({"cardId":card["id"], "seconds":60, "date":"2026-10-22", "startTime":"09:00:00", "description":"Work", "start":true});
    let (first, second) = tokio::join!(
        request(reqwest::Method::POST, "/api/time")
            .json(&fields)
            .send(),
        request(reqwest::Method::POST, "/api/time")
            .json(&fields)
            .send()
    );
    let first = first.unwrap();
    let second = second.unwrap();
    let (created, refused) = if first.status().is_success() {
        (first, second)
    } else {
        (second, first)
    };
    assert_eq!(created.status(), 200);
    assert_eq!(refused.status(), 400);
    let created: Value = created.json().await.unwrap();
    assert_eq!(created["seconds"], 0);
    let card_id = card["id"].as_str().unwrap();
    assert_eq!(
        request(
            reqwest::Method::POST,
            &format!("/api/cards/{card_id}/archive")
        )
        .json(&json!({"archived":true}))
        .send()
        .await
        .unwrap()
        .status(),
        409
    );
    assert_eq!(
        request(reqwest::Method::DELETE, &format!("/api/cards/{card_id}"))
            .json(&json!({}))
            .send()
            .await
            .unwrap()
            .status(),
        409
    );
    let id = created["id"].as_str().unwrap();
    assert_eq!(
        request(reqwest::Method::DELETE, &format!("/api/time/{id}"))
            .json(&json!({}))
            .send()
            .await
            .unwrap()
            .status(),
        400
    );
    let stopped: Value = request(reqwest::Method::POST, &format!("/api/time/{id}/stop"))
        .json(&json!({}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(stopped.get("runningSince").is_none());
    let mut update = fields.clone();
    update["seconds"] = json!(90);
    update["description"] = json!(" Modified ");
    update["expected"] = stopped.clone();
    let modified: Value = request(reqwest::Method::PATCH, &format!("/api/time/{id}"))
        .json(&update)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(modified["seconds"], 90);
    assert_eq!(modified["description"], "Modified");
    assert_eq!(modified["interrupted"], false);
    assert_eq!(
        request(
            reqwest::Method::POST,
            &format!("/api/cards/{card_id}/archive")
        )
        .json(&json!({"archived":"true"}))
        .send()
        .await
        .unwrap()
        .status(),
        400
    );
    let archived: Value = request(
        reqwest::Method::POST,
        &format!("/api/cards/{card_id}/archive"),
    )
    .json(&json!({"archived":true}))
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
    assert_eq!(archived["archived"], true);
    assert_eq!(
        request(reqwest::Method::POST, "/api/time")
            .json(&fields)
            .send()
            .await
            .unwrap()
            .status(),
        400
    );
    assert_eq!(
        request(
            reqwest::Method::POST,
            &format!("/api/cards/{card_id}/archive")
        )
        .json(&json!({"archived":false}))
        .send()
        .await
        .unwrap()
        .status(),
        200
    );
    let mut events = request(reqwest::Method::GET, "/api/events")
        .send()
        .await
        .unwrap();
    let mut buffered = String::new();
    assert!(
        next_event(&mut events, &mut buffered)
            .await
            .contains("data: connected")
    );
    let rejected = request(reqwest::Method::POST, &format!("/api/time/{id}/export"))
        .json(&json!({"confirmed":true}))
        .send()
        .await
        .unwrap();
    assert_eq!(rejected.status(), 400);
    assert_eq!(
        rejected.json::<Value>().await.unwrap()["error"],
        "message.invalid_tempo_export_parameters"
    );
    assert_eq!(
        request(reqwest::Method::POST, "/api/time")
            .json(&fields)
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    assert_eq!(
        next_event(&mut events, &mut buffered).await,
        "data: changed\n\n"
    );
    assert_eq!(
        next_event(&mut events, &mut buffered).await,
        "data: changed\n\n"
    );
    let ticking: Value = request(reqwest::Method::GET, "/api/state")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(ticking["times"][0]["seconds"].as_f64().unwrap() >= 1.0);
    stop(&address, &token, task).await;
    let (address, token, task) = start(data.path(), assets.path()).await;
    let board: Value = client
        .get(format!("{address}/api/state"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(board["times"].as_array().unwrap().len(), 2);
    assert!(
        board["times"]
            .as_array()
            .unwrap()
            .iter()
            .all(|entry| entry.get("runningSince").is_none())
    );
    assert_eq!(board["times"][1]["description"], "Modified");
    assert_eq!(
        client
            .delete(format!("{address}/api/cards/{card_id}"))
            .bearer_auth(&token)
            .header("x-local-request", "1")
            .json(&json!({}))
            .send()
            .await
            .unwrap()
            .status(),
        409
    );
    assert_eq!(
        client
            .delete(format!("{address}/api/time/{id}"))
            .bearer_auth(&token)
            .header("x-local-request", "1")
            .json(&json!({"expected":board["times"][1]}))
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    let other = board["times"][0]["id"].as_str().unwrap();
    assert_eq!(
        client
            .delete(format!("{address}/api/time/{other}"))
            .bearer_auth(&token)
            .header("x-local-request", "1")
            .json(&json!({"expected":board["times"][0]}))
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    let removed: Value = client
        .delete(format!("{address}/api/cards/{card_id}"))
        .bearer_auth(&token)
        .header("x-local-request", "1")
        .json(&json!({}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(removed, json!({"ok":true}));
    assert_eq!(
        client
            .post(format!("{address}/api/cards/{card_id}/archive"))
            .bearer_auth(&token)
            .header("x-local-request", "1")
            .json(&json!({"archived":true}))
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
    stop(&address, &token, task).await;
}

#[tokio::test]
async fn project_edits_back_up_before_relocation_and_roll_back_cards_and_reviews() {
    use atelier_app::infrastructure::{backups::BackupDirectory, store::Documents};
    use atelier_core::{
        application::task_creation::CreationContext, contracts::Board, document::Document,
    };
    #[cfg(windows)]
    fn deny_creation(path: &Path, denied: bool) {
        use std::os::windows::process::CommandExt;
        let executable = std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap())
            .join("System32/icacls.exe");
        let output = std::process::Command::new(executable)
            .arg(path)
            .args(if denied {
                ["/deny", "*S-1-1-0:(WD,AD)"]
            } else {
                ["/remove:d", "*S-1-1-0"]
            })
            .creation_flags(0x08000000)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    fn read(path: &Path, table: &str) -> Vec<Document> {
        let connection =
            rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                .unwrap();
        let mut query = connection
            .prepare(&format!("SELECT data FROM {table} ORDER BY id"))
            .unwrap();
        query
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .map(|row| serde_json::from_str(&row.unwrap()).unwrap())
            .collect()
    }
    for fault in ["none", "review", "backup"] {
        let data = tempfile::tempdir().unwrap();
        let assets = tempfile::tempdir().unwrap();
        let repositories = tempfile::tempdir().unwrap();
        let old = repositories.path().join("old");
        let new = repositories.path().join("new");
        let other = repositories.path().join("other");
        for path in [&old, &new, &other] {
            std::fs::create_dir(path).unwrap();
        }
        std::fs::write(old.join("keep.txt"), "synthetic files stay here").unwrap();
        {
            let _directory = PrototypeDirectory::open(data.path()).unwrap();
            let database = Database::open(&data.path().join("kanban.sqlite")).unwrap();
            let repo = old.clone();
            let other_repo = other.clone();
            database.call(move |store| {
                let repo = store.repository(&serde_json::value::to_raw_value(&repo)?)?.clone();
                let other_repo = store.repository(&serde_json::value::to_raw_value(&other_repo)?)?.clone();
                let mut project: Document = serde_json::from_str(r#"{"id":"project","name":"\u00e9","frontRepo":null,"\u00e9":"\u00e8"}"#)?;
                project.set("repo", &repo)?;
                store.put(Documents::Projects, &project)?;
                store.put(Documents::Projects, &json!({"id":"other","name":"Other","repo":other_repo}))?;
                for id in ["one", "two"] {
                    let mut card: Document = serde_json::from_str(r#"{"title":"\u00e9","status":"todo","automation":{"state":"ready","unknown":"\u00e8"},"extra":"\u00e8"}"#)?;
                    card.set("id", &id)?; card.set("repo", &repo)?; card.set("archived", &(id == "two"))?;
                    store.put(Documents::Cards, &card)?;
                    store.put(Documents::Runs, &json!({"id":format!("review-{id}"),"cardId":id,"kind":"pr-review","status":"completed","repo":repo}))?;
                }
                store.put(Documents::Runs, &json!({"id":"analyze","cardId":"one","kind":"analyze","status":"completed"}))?;
                Ok(())
            }).await.unwrap();
            database.close().await.unwrap();
        }
        if fault == "review" {
            rusqlite::Connection::open(data.path().join("kanban.sqlite")).unwrap().execute_batch(
                "CREATE TRIGGER fail_project_review BEFORE UPDATE ON runs WHEN OLD.id='review-one' BEGIN SELECT RAISE(ABORT,'private synthetic review failure'); END;"
            ).unwrap();
        }
        let (address, token, task) = start(data.path(), assets.path()).await;
        let client = reqwest::Client::new();
        let patch = |body: Document| {
            client
                .patch(format!("{address}/api/projects/project"))
                .bearer_auth(&token)
                .header("x-local-request", "1")
                .json(&body)
                .send()
        };
        let rules: Document =
            serde_json::from_str(r#"{"projectRules":" \u00e9 ","unknown":"ignored"}"#).unwrap();
        let response = patch(rules).await.unwrap();
        assert_eq!(response.status(), 200);
        let response: Document = response.json().await.unwrap();
        let saved_repo = response.get::<String>("repo").unwrap().unwrap();
        assert_eq!(
            response.get::<String>("projectRules").unwrap(),
            Some("é".to_owned())
        );
        assert_eq!(response.raw("é").unwrap().get(), r#""\u00e8""#);
        assert!(response.raw("unknown").is_none());
        let (branch, target) = tokio::join!(
            patch(serde_json::from_str(r#"{"baseBranch":"develop"}"#).unwrap()),
            patch(serde_json::from_str(r#"{"prTarget":"main"}"#).unwrap())
        );
        assert_eq!(branch.unwrap().status(), 200);
        assert_eq!(target.unwrap().status(), 200);
        for (body, status) in [
            (json!({"repo":other}), 409),
            (json!({"repo":repositories.path().join("missing")}), 400),
            (json!({"linkedProjectId":"project"}), 400),
            (json!({"frontDirectory":"../outside"}), 400),
            (json!({"repositoryConnector":"git"}), 400),
        ] {
            assert_eq!(
                patch(serde_json::from_str(&body.to_string()).unwrap())
                    .await
                    .unwrap()
                    .status(),
                status
            );
        }
        assert!(
            BackupDirectory::open(data.path())
                .unwrap()
                .list()
                .unwrap()
                .is_empty()
        );
        if fault == "backup" {
            #[cfg(windows)]
            deny_creation(&data.path().join("backups"), true);
            #[cfg(not(windows))]
            {
                // The held handle must not follow a replacement of this empty synthetic directory.
                std::fs::remove_dir(data.path().join("backups")).unwrap();
                std::fs::create_dir(data.path().join("backups")).unwrap();
            }
        }
        let mut body = Document::default();
        body.set("repo", &new).unwrap();
        let response = patch(body).await;
        #[cfg(windows)]
        if fault == "backup" {
            deny_creation(&data.path().join("backups"), false);
        }
        let response = response.unwrap();
        assert_eq!(response.status(), if fault == "none" { 200 } else { 500 });
        if fault != "none" {
            assert_eq!(
                response.json::<Value>().await.unwrap(),
                json!({"error":"message.the_project_could_not_be_updated"})
            );
        }
        let board: Board = client
            .get(format!("{address}/api/state"))
            .bearer_auth(&token)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let project = board
            .projects
            .iter()
            .find(|project| project.string_equals("id", "project").unwrap())
            .unwrap();
        let expected = if fault == "none" { &new } else { &old };
        assert_eq!(
            std::fs::canonicalize(project.get::<String>("repo").unwrap().unwrap()).unwrap(),
            std::fs::canonicalize(expected).unwrap()
        );
        assert_eq!(
            project.get::<String>("baseBranch").unwrap().as_deref(),
            Some("develop")
        );
        assert_eq!(
            project.get::<String>("prTarget").unwrap().as_deref(),
            Some("main")
        );
        for card in &board.cards {
            assert_eq!(card.title, "é".to_owned());
            assert_eq!(
                std::fs::canonicalize(card.fields.get::<String>("repo").unwrap().unwrap()).unwrap(),
                std::fs::canonicalize(expected).unwrap()
            );
            let automation: Document = card.fields.get("automation").unwrap().unwrap();
            assert_eq!(
                automation.get::<String>("state").unwrap().as_deref(),
                Some(if fault == "none" {
                    "interrupted"
                } else {
                    "ready"
                })
            );
            assert_eq!(
                automation.get::<String>("unknown").unwrap(),
                Some("è".to_owned())
            );
        }
        stop(&address, &token, task).await;
        let backups = BackupDirectory::open(data.path()).unwrap().list().unwrap();
        assert_eq!(backups.len(), usize::from(fault != "backup"));
        if let Some(backup) = backups.first() {
            let snapshot = data.path().join("backups").join(&backup.name);
            for card in read(&snapshot, "cards") {
                assert_eq!(card.get::<String>("repo").unwrap().unwrap(), saved_repo);
            }
            let projects = read(&snapshot, "projects");
            let project = projects
                .iter()
                .find(|project| project.string_equals("id", "project").unwrap())
                .unwrap();
            assert_eq!(project.get::<String>("repo").unwrap().unwrap(), saved_repo);
            assert_eq!(
                project.get::<String>("projectRules").unwrap(),
                Some("é".to_owned())
            );
            for run in read(&snapshot, "runs") {
                assert!(run.raw("reviewStaleReason").is_none());
            }
        }
        for run in read(&data.path().join("kanban.sqlite"), "runs") {
            assert_eq!(
                run.raw("reviewStaleReason").is_some(),
                fault == "none" && run.string_equals("kind", "pr-review").unwrap()
            );
            if run.string_equals("kind", "pr-review").unwrap() {
                assert_eq!(run.get::<String>("repo").unwrap().unwrap(), saved_repo);
            }
        }
        assert_eq!(
            std::fs::read_to_string(old.join("keep.txt")).unwrap(),
            "synthetic files stay here"
        );
        assert!(!new.join("keep.txt").exists());
        let (address, token, task) = start(data.path(), assets.path()).await;
        let restored: Board = client
            .get(format!("{address}/api/state"))
            .bearer_auth(&token)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(restored.revision, 0);
        // The connection revision restarts; every persisted document must remain identical.
        let restored = Board {
            revision: board.revision,
            ..restored
        };
        assert_eq!(
            serde_json::to_string(&restored).unwrap(),
            serde_json::to_string(&board).unwrap()
        );
        stop(&address, &token, task).await;
    }
}

#[tokio::test]
async fn project_detection_reads_only_synthetic_repositories_and_never_saves_drafts() {
    let data = tempfile::tempdir().unwrap();
    let assets = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let (address, token, task) = start(data.path(), assets.path()).await;
    let repo = data.path().join("repositories/main");
    let draft = data.path().join("repositories/draft");
    std::fs::create_dir_all(repo.join("Properties")).unwrap();
    std::fs::create_dir_all(&draft).unwrap();
    std::fs::write(repo.join("package.json"), r#"{"scripts":{"dev":"vite"}}"#).unwrap();
    std::fs::write(
        repo.join("Api.csproj"),
        r#"<Project Sdk="Microsoft.NET.Sdk.Web"/>"#,
    )
    .unwrap();
    std::fs::write(repo.join("Properties/launchSettings.json"), r#"{"profiles":{"http":{"commandName":"Project","environmentVariables":{"SECRET":"synthetic-private-value"}}}}"#).unwrap();
    std::fs::write(
        draft.join("package.json"),
        r#"{"scripts":{"start":"node server.mjs"},"dependencies":{"express":"*"}}"#,
    )
    .unwrap();
    std::fs::write(
        outside.path().join("package.json"),
        "outside synthetic sentinel",
    )
    .unwrap();
    let client = reqwest::Client::new();
    let request = |method, path: &str| {
        client
            .request(method, format!("{address}{path}"))
            .bearer_auth(&token)
            .header("x-local-request", "1")
    };
    let response = request(reqwest::Method::POST, "/api/projects")
        .json(&json!({"name":"Synthetic detection","repo":repo}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let project: Value = response.json().await.unwrap();
    let path = format!(
        "/api/projects/{}/detect-launch",
        project["id"].as_str().unwrap()
    );
    let response = request(reqwest::Method::GET, &path).send().await.unwrap();
    assert_eq!(response.status(), 200);
    let detected: Value = response.json().await.unwrap();
    assert_eq!(detected["candidates"].as_array().unwrap().len(), 2);
    assert_eq!(detected["candidates"][0]["technology"], "Vite");
    assert_eq!(detected["candidates"][1]["technology"], "ASP.NET Core");
    for candidate in detected["candidates"].as_array().unwrap() {
        assert_eq!(candidate["sourceRepo"], project["repo"]);
    }
    assert!(!detected.to_string().contains("synthetic-private-value"));
    let response = request(reqwest::Method::POST, &path)
        .json(&json!({"role":"front","frontRepo":draft,"backRepo":outside.path()}))
        .send()
        .await
        .unwrap();
    let status = response.status();
    let response = response.text().await.unwrap();
    assert_eq!(status, 200, "{response}; draft={draft:?}; saved={project}");
    let detected: Value = serde_json::from_str(&response).unwrap();
    assert_eq!(detected["candidates"].as_array().unwrap().len(), 1);
    assert_eq!(detected["candidates"][0]["role"], "front");
    assert_eq!(detected["candidates"][0]["technology"], "Express");
    assert_ne!(detected["candidates"][0]["sourceRepo"], project["repo"]);
    let response = request(reqwest::Method::POST, &path)
        .json(&json!({"repo":outside.path()}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 400);
    assert!(
        response
            .text()
            .await
            .unwrap()
            .contains("message.explicitly_authorize_this_repository_in_settings_before_using_it")
    );
    assert_eq!(
        request(reqwest::Method::POST, &path)
            .json(&json!({"role":"invalid"}))
            .send()
            .await
            .unwrap()
            .status(),
        400
    );
    assert_eq!(
        request(reqwest::Method::POST, "/api/projects/missing/detect-launch")
            .json(&json!({"role":"invalid"}))
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
    assert_eq!(
        client
            .get(format!("{address}{path}"))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    assert_eq!(
        request(reqwest::Method::POST, &path)
            .header("origin", "https://example.invalid")
            .json(&json!({}))
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    let state: Value = request(reqwest::Method::GET, "/api/state")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(state["projects"][0], project);
    assert_eq!(
        std::fs::read(outside.path().join("package.json")).unwrap(),
        b"outside synthetic sentinel"
    );
    stop(&address, &token, task).await;
}

#[tokio::test]
async fn project_routes_serialize_duplicates_guard_timers_and_roll_back_archival_with_deletion() {
    use atelier_core::{contracts::Board, document::Document};
    let data = tempfile::tempdir().unwrap();
    let assets = tempfile::tempdir().unwrap();
    let repositories = tempfile::tempdir().unwrap();
    let repo = repositories.path().join("project");
    std::fs::create_dir(&repo).unwrap();
    let (address, token, task) = start(data.path(), assets.path()).await;
    let client = reqwest::Client::new();
    let request = |method, path: &str| {
        client
            .request(method, format!("{address}{path}"))
            .bearer_auth(&token)
            .header("x-local-request", "1")
    };
    let mut body = Document::default();
    body.set("name", &"é".to_owned()).unwrap();
    body.set("repo", &repo).unwrap();
    let (first, second) = tokio::join!(
        request(reqwest::Method::POST, "/api/projects")
            .json(&body)
            .send(),
        request(reqwest::Method::POST, "/api/projects")
            .json(&body)
            .send()
    );
    let mut project = None;
    for response in [first.unwrap(), second.unwrap()] {
        if response.status() == 200 {
            assert!(project.is_none());
            project = Some(response.json::<Document>().await.unwrap());
        } else {
            assert_eq!(response.status(), 409);
        }
    }
    let project = project.unwrap();
    let id = project.get::<String>("id").unwrap().unwrap();
    let repo_string = project.get::<String>("repo").unwrap().unwrap();
    assert_eq!(project.get::<String>("name").unwrap(), Some("é".to_owned()));
    let mut cards = Vec::new();
    for title in ["First", "Second"] {
        let response = request(reqwest::Method::POST, "/api/cards")
            .json(&json!({"title":title,"repo":repo}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        cards.push(response.json::<Card>().await.unwrap());
    }
    let timer = request(reqwest::Method::POST, "/api/time").json(&json!({"cardId":cards[0].id,"seconds":60,"date":"2026-10-24","startTime":"09:00:00","description":"synthetic","start":true})).send().await.unwrap();
    assert_eq!(timer.status(), 200);
    let timer: Value = timer.json().await.unwrap();
    assert_eq!(
        request(reqwest::Method::DELETE, &format!("/api/projects/{id}"))
            .json(&json!({"archiveTasks":true}))
            .send()
            .await
            .unwrap()
            .status(),
        409
    );
    assert_eq!(
        request(reqwest::Method::POST, "/api/projects/archive-tasks")
            .json(&json!({"repo":repo_string}))
            .send()
            .await
            .unwrap()
            .status(),
        409
    );
    for (method, path, expected) in [
        (reqwest::Method::PATCH, format!("/api/projects/{id}"), 200),
        (
            reqwest::Method::GET,
            format!("/api/projects/{id}/detect-launch"),
            400,
        ),
    ] {
        assert_eq!(
            request(method, &path)
                .json(&json!({}))
                .send()
                .await
                .unwrap()
                .status(),
            expected
        );
    }
    assert_eq!(
        request(
            reqwest::Method::POST,
            &format!("/api/time/{}/stop", timer["id"].as_str().unwrap())
        )
        .json(&json!({}))
        .send()
        .await
        .unwrap()
        .status(),
        200
    );
    stop(&address, &token, task).await;
    {
        let injection = rusqlite::Connection::open(data.path().join("kanban.sqlite")).unwrap();
        injection.execute_batch("CREATE TRIGGER reject_project_deletion BEFORE DELETE ON projects BEGIN SELECT RAISE(ABORT, 'private synthetic deletion failure'); END;").unwrap();
    }
    let (address, token, task) = start(data.path(), assets.path()).await;
    let response = client
        .delete(format!("{address}/api/projects/{id}"))
        .bearer_auth(&token)
        .header("x-local-request", "1")
        .json(&json!({"archiveTasks":true}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 500);
    assert_eq!(
        response.json::<Value>().await.unwrap(),
        json!({"error":"message.the_project_could_not_be_updated"})
    );
    let board: Board = client
        .get(format!("{address}/api/state"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(board.projects.len(), 1);
    assert_eq!(board.cards.len(), 2);
    assert_eq!(board.times.len(), 1);
    for card in &board.cards {
        assert!(!card.fields.flag("archived").unwrap());
    }
    assert_eq!(
        board.projects[0].get::<String>("name").unwrap(),
        Some("é".to_owned())
    );
    stop(&address, &token, task).await;
    {
        let injection = rusqlite::Connection::open(data.path().join("kanban.sqlite")).unwrap();
        injection
            .execute_batch("DROP TRIGGER reject_project_deletion")
            .unwrap();
    }
    let (address, token, task) = start(data.path(), assets.path()).await;
    let response = client
        .delete(format!("{address}/api/projects/{id}"))
        .bearer_auth(&token)
        .header("x-local-request", "1")
        .json(&json!({"archiveTasks":true}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(
        response.json::<Value>().await.unwrap(),
        json!({"ok":true,"archived":2})
    );
    std::fs::rename(&repo, repositories.path().join("moved-synthetic-directory")).unwrap();
    let response = client
        .post(format!("{address}/api/projects/archive-tasks"))
        .bearer_auth(&token)
        .header("x-local-request", "1")
        .json(&json!({"repo":repo_string}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(response.json::<Value>().await.unwrap(), json!({"count":2}));
    stop(&address, &token, task).await;
    let (address, token, task) = start(data.path(), assets.path()).await;
    let board: Board = client
        .get(format!("{address}/api/state"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(board.projects.is_empty());
    assert_eq!(board.cards.len(), 2);
    assert_eq!(board.times.len(), 1);
    for card in &board.cards {
        assert!(card.fields.flag("archived").unwrap());
    }
    stop(&address, &token, task).await;
}

#[tokio::test]
async fn edits_invalidate_reviews_atomically_preserve_guards_and_refuse_unported_effects() {
    use atelier_app::infrastructure::store::Documents;
    use atelier_core::{contracts::Board, document::Document};
    let data = tempfile::tempdir().unwrap();
    let assets = tempfile::tempdir().unwrap();
    {
        let root = PrototypeDirectory::open(data.path()).unwrap();
        let database = Database::open(&root.path.join("kanban.sqlite")).unwrap();
        database.call(|store| {
            let seed: Document = serde_json::from_str(r#"{"id":"local","title":"Source","description":"\u00e9","instructions":"","agent":"codex","mode":"edit","status":"todo","updatedAt":"before","unknown":"\u00e8","checklist":[{"text":"\u00e9","done":false}],"automation":{"state":"ready","future":"\u00e9"}}"#)?;
            for id in ["local", "ticket", "worktree", "archived", "rollback"] {
                let mut card = seed.clone(); card.set("id", &id)?;
                match id {
                    "ticket" => card.set("ticket", &json!({"key":"TEST-1"}))?,
                    "worktree" => { card.set("worktree", &json!({"path":"protected"}))?; card.set("branch", &"main")?; },
                    "archived" => card.set("archived", &true)?,
                    _ => (),
                }
                store.put(Documents::Cards, &card)?;
                store.put(Documents::Runs, &json!({"id":id,"cardId":id,"kind":"pr-review","future":{"keep":true}}))?;
            }
            store.put(Documents::Cards, &json!({"id":"dependent","title":"Dependency","agent":"codex","status":"todo","blockedBy":["local"]}))?;
            store.put(Documents::Runs, &json!({"id":"other","cardId":"local","kind":"edit"}))?;
            Ok(())
        }).await.unwrap();
        database.close().await.unwrap();
        let injection = rusqlite::Connection::open(root.path.join("kanban.sqlite")).unwrap();
        injection.execute_batch("CREATE TRIGGER reject_review BEFORE UPDATE ON runs WHEN NEW.id='rollback' BEGIN SELECT RAISE(ABORT, 'private synthetic review failure'); END;").unwrap();
    }
    let (address, token, task) = start(data.path(), assets.path()).await;
    let client = reqwest::Client::new();
    let edit = |id: &str| {
        client
            .patch(format!("{address}/api/cards/{id}"))
            .bearer_auth(&token)
            .header("x-local-request", "1")
    };
    let checked = edit("local")
        .header("content-type", "application/json")
        .body(r#"{"checklist":[{"text":"\u00e9","done":true}]}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(checked.status(), 200);
    let checked: Card = checked.json().await.unwrap();
    assert!(
        checked
            .fields
            .get::<Document>("automation")
            .unwrap()
            .unwrap()
            .string_equals("state", "ready")
            .unwrap()
    );
    let renamed = edit("local")
        .json(&json!({"title":"Renamed", "agent":"claude"}))
        .send()
        .await
        .unwrap();
    assert_eq!(renamed.status(), 200);
    let renamed: Card = renamed.json().await.unwrap();
    assert_eq!(
        renamed.fields.get::<bool>("agentExplicit").unwrap(),
        Some(true)
    );
    assert_eq!(
        renamed.fields.get::<String>("description").unwrap(),
        Some("é".to_owned())
    );
    let automation = renamed
        .fields
        .get::<Document>("automation")
        .unwrap()
        .unwrap();
    assert!(automation.string_equals("state", "interrupted").unwrap());
    assert_eq!(
        automation.get::<String>("future").unwrap(),
        Some("é".to_owned())
    );
    let (description, instructions) = tokio::join!(
        edit("local")
            .json(&json!({"description":"New description"}))
            .send(),
        edit("local")
            .json(&json!({"instructions":"New instructions"}))
            .send()
    );
    assert_eq!(description.unwrap().status(), 200);
    assert_eq!(instructions.unwrap().status(), 200);
    let moved = edit("local")
        .json(&json!({"status":"doing"}))
        .send()
        .await
        .unwrap();
    assert_eq!(moved.status(), 200);
    assert!(
        moved
            .json::<Card>()
            .await
            .unwrap()
            .fields
            .string_equals("status", "doing")
            .unwrap()
    );
    for (id, body, status) in [
        (
            "ticket",
            json!({"status":"review", "instructions":"Must not persist"}),
            503,
        ),
        ("local", json!({"blockedBy":["dependent"]}), 400),
        ("worktree", json!({"branch":"changed"}), 400),
        ("archived", json!({"title":null}), 409),
        ("missing", json!({}), 404),
    ] {
        assert_eq!(edit(id).json(&body).send().await.unwrap().status(), status);
    }
    let rejected_ticket = edit("ticket")
        .json(&json!({"title":null,"description":false,"instructions":"Must not persist"}))
        .send()
        .await
        .unwrap();
    assert_eq!(rejected_ticket.status(), 409);
    let ticket = edit("ticket")
        .json(&json!({"instructions":"Local note"}))
        .send()
        .await
        .unwrap();
    assert_eq!(ticket.status(), 200);
    let ticket: Card = ticket.json().await.unwrap();
    assert_eq!(ticket.title, String::from("Source"));
    assert_eq!(
        ticket.fields.get::<String>("description").unwrap(),
        Some("é".to_owned())
    );
    assert_eq!(
        ticket
            .fields
            .get::<String>("instructions")
            .unwrap()
            .as_deref(),
        Some("Local note")
    );
    let rejected = edit("rollback")
        .json(&json!({"title":"Must roll back"}))
        .send()
        .await
        .unwrap();
    assert_eq!(rejected.status(), 500);
    assert_eq!(
        rejected.json::<Value>().await.unwrap(),
        json!({"error":"message.card_update_failed"})
    );
    stop(&address, &token, task).await;
    let database = Database::open(&data.path().join("kanban.sqlite")).unwrap();
    database
        .call(|store| {
            let review = store
                .document::<Document>(Documents::Runs, "local")?
                .unwrap();
            assert!(review.string_equals(
                "reviewStaleReason",
                atelier_core::application::task_editing::REVIEW_CHANGE_REASON
            )?);
            assert!(review.raw("future").is_some());
            assert!(
                store
                    .document::<Document>(Documents::Runs, "other")?
                    .unwrap()
                    .raw("reviewStaleReason")
                    .is_none()
            );
            assert!(
                store
                    .document::<Document>(Documents::Runs, "rollback")?
                    .unwrap()
                    .raw("reviewStaleReason")
                    .is_none()
            );
            let rollback = store
                .document::<Card>(Documents::Cards, "rollback")?
                .unwrap();
            assert_eq!(rollback.title, String::from("Source"));
            assert!(rollback.fields.string_equals("updatedAt", "before")?);
            assert!(
                rollback
                    .fields
                    .get::<Document>("automation")?
                    .unwrap()
                    .string_equals("state", "ready")?
            );
            Ok(())
        })
        .await
        .unwrap();
    database.close().await.unwrap();
    let (address, token, task) = start(data.path(), assets.path()).await;
    let board: Board = client
        .get(format!("{address}/api/state"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let local = board.cards.iter().find(|card| card.id == "local").unwrap();
    assert_eq!(local.title, String::from("Renamed"));
    assert!(local.fields.string_equals("status", "doing").unwrap());
    assert!(
        local
            .fields
            .string_equals("description", "New description")
            .unwrap()
    );
    assert!(
        local
            .fields
            .string_equals("instructions", "New instructions")
            .unwrap()
    );
    assert_eq!(
        local.fields.get::<String>("unknown").unwrap(),
        Some("è".to_owned())
    );
    stop(&address, &token, task).await;
}

#[tokio::test]
async fn complete_creation_uses_templates_projects_native_paths_and_atomic_storage() {
    use atelier_app::infrastructure::store::Documents;
    use atelier_core::{contracts::Board, document::Document};
    let data = tempfile::tempdir().unwrap();
    let assets = tempfile::tempdir().unwrap();
    let repositories = tempfile::tempdir().unwrap();
    let repo = repositories.path().join("dépôt 🔧");
    std::fs::create_dir(&repo).unwrap();
    let file = repositories.path().join("file");
    std::fs::write(&file, "synthetic").unwrap();
    {
        let root = PrototypeDirectory::open(data.path()).unwrap();
        let database = Database::open(&root.path.join("kanban.sqlite")).unwrap();
        let repo = repo.clone();
        database.call(move |store| {
            let mut settings: Document = serde_json::from_str(r#"{"agent":"codex","unknown":"\u00e9","taskTemplates":[{"id":"template","description":"\u00e9","instructions":" inherited ","mode":"analyze","checklist":[{"text":" \u00e8 ","done":false}]}]}"#)?;
            settings.set("repo", &repo)?;
            store.save_settings(&settings)?;
            store.put(Documents::Projects, &json!({"id":"project", "repo":repo, "agent":"claude", "unknown":"keep"}))?;
            Ok(())
        }).await.unwrap();
        database.close().await.unwrap();
        let injection = rusqlite::Connection::open(root.path.join("kanban.sqlite")).unwrap();
        injection.execute_batch("CREATE TRIGGER reject_creation BEFORE INSERT ON cards WHEN json_extract(NEW.data, '$.title')='fail' BEGIN SELECT RAISE(ABORT, 'synthetic private database detail'); END;").unwrap();
    }
    let (address, token, task) = start(data.path(), assets.path()).await;
    let client = reqwest::Client::new();
    let create = |path: &str| {
        client
            .post(format!("{address}{path}"))
            .bearer_auth(&token)
            .header("x-local-request", "1")
    };
    let first = create("/api/cards").json(&json!({"title":"Inherited", "templateId":"template", "status":"doing", "unknown":"discard"}));
    let second = create("/api/cards").json(&json!({"title":"Explicit", "repo":repo, "agent":"codex", "model":" model/v1 ", "effort":"high", "checklist":[{"text":" criterion ","done":true}]}));
    let (first, second) = tokio::join!(first.send(), second.send());
    let first = first.unwrap();
    let second = second.unwrap();
    assert_eq!(first.status(), 200);
    assert_eq!(second.status(), 200);
    let first: Card = first.json().await.unwrap();
    let second: Card = second.json().await.unwrap();
    assert_ne!(first.id, second.id);
    assert!(first.fields.string_equals("agent", "claude").unwrap());
    assert!(first.fields.string_equals("status", "doing").unwrap());
    assert_eq!(
        first.fields.get::<String>("description").unwrap(),
        Some("é".to_owned())
    );
    assert!(first.fields.raw("unknown").is_none());
    assert!(second.fields.string_equals("model", "model/v1").unwrap());
    let resolved = second.fields.get::<String>("repo").unwrap().unwrap();
    assert!(!resolved.starts_with(r"\\?\"));
    assert_eq!(
        std::fs::canonicalize(&resolved).unwrap(),
        std::fs::canonicalize(&repo).unwrap()
    );
    let idea = create("/api/brainstorms").json(&json!({"title":"Idea", "templateId":"template", "mode":"invalid", "status":"invalid", "description":"", "checklist":[]})).send().await.unwrap();
    assert_eq!(idea.status(), 200);
    let idea: Card = idea.json().await.unwrap();
    for (key, value) in [
        ("mode", "analyze"),
        ("status", "todo"),
        ("repo", ""),
        ("description", ""),
    ] {
        assert!(idea.fields.string_equals(key, value).unwrap());
    }
    assert_eq!(
        idea.fields.get::<bool>("brainstormSession").unwrap(),
        Some(true)
    );
    for body in [
        json!({"title":"Bad", "repo":file}),
        json!({"title":"Bad", "repo":repo.join("missing")}),
        json!({"title":"Bad", "repo":"x\u{0}y"}),
        json!({"title":"Bad", "templateId":"missing"}),
        json!({"title":"Bad", "model":false}),
    ] {
        assert_eq!(
            create("/api/cards")
                .json(&body)
                .send()
                .await
                .unwrap()
                .status(),
            400
        );
    }
    let failed = create("/api/cards")
        .json(&json!({"title":"fail"}))
        .send()
        .await
        .unwrap();
    assert_eq!(failed.status(), 500);
    assert_eq!(
        failed.json::<Value>().await.unwrap(),
        json!({"error":"message.card_creation_failed"})
    );
    stop(&address, &token, task).await;
    let (address, token, task) = start(data.path(), assets.path()).await;
    let board: Board = client
        .get(format!("{address}/api/state"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(board.cards.len(), 2);
    assert_eq!(board.brainstorms.len(), 1);
    assert_eq!(board.brainstorms[0].id, idea.id);
    let saved = board.cards.iter().find(|card| card.id == first.id).unwrap();
    let criteria = saved
        .fields
        .get::<Vec<Document>>("checklist")
        .unwrap()
        .unwrap();
    assert_eq!(
        criteria[0].get::<String>("text").unwrap(),
        Some("è".to_owned())
    );
    stop(&address, &token, task).await;
}

#[tokio::test]
async fn template_routes_serialize_the_limit_and_preserve_it_across_restart() {
    let data = tempfile::tempdir().unwrap();
    let assets = tempfile::tempdir().unwrap();
    let (address, token, task) = start(data.path(), assets.path()).await;
    let client = reqwest::Client::new();
    let card: Value = client
        .post(format!("{address}/api/cards"))
        .bearer_auth(&token)
        .header("x-local-request", "1")
        .json(&json!({"title":"Template source"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let responses = futures_util::future::join_all((0..55).map(|index| {
        client
            .post(format!("{address}/api/templates"))
            .bearer_auth(&token)
            .header("x-local-request", "1")
            .json(&json!({"name":format!("Template {index}"), "cardId":card["id"]}))
            .send()
    }))
    .await;
    let mut ids = Vec::new();
    for response in responses {
        let response = response.unwrap();
        if response.status().is_success() {
            let template: Value = response.json().await.unwrap();
            ids.push(template["id"].as_str().unwrap().to_owned());
            assert_eq!(template["mode"], "edit");
        } else {
            assert_eq!(response.status(), 400);
        }
    }
    assert_eq!(ids.len(), 50);
    stop(&address, &token, task).await;
    let (address, token, task) = start(data.path(), assets.path()).await;
    for id in [&ids[0], &"missing".to_owned()] {
        assert_eq!(
            client
                .delete(format!("{address}/api/templates/{id}"))
                .bearer_auth(&token)
                .header("x-local-request", "1")
                .json(&json!({}))
                .send()
                .await
                .unwrap()
                .status(),
            200
        );
    }
    for expected in [200, 400] {
        assert_eq!(
            client
                .post(format!("{address}/api/templates"))
                .bearer_auth(&token)
                .header("x-local-request", "1")
                .json(&json!({"name":"Again", "cardId":card["id"]}))
                .send()
                .await
                .unwrap()
                .status(),
            expected
        );
    }
    assert_eq!(
        client
            .post(format!("{address}/api/templates"))
            .bearer_auth(&token)
            .header("x-local-request", "1")
            .json(&json!({"name":"Missing", "cardId":"missing"}))
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
    stop(&address, &token, task).await;
}

#[tokio::test]
async fn time_routes_and_text_identifiers_reject_invalid_requests_before_writing() {
    let data = tempfile::tempdir().unwrap();
    let assets = tempfile::tempdir().unwrap();
    let (address, token, task) = start(data.path(), assets.path()).await;
    let client = reqwest::Client::new();
    let post = |path: &str| {
        client
            .post(format!("{address}{path}"))
            .bearer_auth(&token)
            .header("x-local-request", "1")
    };
    for path in ["/api/time", "/api/templates"] {
        for id in [
            json!(true),
            json!(1),
            json!([]),
            json!({}),
            json!(null),
            json!(" "),
        ] {
            let response = post(path)
                .json(&json!({
                    "name":"Synthetic", "cardId":id, "seconds":60,
                    "date":"2026-10-30", "startTime":"10:00:00", "description":"Synthetic"
                }))
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), 400);
            assert_eq!(
                response.json::<Value>().await.unwrap()["error"],
                "message.invalid_card_id"
            );
        }
    }
    for path in ["/api/time/missing/unknown", "/api/time/missing/%65xport"] {
        let response = post(path).json(&json!({})).send().await.unwrap();
        assert_eq!(response.status(), 400);
        assert_eq!(
            response.json::<Value>().await.unwrap()["error"],
            "message.unknown_time_route"
        );
    }
    let board: Value = client
        .get(format!("{address}/api/state"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(board["times"], json!([]));
    assert!(
        board["settings"]["taskTemplates"]
            .as_array()
            .is_none_or(Vec::is_empty)
    );
    let response = post("/api/cards")
        .json(&json!({"title":"Synthetic", "unused":"é".repeat(110_000)}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    stop(&address, &token, task).await;
}

#[tokio::test]
async fn events_authenticate_notify_commits_reconnect_and_end_on_shutdown() {
    let data = tempfile::tempdir().unwrap();
    let assets = tempfile::tempdir().unwrap();
    let (address, token, task) = start(data.path(), assets.path()).await;
    let client = reqwest::Client::new();
    assert_eq!(
        client
            .get(format!("{address}/api/events?session={token}"))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    assert_eq!(
        client
            .get(format!("{address}/api/events/session"))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    let credentials = client
        .get(format!("{address}/api/events/session"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(credentials.status(), 200);
    let cookie = credentials.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .to_owned();
    assert!(cookie.contains("HttpOnly; SameSite=Strict; Path=/api/events"));
    let cookie = cookie.split(';').next().unwrap();
    assert!(!cookie.contains(&token));
    for path in ["/api/state", "/api/events/session"] {
        assert_eq!(
            client
                .get(format!("{address}{path}"))
                .header("cookie", cookie)
                .send()
                .await
                .unwrap()
                .status(),
            401
        );
    }
    let url = format!("{address}/api/events");
    assert_eq!(
        client
            .get(&url)
            .header("cookie", cookie)
            .bearer_auth("wrong")
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    assert_eq!(
        client
            .get(&url)
            .header("origin", "https://foreign.example")
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    let mut response = client
        .get(&url)
        .header("cookie", cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(response.headers()["content-type"], "text/event-stream");
    assert_eq!(response.headers()["x-accel-buffering"], "no");
    assert_eq!(response.headers()["cache-control"], "no-store");
    let mut buffered = String::new();
    let connected = next_event(&mut response, &mut buffered).await;
    assert!(connected.contains("retry: 1000\n"));
    assert!(connected.contains("data: connected\n"));
    assert_eq!(
        next_event(&mut response, &mut buffered).await,
        ": heartbeat\n\n"
    );
    assert_eq!(
        client
            .post(format!("{address}/api/cards"))
            .bearer_auth(&token)
            .header("x-local-request", "1")
            .json(&json!({"title":""}))
            .send()
            .await
            .unwrap()
            .status(),
        400
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(150), response.chunk())
            .await
            .is_err()
    );
    let created: Value = client
        .post(format!("{address}/api/cards"))
        .bearer_auth(&token)
        .header("x-local-request", "1")
        .json(&json!({"title":"synthetic event"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        next_event(&mut response, &mut buffered).await,
        "data: changed\n\n"
    );
    let board: Value = client
        .get(format!("{address}/api/state"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(board["cards"][0]["id"], created["id"]);
    drop(response);
    let mut response = client
        .get(format!("{address}/api/events"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    buffered.clear();
    assert!(
        next_event(&mut response, &mut buffered)
            .await
            .contains("data: connected\n")
    );
    stop(&address, &token, task).await;
    assert!(
        tokio::time::timeout(Duration::from_secs(2), response.chunk())
            .await
            .unwrap()
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn authenticated_http_preserves_tasks_after_close_and_rotates_sessions() {
    let data = tempfile::tempdir().unwrap();
    let assets = tempfile::tempdir().unwrap();
    std::fs::write(
        assets.path().join("index.html"),
        "<!doctype html><title>Fixture</title>",
    )
    .unwrap();
    std::fs::write(assets.path().join("local-session"), "must not be served").unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    assert!(
        http::serve(listener, data.path().to_owned(), assets.path().to_owned())
            .await
            .is_err()
    );
    assert!(!data.path().join("local-session").exists());
    std::fs::remove_file(assets.path().join("local-session")).unwrap();
    let (address, token, task) = start(data.path(), assets.path()).await;
    let client = reqwest::Client::new();
    for path in [
        "/api/state",
        "/api/health",
        "/api/events",
        "/api/state?session=ignored",
    ] {
        assert_eq!(
            client
                .get(format!("{address}{path}"))
                .send()
                .await
                .unwrap()
                .status(),
            401
        );
    }
    for (header, value) in [
        ("host", "foreign.example"),
        ("origin", "https://foreign.example"),
    ] {
        assert_eq!(
            client
                .get(format!("{address}/api/state"))
                .bearer_auth(&token)
                .header(header, value)
                .send()
                .await
                .unwrap()
                .status(),
            403
        );
    }
    let challenge = "a".repeat(64);
    let health: Value = client
        .get(format!("{address}/api/health?challenge={challenge}"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let port = address.rsplit(':').next().unwrap().parse().unwrap();
    assert_eq!(
        health,
        json!({"proof": session::proof(&token, &challenge, port).unwrap()})
    );
    let index = client.get(&address).send().await.unwrap();
    assert_eq!(index.headers()["content-security-policy"], http::CSP);
    assert!(!http::CSP.contains("'unsafe-eval'"));
    assert!(!http::CSP.contains("'unsafe-inline'"));
    for path in [
        "/local-session",
        "/data/local-session",
        "/Cargo.toml",
        "/api/missing",
    ] {
        assert_eq!(
            client
                .get(format!("{address}{path}"))
                .bearer_auth(&token)
                .send()
                .await
                .unwrap()
                .status(),
            404
        );
    }
    assert_eq!(
        client
            .post(format!("{address}/api/cards"))
            .bearer_auth(&token)
            .json(&json!({"title":"Refusée"}))
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    for body in [
        "[]".to_owned(),
        "null".to_owned(),
        "{".to_owned(),
        "{\"title\":\" \"}".to_owned(),
    ] {
        assert_eq!(
            client
                .post(format!("{address}/api/cards"))
                .bearer_auth(&token)
                .header("x-local-request", "1")
                .header("content-type", "application/json")
                .body(body)
                .send()
                .await
                .unwrap()
                .status(),
            400
        );
    }
    assert_eq!(
        client
            .post(format!("{address}/api/cards"))
            .bearer_auth(&token)
            .header("x-local-request", "1")
            .json(&json!({"title":"a".repeat(400_001)}))
            .send()
            .await
            .unwrap()
            .status(),
        413
    );
    let created = client
        .post(format!("{address}/api/cards"))
        .bearer_auth(&token)
        .header("x-local-request", "1")
        .json(&json!({"title":" Tâche 🔧 <script> "}))
        .send()
        .await
        .unwrap();
    assert_eq!(created.status(), 200);
    let card: Value = created.json().await.unwrap();
    assert_eq!(card["title"], "Tâche 🔧 <script>");
    assert_eq!(card["createdAt"].as_str().unwrap().len(), 24);
    stop(&address, &token, task).await;
    let (address, next_token, task) = start(data.path(), assets.path()).await;
    assert!(!session::same_secret(&token, &next_token));
    assert_eq!(
        client
            .get(format!("{address}/api/state"))
            .bearer_auth(token)
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    let board: Value = client
        .get(format!("{address}/api/state"))
        .bearer_auth(&next_token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let mut board = board;
    assert!(
        board
            .as_object_mut()
            .unwrap()
            .remove("revision")
            .unwrap()
            .is_u64()
    );
    assert_eq!(
        board,
        json!({"cards":[card], "brainstorms":[], "projects":[], "runs":[], "times":[], "settings": atelier_core::application::settings::public_config(&Default::default(), &Default::default(), atelier_app::infrastructure::settings::platform()).unwrap()})
    );
    stop(&address, &next_token, task).await;
}

#[tokio::test]
async fn http_and_restart_preserve_unicode_and_unknown_json_keys() {
    use atelier_core::{contracts::Board, document::Document};
    let data = tempfile::tempdir().unwrap();
    let assets = tempfile::tempdir().unwrap();
    std::fs::write(
        assets.path().join("index.html"),
        "<!doctype html><title>Fixture</title>",
    )
    .unwrap();
    let (address, token, server) = start(data.path(), assets.path()).await;
    let response = reqwest::Client::new()
        .post(format!("{address}/api/cards"))
        .bearer_auth(&token)
        .header("x-local-request", "1")
        .header("content-type", "application/json")
        .body(r#"{"title":"\ufeff\u00e9\u00a0"}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let mut card: Card = response.json().await.unwrap();
    assert_eq!(card.title, "é".to_owned());
    stop(&address, &token, server).await;
    card.fields =
        serde_json::from_str::<Document>(r#"{"\u00e9":{"unknown":"\u00e8"},"nullable":null}"#)
            .unwrap();
    let database = Database::open(&data.path().join("kanban.sqlite")).unwrap();
    database.put_card(card).await.unwrap();
    database.close().await.unwrap();
    let (address, token, server) = start(data.path(), assets.path()).await;
    let response = reqwest::Client::new()
        .get(format!("{address}/api/state"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let raw = response.text().await.unwrap();
    assert!(raw.contains(r#""é":{"unknown":"\u00e8"}"#));
    let board: Board = serde_json::from_str(&raw).unwrap();
    assert_eq!(board.cards[0].title, "é".to_owned());
    let client = reqwest::Client::new();
    let duplicate = || {
        client
            .post(format!(
                "{address}/api/cards/{}/duplicate",
                board.cards[0].id
            ))
            .bearer_auth(&token)
            .header("x-local-request", "1")
            .json(&json!({}))
    };
    let (first, second) = tokio::join!(duplicate().send(), duplicate().send());
    let first = first.unwrap();
    let second = second.unwrap();
    assert_eq!(first.status(), 200);
    assert_eq!(second.status(), 200);
    let first: Card = first.json().await.unwrap();
    let second: Card = second.json().await.unwrap();
    assert_ne!(first.id, second.id);
    let mut copied_title = String::from("Copie : ");
    copied_title.push('é');
    assert_eq!(first.title, copied_title);
    assert_eq!(second.title, copied_title);
    assert!(first.fields.string_equals("status", "todo").unwrap());
    assert!(first.fields.raw("nullable").is_none());
    let missing = client
        .post(format!("{address}/api/cards/missing/duplicate"))
        .bearer_auth(&token)
        .header("x-local-request", "1")
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(missing.status(), 404);
    assert_eq!(
        missing.json::<Value>().await.unwrap(),
        json!({"error":"message.card_not_found"})
    );
    let missing_files = client
        .get(format!("{address}/api/cards/missing/files"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(missing_files.status(), 404);
    assert_eq!(
        missing_files.json::<Value>().await.unwrap(),
        json!({"error":"message.card_not_found"})
    );
    stop(&address, &token, server).await;
    let database = Database::open(&data.path().join("kanban.sqlite")).unwrap();
    let saved = database.cards().await.unwrap();
    assert_eq!(saved.len(), 3);
    assert_eq!(saved[2].title, "é".to_owned());
    assert!(saved[2].fields.raw("nullable").is_some());
    database.close().await.unwrap();
}

#[tokio::test]
async fn database_preserves_unknown_fields_null_and_row_order() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("test.sqlite");
    let database = Database::open(&path).unwrap();
    let first: Card =
        serde_json::from_str(r#"{"id":"1", "title":"é", "extra":{"nullable":null}}"#).unwrap();
    let second: Card = serde_json::from_str(r#"{"id":"2", "title":"two"}"#).unwrap();
    database.put_card(first.clone()).await.unwrap();
    database.put_card(second.clone()).await.unwrap();
    let mut updated = first;
    updated.title = "changed".into();
    database.put_card(updated.clone()).await.unwrap();
    database.close().await.unwrap();
    let reopened = Database::open(&path).unwrap();
    assert_eq!(
        serde_json::to_value(reopened.cards().await.unwrap()).unwrap(),
        serde_json::to_value(vec![second, updated]).unwrap()
    );
    reopened.close().await.unwrap();
}

#[test]
fn prototype_refuses_existing_data_and_locks_aliases_until_closed() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("kanban.sqlite"), "untouched").unwrap();
    assert!(PrototypeDirectory::open(directory.path()).is_err());
    assert_eq!(
        std::fs::read_to_string(directory.path().join("kanban.sqlite")).unwrap(),
        "untouched"
    );
    let empty = tempfile::tempdir().unwrap();
    let first = PrototypeDirectory::open(empty.path()).unwrap();
    assert!(PrototypeDirectory::open(&empty.path().join(".")).is_err());
    drop(first);
    assert!(PrototypeDirectory::open(empty.path()).is_ok());
}

#[tokio::test]
async fn dropping_database_drains_an_accepted_write_after_caller_cancellation() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("cancelled.sqlite");
    let database = Database::open(&path).unwrap();
    let (started, ready) = tokio::sync::oneshot::channel();
    let (release, waiting) = std::sync::mpsc::channel();
    let mut operation = Box::pin(database.call(move |store| {
        started.send(()).unwrap();
        waiting.recv().unwrap();
        store.save_settings(&json!({"accepted":true}))
    }));
    tokio::select! {
        _ = ready => {},
        _ = &mut operation => panic!("Write finished before release"),
    }
    drop(operation);
    release.send(()).unwrap();
    drop(database);
    let reopened = Database::open(&path).unwrap();
    assert_eq!(
        reopened
            .call(|store| store.settings::<serde_json::Value>())
            .await
            .unwrap(),
        json!({"accepted":true})
    );
    reopened.close().await.unwrap();
}

#[test]
fn session_tokens_and_constant_time_comparison_reject_malformed_values() {
    // Generated with the existing Node sessionProof, not with this Rust implementation.
    assert_eq!(
        session::proof(&"a".repeat(64), &"b".repeat(64), 4318).unwrap(),
        "1ae8821520411244274c996fe46f3a9ee07c312cc03462a73e34a72c949d7bfb"
    );
    let directory = tempfile::tempdir().unwrap();
    let token = session::new_token().unwrap();
    session::write(directory.path(), &token).unwrap();
    assert!(session::same_secret(
        &token,
        &session::read(directory.path()).unwrap()
    ));
    assert!(!session::same_secret(
        &token,
        &session::new_token().unwrap()
    ));
    assert!(!session::same_secret(&token, &"é".repeat(64)));
    session::write(directory.path(), "invalid").unwrap();
    assert!(session::read(directory.path()).is_err());
}

#[cfg(windows)]
#[test]
fn dpapi_is_compatible_with_powershell_in_both_directions() {
    use std::os::windows::process::CommandExt;
    use std::{
        io::Write,
        process::{Command, Stdio},
    };
    fn powershell(input: &str, decrypt: bool) -> String {
        let operation = if decrypt {
            "$secure = ConvertTo-SecureString -String $value; [Console]::Write(([System.Net.NetworkCredential]::new('', $secure)).Password)"
        } else {
            "$secure = ConvertTo-SecureString -String $value -AsPlainText -Force; [Console]::Write((ConvertFrom-SecureString -SecureString $secure))"
        };
        let script = format!(
            "$ErrorActionPreference='Stop'; Import-Module ([System.Reflection.Assembly]::LoadWithPartialName('Microsoft.PowerShell.Security').Location); [Console]::InputEncoding=New-Object System.Text.UTF8Encoding($false); [Console]::OutputEncoding=New-Object System.Text.UTF8Encoding($false); $value=[Console]::In.ReadToEnd(); {operation}"
        );
        let executable = std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap())
            .join("System32/WindowsPowerShell/v1.0/powershell.exe");
        let mut child = Command::new(executable)
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .creation_flags(0x08000000)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "PowerShell DPAPI test failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }
    let plaintext = "Secret synthétique 🔧 uniquement";
    let encrypted = session::protect(plaintext, false).unwrap();
    assert_eq!(powershell(&encrypted, true), plaintext);
    assert_eq!(
        session::protect(&powershell(plaintext, false), true).unwrap(),
        plaintext
    );
    assert!(session::protect("bad", true).is_err());
    assert!(session::protect("00", true).is_err());
}
