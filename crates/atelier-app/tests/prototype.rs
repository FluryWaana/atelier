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

async fn stop(address: &str, token: &str, task: tokio::task::JoinHandle<atelier_app::Result<()>>) {
    assert!(
        reqwest::Client::new()
            .post(format!("{address}/api/prototype/stop"))
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
async fn events_authenticate_notify_commits_reconnect_and_end_on_shutdown() {
    let data = tempfile::tempdir().unwrap();
    let assets = tempfile::tempdir().unwrap();
    let (address, token, task) = start(data.path(), assets.path()).await;
    let client = reqwest::Client::new();
    let url = format!("{address}/api/events?session={token}");
    assert_eq!(
        client
            .get(&url)
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
    let mut response = client.get(&url).send().await.unwrap();
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
            .json(&json!({"title":"a".repeat(100_001)}))
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
    assert_eq!(board, json!({"cards":[card]}));
    stop(&address, &next_token, task).await;
}

#[tokio::test]
async fn http_and_restart_preserve_isolated_utf16_and_unknown_json_keys() {
    use atelier_core::{
        contracts::Board,
        document::{Document, JsonString},
    };
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
        .body(r#"{"title":"\ufeff\ud83d\u00a0"}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let mut card: Card = response.json().await.unwrap();
    assert_eq!(card.title, JsonString(vec![0xd83d]));
    stop(&address, &token, server).await;
    card.fields =
        serde_json::from_str::<Document>(r#"{"\ud800":{"unknown":"\udc00"},"nullable":null}"#)
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
    assert!(raw.contains(r#""\ud800":{"unknown":"\udc00"}"#));
    let board: Board = serde_json::from_str(&raw).unwrap();
    assert_eq!(board.cards[0].title, JsonString(vec![0xd83d]));
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
    let mut copied_title = JsonString::from("Copie — ");
    copied_title.0.push(0xd83d);
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
        json!({"error":"Carte introuvable."})
    );
    stop(&address, &token, server).await;
    let database = Database::open(&data.path().join("kanban.sqlite")).unwrap();
    let saved = database.cards().await.unwrap();
    assert_eq!(saved.len(), 3);
    assert_eq!(saved[2].title, JsonString(vec![0xd83d]));
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
